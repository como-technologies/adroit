//! Opt-in forge / tracker integration: drive the *process* lifecycle (a tracker
//! issue + a code-review PR/MR) alongside the ADR's *decision* lifecycle.
//!
//! Two trait objects model the two roles — [`Forge`] (the PR/MR host) and
//! [`Tracker`] (the issue host). A *same-system* provider (GitHub, GitLab)
//! implements both over one client and one token; a *split* setup (e.g. GitLab
//! MRs + Jira issues) returns two different adapters. [`open`] is the single
//! place the provider is matched.
//!
//! This whole module is gated behind the `forge` Cargo feature so the core CLI
//! stays synchronous and HTTP-free. The always-compiled facade in
//! [`crate::forge_hook`] is what `main` calls (it no-ops when the feature is
//! off), so verbs never carry `#[cfg]` or `if forge_enabled`.

use std::io::Read;

use serde::Serialize;

use crate::adr::Status;
use crate::config::{Config, ForgeConfig, Provider};

pub mod github;
pub mod noop;

// ---------------------------------------------------------------------------
// Value types (framework-free, serde-derived so Phase 3 can embed them in views)
// ---------------------------------------------------------------------------

/// A created or known tracker issue.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IssueRef {
    /// Forge-native id (e.g. a GitHub issue number as a string).
    pub id: String,
    /// Browser URL, written into the ADR's `## References`.
    pub url: String,
    pub title: String,
}

/// A created or known pull/merge request.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrRef {
    pub id: String,
    pub url: String,
    /// Head branch the PR was opened from (`adr/0021-slug`).
    pub branch: String,
}

/// Whether a tracker issue is open, plus its URL.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IssueState {
    pub open: bool,
    pub url: String,
}

/// A PR's review / CI / merge snapshot (the `accepted` guard + Phase 3 reads).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrState {
    /// Count of approving reviews.
    pub approvals: u32,
    pub ci: CiStatus,
    pub merged: bool,
    pub draft: bool,
}

/// Rollup of a PR's CI checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Success,
    Pending,
    Failure,
    /// No checks configured / reported.
    None,
}

/// Target state for a tracker transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Done,
    WontFix,
    Reopen,
}

/// The inputs for opening a draft PR over an already-pushed branch.
#[derive(Debug, Clone)]
pub struct PrDraft {
    pub branch: String,
    pub base: String,
    pub title: String,
    pub body: String,
}

/// What went wrong talking to a forge.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    /// Network / connectivity failure — callers warn-once and continue (the ADR
    /// is already written locally).
    #[error("forge unreachable: {0}")]
    Offline(String),
    /// Authentication failed — a loud misconfiguration, not swallowed.
    #[error("forge auth failed (check your *_TOKEN env var): {0}")]
    Auth(String),
    /// The API returned an error status.
    #[error("forge API error {status}: {message}")]
    Api { status: u16, message: String },
    /// A local git step failed.
    #[error(transparent)]
    Git(#[from] crate::git::GitError),
}

impl ForgeError {
    /// True for transient connectivity failures the graceful-offline path
    /// swallows (vs. an auth/API error, which should surface).
    pub fn is_offline(&self) -> bool {
        matches!(self, ForgeError::Offline(_))
    }
}

// ---------------------------------------------------------------------------
// Traits — the two roles. GitHub/GitLab implement both.
// ---------------------------------------------------------------------------

/// The code-review host (pull/merge requests).
pub trait Forge {
    /// Open a **draft** PR/MR for a branch that has already been pushed.
    fn open_pr(&self, draft: &PrDraft) -> Result<PrRef, ForgeError>;
    /// Current review/CI/merge snapshot for a PR id.
    fn pr_state(&self, pr: &str) -> Result<PrState, ForgeError>;
    /// Merge a PR (the adapter chooses the method, e.g. squash).
    fn merge_pr(&self, pr: &str) -> Result<(), ForgeError>;
    /// Close a PR without merging.
    fn close_pr(&self, pr: &str) -> Result<(), ForgeError>;
    /// Post a comment on a PR.
    fn comment_pr(&self, pr: &str, body: &str) -> Result<(), ForgeError>;
    /// Short label for diagnostics (e.g. `github:owner/repo`).
    fn describe(&self) -> String;
}

/// The configured adapters from [`open`]: either role may be absent (forge
/// disabled, no token, or — for now — a not-yet-wired provider).
pub type Adapters = (Option<Box<dyn Forge>>, Option<Box<dyn Tracker>>);

/// The issue tracker.
pub trait Tracker {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError>;
    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError>;
    fn close_issue(&self, issue: &str) -> Result<(), ForgeError>;
    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError>;
    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError>;
    fn describe(&self) -> String;
}

// ---------------------------------------------------------------------------
// HTTP transport seam — adapters depend on this, not on ureq directly, so tests
// inject a fake transport and never hit the network.
// ---------------------------------------------------------------------------

/// A minimal blocking HTTP response (status + raw body).
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Blocking HTTP, abstracted so adapters are testable with a fake.
pub trait HttpTransport: Send + Sync {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, ForgeError>;
}

/// Production transport over `ureq` (blocking, rustls). A non-2xx status is
/// returned as a normal [`HttpResponse`] (so adapters can map 401→`Auth`,
/// 4xx/5xx→`Api`); only a connection-level failure becomes [`ForgeError::Offline`].
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, ForgeError> {
        let mut req = ureq::request(method, url);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let result = match body {
            Some(b) => req.send_bytes(b),
            None => req.call(),
        };
        match result {
            Ok(resp) => Ok(read_response(resp)),
            // A non-2xx still carries a response we can read (status + body).
            Err(ureq::Error::Status(_, resp)) => Ok(read_response(resp)),
            // Connection refused / DNS / TLS / timeout → offline.
            Err(ureq::Error::Transport(t)) => Err(ForgeError::Offline(t.to_string())),
        }
    }
}

fn read_response(resp: ureq::Response) -> HttpResponse {
    let status = resp.status();
    let mut body = Vec::new();
    // Best-effort body read; an unreadable body is just empty bytes.
    let _ = resp.into_reader().read_to_end(&mut body);
    HttpResponse { status, body }
}

// ---------------------------------------------------------------------------
// Factory — the ONE place a provider is matched.
// ---------------------------------------------------------------------------

/// Construct the configured `(forge, tracker)` adapters. Returns `(None, None)`
/// when the integration is disabled (`provider = none`) or no token is set, so
/// callers warn-once and continue with a local-only write.
///
/// GitHub/GitLab arms land in Phase 1/1b; for now an enabled provider with a
/// token returns `(None, None)` as well (no adapter wired yet).
pub fn open(cfg: &ForgeConfig) -> Adapters {
    // Thin dispatcher: each provider module owns its own construction (token env
    // var, slug/host requirements). Adding a provider is one arm + one module.
    match cfg.provider {
        Provider::None => (None, None),
        Provider::Github => github::open(cfg),
        // Phase 1b: gitlab::open(cfg)
        Provider::Gitlab => (None, None),
    }
}

/// Orchestrate the forge side of `adroit new`: create the tracker issue, base a
/// draft PR on an `adr/NNNN-…` branch, and record both URLs in the ADR's
/// `## References`. `dry_run` previews the plan and touches nothing.
///
/// Graceful by design: a network/offline or git failure warns and returns `Ok`
/// (the ADR is already written locally — the durable record); only an auth/API
/// error surfaces. Every write is idempotent, so re-running converges.
pub fn after_new(
    cfg: &Config,
    path: &std::path::Path,
    title: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        eprintln!(
            "adroit: --with-forge: the `{}` integration is inactive — set the repo \
             (`forge.repo`) and an auth token (e.g. ADROIT_GITHUB_TOKEN). Wrote the ADR \
             locally only.",
            fcfg.provider
        );
        return Ok(());
    };
    run_new(forge.as_ref(), tracker.as_ref(), path, title, fcfg, dry_run)
}

/// The provider-agnostic orchestration (testable with mock/noop adapters and a
/// scratch git repo). Separated from [`after_new`] so tests don't need real
/// config/env to construct an adapter.
fn run_new(
    forge: &dyn Forge,
    tracker: &dyn Tracker,
    path: &std::path::Path,
    title: &str,
    fcfg: &ForgeConfig,
    dry_run: bool,
) -> anyhow::Result<()> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("adr");
    let branch = format!("{}{stem}", fcfg.branch_prefix);
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or(stem);
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));

    if dry_run {
        println!("Forge plan ({}):", forge.describe());
        println!("  - create issue: {title:?}");
        println!(
            "  - branch {branch} off {} + commit {file} + push",
            fcfg.base_branch
        );
        println!("  - open draft PR (Closes the issue)");
        println!("  - write issue + PR URLs into ## References");
        println!("\nDry run - re-run without --dry-run to apply.");
        return Ok(());
    }

    // 1. Tracker issue. Offline -> keep local; auth/API -> surface.
    let issue_body =
        format!("Tracking issue for ADR \u{201c}{title}\u{201d} (`{file}`), managed by adroit.");
    let issue = match tracker.create_issue(title, &issue_body) {
        Ok(i) => i,
        Err(e) if e.is_offline() => {
            eprintln!("adroit: forge unreachable ({e}); wrote the ADR locally only.");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // 2. Record the issue URL first (durable even if the push later fails).
    upsert_ref(path, "Issue", &issue.url)?;

    // 3. Branch + commit + push. Any git failure -> warn, keep the issue link.
    let commit_msg = format!("ADR: {title}");
    if let Err(e) = (|| -> Result<(), crate::git::GitError> {
        crate::git::create_branch(dir, &branch)?;
        crate::git::add(dir, path)?;
        crate::git::commit(dir, &commit_msg)?;
        crate::git::push(dir, "origin", &branch)
    })() {
        eprintln!(
            "adroit: git step failed ({e}); created issue {} but skipped the PR.",
            issue.url
        );
        return Ok(());
    }

    // 4. Draft PR linking the issue.
    let pr = match forge.open_pr(&PrDraft {
        branch: branch.clone(),
        base: fcfg.base_branch.clone(),
        title: format!("ADR: {title}"),
        body: format!("ADR: {title}\n\nCloses #{}.", issue.id),
    }) {
        Ok(p) => p,
        Err(e) if e.is_offline() => {
            eprintln!(
                "adroit: forge unreachable opening PR ({e}); branch {branch} pushed, issue {} created.",
                issue.url
            );
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // 5. Record the PR URL and commit+push it too (idempotent upsert).
    upsert_ref(path, "Pull Request", &pr.url)?;
    let _ = (|| -> Result<(), crate::git::GitError> {
        crate::git::add(dir, path)?;
        crate::git::commit(dir, &format!("ADR: link PR for {title}"))?;
        crate::git::push(dir, "origin", &branch)
    })();

    println!("Forge: issue {} - PR {}", issue.url, pr.url);
    Ok(())
}

/// Read `path`, upsert a `## References` bullet, write it back (no-op if
/// unchanged).
fn upsert_ref(path: &std::path::Path, label: &str, url: &str) -> anyhow::Result<()> {
    let original = std::fs::read_to_string(path)?;
    let updated = crate::format::upsert_reference(&original, label, url);
    if updated != original {
        std::fs::write(path, updated)?;
    }
    Ok(())
}

/// The linked issue / PR `(number, url)` parsed from an ADR's `## References`.
struct ForgeRefs {
    issue: Option<(String, String)>,
    pr: Option<(String, String)>,
}

fn read_refs(path: &std::path::Path) -> ForgeRefs {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let refs = crate::format::parse_references(&content);
    let find = |label: &str| {
        refs.iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(label))
            .map(|(_, u)| u.clone())
    };
    // The trailing path segment of the URL is the number (.../issues/7, .../pull/42).
    let pair = |u: String| (u.rsplit('/').next().unwrap_or("").to_string(), u);
    ForgeRefs {
        issue: find("Issue").map(pair),
        pr: find("Pull Request").map(pair),
    }
}

/// Forge actions for `set-status`, run **before** the local move. Returns
/// `Ok(true)` to proceed with the move, `Ok(false)` to stop (preview / not
/// `--yes`), or `Err` to abort (e.g. accepting a PR that isn't approved).
pub fn before_status_change(
    cfg: &Config,
    path: &std::path::Path,
    new_status: Status,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(true);
    };
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        eprintln!(
            "adroit: --with-forge: `{}` integration inactive (set forge.repo + a token); \
             doing the local status change only.",
            fcfg.provider
        );
        return Ok(true);
    };
    run_status_change(
        forge.as_ref(),
        tracker.as_ref(),
        &read_refs(path),
        new_status,
        cfg.review_quorum,
        dry_run,
        yes,
    )
}

/// Provider-agnostic `set-status` orchestration (testable with mock adapters).
fn run_status_change(
    forge: &dyn Forge,
    tracker: &dyn Tracker,
    refs: &ForgeRefs,
    new_status: Status,
    quorum: u32,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let apply = yes && !dry_run;
    match new_status {
        Status::Accepted => {
            let Some((pr, pr_url)) = refs.pr.clone() else {
                return Ok(true); // no PR recorded → just move locally
            };
            let st = match forge.pr_state(&pr) {
                Ok(s) => s,
                Err(e) if e.is_offline() => {
                    eprintln!("adroit: forge unreachable ({e}); local status change only.");
                    return Ok(true);
                }
                Err(e) => return Err(e.into()),
            };
            let ok = st.approvals >= quorum && matches!(st.ci, CiStatus::Success | CiStatus::None);
            if !apply {
                println!("Forge plan (accept):");
                println!(
                    "  - PR {pr_url}: {}/{quorum} approvals, CI {:?}{}",
                    st.approvals,
                    st.ci,
                    if st.merged { " (already merged)" } else { "" }
                );
                if let Some((_, iu)) = &refs.issue {
                    println!("  - close issue {iu}");
                }
                if !ok && !st.merged {
                    println!("  (blocked: needs {quorum} approvals + passing CI)");
                }
                println!("\nPreview — re-run with --yes to apply.");
                return Ok(false);
            }
            if !st.merged {
                if !ok {
                    anyhow::bail!(
                        "refusing to accept: PR {pr_url} has {}/{quorum} approvals, CI {:?}",
                        st.approvals,
                        st.ci
                    );
                }
                match forge.merge_pr(&pr) {
                    Ok(()) => {}
                    Err(e) if e.is_offline() => {
                        eprintln!("adroit: forge unreachable merging PR ({e}); local change only.");
                        return Ok(true);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            if let Some((issue, _)) = &refs.issue
                && let Err(e) = tracker.close_issue(issue)
            {
                eprintln!("adroit: couldn't close issue ({e})");
            }
            println!("Forge: merged PR {pr_url}");
            Ok(true)
        }
        Status::Rejected | Status::Deprecated => {
            let verb = if new_status == Status::Rejected {
                "rejected"
            } else {
                "deprecated"
            };
            if !apply {
                println!("Forge plan ({verb}):");
                if let Some((_, pu)) = &refs.pr {
                    println!("  - close PR {pu}");
                }
                if let Some((_, iu)) = &refs.issue {
                    println!("  - mark issue {iu} won't-fix");
                }
                println!("\nPreview — re-run with --yes to apply.");
                return Ok(false);
            }
            if let Some((pr, _)) = &refs.pr {
                let _ = forge.comment_pr(pr, &format!("Closing: ADR {verb}."));
                match forge.close_pr(pr) {
                    Ok(()) => {}
                    Err(e) if e.is_offline() => {
                        eprintln!("adroit: forge unreachable ({e}); local change only.");
                        return Ok(true);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            if let Some((issue, _)) = &refs.issue {
                let _ = tracker.transition(issue, Transition::WontFix);
            }
            Ok(true)
        }
        // Proposed / Superseded: no forge action here (Superseded → `supersede`).
        _ => Ok(true),
    }
}

/// Forge actions for `supersede`, run before the local change: comment on and
/// close the *old* ADR's issue + PR. Returns `Ok(true)` to proceed locally.
pub fn on_supersede(
    cfg: &Config,
    old_path: &std::path::Path,
    new_label: &str,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(true);
    };
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        eprintln!(
            "adroit: --with-forge: `{}` integration inactive; doing the local supersede only.",
            fcfg.provider
        );
        return Ok(true);
    };
    let refs = read_refs(old_path);
    let apply = yes && !dry_run;
    if !apply {
        println!("Forge plan (supersede):");
        if let Some((_, pu)) = &refs.pr {
            println!("  - comment + close PR {pu}");
        }
        if let Some((_, iu)) = &refs.issue {
            println!("  - comment + close issue {iu}");
        }
        println!("\nPreview — re-run with --yes to apply.");
        return Ok(false);
    }
    let msg = format!("Superseded by {new_label}.");
    if let Some((pr, _)) = &refs.pr {
        let _ = forge.comment_pr(pr, &msg);
        let _ = forge.close_pr(pr);
    }
    if let Some((issue, _)) = &refs.issue {
        let _ = tracker.comment_issue(issue, &msg);
        let _ = tracker.transition(issue, Transition::Done);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ForgeConfig;

    #[test]
    fn open_disabled_provider_yields_no_adapters() {
        let (f, t) = open(&ForgeConfig::default()); // provider = none
        assert!(f.is_none() && t.is_none());
    }

    #[test]
    fn noop_adapters_succeed_without_side_effects() {
        let f = noop::NoopForge;
        let t = noop::NoopTracker;
        let issue = t.create_issue("Title", "body").unwrap();
        assert_eq!(issue.url, "(dry-run)");
        let pr = f
            .open_pr(&PrDraft {
                branch: "adr/0001-x".into(),
                base: "main".into(),
                title: "x".into(),
                body: "y".into(),
            })
            .unwrap();
        assert_eq!(pr.branch, "adr/0001-x");
        assert!(!f.pr_state("1").unwrap().merged);
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
    }

    #[test]
    fn run_new_records_issue_reference_and_survives_a_remoteless_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "T"]);
        std::fs::write(dir.join("seed"), "x").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "seed"]);

        let adr = dir.join("0001-x.md");
        std::fs::write(&adr, "# ADR-0001: X\n\n## Status\n\nProposed\n").unwrap();

        // No `origin` remote → the push fails → graceful Ok; the issue link is
        // still recorded (durable-record-first ordering).
        run_new(
            &noop::NoopForge,
            &noop::NoopTracker,
            &adr,
            "X",
            &ForgeConfig::default(),
            false,
        )
        .unwrap();

        let body = std::fs::read_to_string(&adr).unwrap();
        assert!(body.contains("## References"), "got:\n{body}");
        assert!(body.contains("- Issue: (dry-run)"), "got:\n{body}");
    }

    #[test]
    fn run_new_dry_run_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let adr = tmp.path().join("0001-x.md");
        let original = "# ADR-0001: X\n\n## Status\n\nProposed\n";
        std::fs::write(&adr, original).unwrap();
        run_new(
            &noop::NoopForge,
            &noop::NoopTracker,
            &adr,
            "X",
            &ForgeConfig::default(),
            true,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&adr).unwrap(), original);
    }

    /// A forge whose PR state is configurable and that records a merge.
    struct MockForge {
        state: PrState,
        merged: std::cell::Cell<bool>,
    }
    impl Forge for MockForge {
        fn open_pr(&self, d: &PrDraft) -> Result<PrRef, ForgeError> {
            Ok(PrRef {
                id: "1".into(),
                url: "u".into(),
                branch: d.branch.clone(),
            })
        }
        fn pr_state(&self, _: &str) -> Result<PrState, ForgeError> {
            Ok(self.state.clone())
        }
        fn merge_pr(&self, _: &str) -> Result<(), ForgeError> {
            self.merged.set(true);
            Ok(())
        }
        fn close_pr(&self, _: &str) -> Result<(), ForgeError> {
            Ok(())
        }
        fn comment_pr(&self, _: &str, _: &str) -> Result<(), ForgeError> {
            Ok(())
        }
        fn describe(&self) -> String {
            "mock".into()
        }
    }

    fn refs_with_pr() -> ForgeRefs {
        ForgeRefs {
            issue: Some(("7".into(), "issue-url".into())),
            pr: Some(("42".into(), "pr-url".into())),
        }
    }
    fn pr_state(approvals: u32, ci: CiStatus) -> PrState {
        PrState {
            approvals,
            ci,
            merged: false,
            draft: true,
        }
    }

    #[test]
    fn accept_refuses_below_quorum() {
        let forge = MockForge {
            state: pr_state(1, CiStatus::Success),
            merged: false.into(),
        };
        let err = run_status_change(
            &forge,
            &noop::NoopTracker,
            &refs_with_pr(),
            Status::Accepted,
            3,
            false,
            true, // --yes
        )
        .unwrap_err();
        assert!(err.to_string().contains("refusing to accept"));
        assert!(!forge.merged.get());
    }

    #[test]
    fn accept_merges_when_approved_and_green() {
        let forge = MockForge {
            state: pr_state(3, CiStatus::Success),
            merged: false.into(),
        };
        let proceed = run_status_change(
            &forge,
            &noop::NoopTracker,
            &refs_with_pr(),
            Status::Accepted,
            3,
            false,
            true,
        )
        .unwrap();
        assert!(proceed); // local move proceeds
        assert!(forge.merged.get()); // PR merged
    }

    #[test]
    fn accept_without_yes_previews_and_does_not_merge() {
        let forge = MockForge {
            state: pr_state(3, CiStatus::Success),
            merged: false.into(),
        };
        let proceed = run_status_change(
            &forge,
            &noop::NoopTracker,
            &refs_with_pr(),
            Status::Accepted,
            3,
            false,
            false, // no --yes → preview
        )
        .unwrap();
        assert!(!proceed); // stop: no local move
        assert!(!forge.merged.get()); // no merge
    }
}
