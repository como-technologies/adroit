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
use serde_json::Value;

use crate::adr::Status;
use crate::config::{Config, ForgeConfig, Provider, TrackerProvider};

pub mod github;
pub mod gitlab;
pub mod jira;
pub mod linear;
pub mod monday;
pub mod noop;
pub mod oauth;

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
    /// OAuth **device-flow login** failed (bad client id, device flow not enabled,
    /// denied/expired) — distinct from a token-auth failure, so the message points
    /// at `forge.oauth_client_id` rather than `*_TOKEN`.
    #[error("forge OAuth login failed: {0}")]
    OAuth(String),
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
    /// Replace a PR's description/body (MR-description sync, relink URL patch).
    fn set_pr_body(&self, pr: &str, body: &str) -> Result<(), ForgeError>;
    /// Add a label to a PR/MR (best-effort review-deadline marker). Default no-op
    /// for hosts without a label API; the GitHub/GitLab adapters override it.
    fn add_label(&self, _pr: &str, _label: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    /// Mark a **draft** PR/MR ready for review (un-draft it), so it can be
    /// reviewed and merged. Best-effort and idempotent (un-drafting a ready PR is
    /// a no-op); default no-op for hosts without the concept.
    fn mark_ready(&self, _pr: &str) -> Result<(), ForgeError> {
        Ok(())
    }
    /// List a PR/MR's existing comments (for idempotent upsert). Default empty — a
    /// host that doesn't override this falls back to always creating.
    fn comments_on_pr(&self, _pr: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        Ok(Vec::new())
    }
    /// Edit an existing PR/MR comment by id. Default no-op (no edit endpoint).
    fn update_pr_comment(
        &self,
        _pr: &str,
        _comment_id: &str,
        _body: &str,
    ) -> Result<(), ForgeError> {
        Ok(())
    }
    /// Post `body` on a PR/MR **idempotently**: edit the comment tagged with
    /// `marker` in place (no-op if unchanged), else create one. Converges instead
    /// of accumulating, so re-running `review`/`set-review --forge` never spams.
    fn upsert_pr_comment(&self, pr: &str, marker: &str, body: &str) -> Result<(), ForgeError> {
        let tagged = tag_body(marker, body);
        match plan_upsert(&self.comments_on_pr(pr)?, marker, &tagged) {
            UpsertAction::Noop => Ok(()),
            UpsertAction::Update(id) => self.update_pr_comment(pr, &id, &tagged),
            UpsertAction::Create => self.comment_pr(pr, &tagged),
        }
    }
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
    /// Set (`Some`) or clear (`None`) the issue's native due/target date, an ISO
    /// `YYYY-MM-DD` string. Best-effort; default no-op for trackers without a
    /// due-date field (e.g. GitHub Issues). Jira/GitLab/Linear/monday override it.
    fn set_due_date(&self, _issue: &str, _date: Option<&str>) -> Result<(), ForgeError> {
        Ok(())
    }
    /// List an issue's existing comments (for idempotent upsert). Default empty.
    fn comments_on_issue(&self, _issue: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        Ok(Vec::new())
    }
    /// Edit an existing issue comment by id. Default no-op (no edit endpoint).
    fn update_issue_comment(
        &self,
        _issue: &str,
        _comment_id: &str,
        _body: &str,
    ) -> Result<(), ForgeError> {
        Ok(())
    }
    /// Post `body` on an issue **idempotently** (see [`Forge::upsert_pr_comment`]).
    fn upsert_issue_comment(
        &self,
        issue: &str,
        marker: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        let tagged = tag_body(marker, body);
        match plan_upsert(&self.comments_on_issue(issue)?, marker, &tagged) {
            UpsertAction::Noop => Ok(()),
            UpsertAction::Update(id) => self.update_issue_comment(issue, &id, &tagged),
            UpsertAction::Create => self.comment_issue(issue, &tagged),
        }
    }
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
        // ureq 3 reports 4xx/5xx as `Err` by default; disable that so a non-2xx
        // still comes back as a normal response (adapters map 401→Auth, 4xx/5xx→Api).
        // Bound every request (connect + overall) so a network hang — common on
        // WSL2, where DNS/connect to a forge can stall — surfaces as a clean
        // `Offline` error instead of freezing (e.g. the OAuth device-flow poll loop)
        // with no overall timeout of its own.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(std::time::Duration::from_secs(20)))
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build()
            .into();
        let mut builder = ureq::http::Request::builder().method(method).uri(url);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let request = builder
            .body(body.unwrap_or(&[]).to_vec())
            .map_err(|e| ForgeError::Offline(e.to_string()))?;
        match agent.run(request) {
            Ok(resp) => Ok(read_response(resp)),
            // Connection refused / DNS / TLS / timeout → offline.
            Err(e) => Err(ForgeError::Offline(e.to_string())),
        }
    }
}

fn read_response(resp: ureq::http::Response<ureq::Body>) -> HttpResponse {
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    // Best-effort body read; an unreadable body is just empty bytes.
    let _ = resp.into_body().into_reader().read_to_end(&mut body);
    HttpResponse { status, body }
}

/// Run one REST call: serialize `body`, send via `transport`, classify the
/// status (2xx ok; 401/403 → Auth; else Api), return `Value::Null` for an empty
/// 2xx body, else parse JSON. `label` names the provider in the parse-error
/// message; `extract_error` pulls the provider's error text from a failed body.
pub(crate) fn rest_call(
    transport: &dyn HttpTransport,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
    label: &str,
    extract_error: fn(&[u8]) -> String,
) -> Result<Value, ForgeError> {
    let bytes = body.map(|b| serde_json::to_vec(&b).expect("serialize JSON body"));
    let resp = transport.request(method, url, headers, bytes.as_deref())?;
    match resp.status {
        200..=299 => {}
        401 | 403 => return Err(ForgeError::Auth(extract_error(&resp.body))),
        status => {
            return Err(ForgeError::Api {
                status,
                message: extract_error(&resp.body),
            });
        }
    }
    if resp.body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&resp.body).map_err(|e| ForgeError::Api {
        status: resp.status,
        message: format!("invalid JSON from {label}: {e}"),
    })
}

/// Require a string field from a JSON response, else an `Api` error naming the
/// missing key (`label` = the provider, e.g. "GitHub").
pub(crate) fn want_str(v: &Value, key: &str, label: &str) -> Result<String, ForgeError> {
    v[key].as_str().map(str::to_string).ok_or(ForgeError::Api {
        status: 0,
        message: format!("{label} response missing `{key}`"),
    })
}

/// Require a numeric id field, rendered as a string.
pub(crate) fn want_num(v: &Value, key: &str, label: &str) -> Result<String, ForgeError> {
    v[key]
        .as_u64()
        .map(|n| n.to_string())
        .ok_or(ForgeError::Api {
            status: 0,
            message: format!("{label} response missing numeric `{key}`"),
        })
}

// ---------------------------------------------------------------------------
// Idempotent comments — converge, don't accumulate.
// ---------------------------------------------------------------------------

/// One existing comment fetched from a forge (id + raw body), so adroit can find
/// and update the comment it authored on a re-run instead of posting a new one.
#[derive(Debug, Clone)]
pub struct ForgeComment {
    pub id: String,
    pub body: String,
}

/// Hidden marker tagging adroit's `review --forge` kickoff comment, so a re-run
/// edits that one comment in place rather than posting a duplicate. Invisible in
/// rendered markdown (GitHub/GitLab/Linear); see [`plan_upsert`].
pub const MARKER_REVIEW_KICKOFF: &str = "<!-- adroit:review-kickoff -->";
/// Hidden marker tagging adroit's `set-review --forge` deadline comment.
pub const MARKER_REVIEW_DEADLINE: &str = "<!-- adroit:review-deadline -->";

/// Parse a REST comments array into [`ForgeComment`]s. `id` may be numeric (GitHub/
/// GitLab) or a string (Jira); `body` is read from `body_key`. Anything malformed
/// is skipped, never panics.
pub(crate) fn parse_rest_comments(v: &Value, body_key: &str) -> Vec<ForgeComment> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c["id"]
                .as_u64()
                .map(|n| n.to_string())
                .or_else(|| c["id"].as_str().map(str::to_string))?;
            let body = c[body_key].as_str().unwrap_or_default().to_string();
            Some(ForgeComment { id, body })
        })
        .collect()
}

/// The action an idempotent comment upsert should take, decided purely from the
/// existing comments. The body is tagged with `marker` so a later run can find it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UpsertAction {
    /// A marked comment already holds exactly this body — do nothing (no API write).
    Noop,
    /// A marked comment exists but differs — edit it (carry its id).
    Update(String),
    /// No marked comment — create one.
    Create,
}

/// Append `marker` to `body` (what gets stored, so the next run recognizes it).
pub(crate) fn tag_body(marker: &str, body: &str) -> String {
    format!("{body}\n\n{marker}")
}

/// Decide the upsert action: find the first comment containing `marker`; if its
/// body already equals `tagged` it's a no-op, else update it; absent → create.
/// Pure — unit-tested without a network.
pub(crate) fn plan_upsert(existing: &[ForgeComment], marker: &str, tagged: &str) -> UpsertAction {
    match existing.iter().find(|c| c.body.contains(marker)) {
        Some(c) if c.body == tagged => UpsertAction::Noop,
        Some(c) => UpsertAction::Update(c.id.clone()),
        None => UpsertAction::Create,
    }
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
    // Thin dispatcher. The forge (PR/MR host) comes from `provider`; each module
    // owns its own construction. The tracker is normally the same system, but a
    // split setup (`tracker = jira`) swaps in a different tracker adapter —
    // that's how GitLab MRs + Jira issues are reached.
    let (forge, native_tracker) = match cfg.provider {
        Provider::None => (None, None),
        Provider::Github => github::open(cfg),
        Provider::Gitlab => gitlab::open(cfg),
    };
    let tracker = match cfg.tracker {
        TrackerProvider::Jira => jira::open(cfg),
        TrackerProvider::Linear => linear::open(cfg),
        TrackerProvider::Monday => monday::open(cfg),
        // `native` (and the explicit `gh_issues` / `gl_issues` aliases) file to
        // the forge's own issue tracker — there's no separate adapter to open.
        TrackerProvider::Native | TrackerProvider::GhIssues | TrackerProvider::GlIssues => {
            native_tracker
        }
    };
    (forge, tracker)
}

/// Warn that the forge integration is inactive, naming the **specific** pieces to
/// set so the message is actionable (vs. a bare "inactive"): the provider's token
/// env var + `forge.repo`, plus — when the verb needs the issue side — any split
/// tracker's own requirements. `action` is the verb-specific tail, e.g.
/// "skipping the review kickoff".
fn warn_inactive(fcfg: &ForgeConfig, needs_tracker: bool, action: &str) {
    let token = match fcfg.provider {
        Provider::Github => "ADROIT_GITHUB_TOKEN",
        Provider::Gitlab => "ADROIT_GITLAB_TOKEN",
        Provider::None => "a forge token",
    };
    let tracker = match (needs_tracker, fcfg.tracker) {
        (true, TrackerProvider::Jira) => {
            " + the Jira tracker (forge.tracker_host/tracker_project + ADROIT_JIRA_TOKEN)"
        }
        (true, TrackerProvider::Linear) => {
            " + the Linear tracker (forge.tracker_project = team key + ADROIT_LINEAR_TOKEN)"
        }
        (true, TrackerProvider::Monday) => {
            " + the monday tracker (forge.tracker_project = board id, forge.tracker_host = subdomain + ADROIT_MONDAY_TOKEN)"
        }
        _ => "",
    };
    eprintln!(
        "adroit: --forge: `{}` integration inactive — set forge.repo + {token}{tracker}; {action}.",
        fcfg.provider
    );
}

/// Whether the forge config plausibly applies to the ADR directory `dir`.
///
/// `forge.*` is a single (global) config, but the dashboard can switch ADR
/// directories at runtime and the CLI can be run anywhere — so the active
/// directory may belong to a *different* repo than `forge.repo`. We compare the
/// directory's `origin` remote against the configured repo: a definite mismatch
/// (the remote resolves to a different provider/slug) means the config doesn't
/// apply here, so forge data would be cross-wired and misleading. When we can't
/// tell — no `repo` configured, or `dir` has no recognizable remote — we assume
/// it applies and preserve prior behavior (don't block non-git ADR dirs).
fn dir_matches_forge(fcfg: &ForgeConfig, dir: &std::path::Path) -> bool {
    let Some(want) = fcfg.repo.as_deref() else {
        return true;
    };
    match crate::git::remote_url(dir, "origin").and_then(|u| crate::config::parse_remote_url(&u)) {
        Some((prov, repo, _host)) => prov == fcfg.provider && repo.eq_ignore_ascii_case(want),
        None => true,
    }
}

/// Whether a `--forge` operation should be skipped because the ADR directory
/// belongs to a *different* repo than `forge.repo` (warns with the reason when
/// so). `forge.*` is a single global config but ADR directories vary — the
/// dashboard switches them at runtime and the CLI runs anywhere — so this is the
/// guard that keeps forge reads *and writes* from cross-wiring another repo's
/// issues/PRs. Callers keep their local ADR record either way; an undeterminable
/// case (no `repo` configured, or no recognizable remote) is *not* skipped.
fn skip_dir_mismatch(fcfg: &ForgeConfig, dir: &std::path::Path) -> bool {
    if dir_matches_forge(fcfg, dir) {
        return false;
    }
    eprintln!(
        "adroit: forge.repo is `{}` but this directory's `origin` is a different repo — \
         skipping forge here (configure forge for this repo to enable it).",
        fcfg.repo.as_deref().unwrap_or("")
    );
    true
}

/// [`skip_dir_mismatch`] for a verb that operates on an ADR *file* — the repo is
/// resolved from the file's directory.
fn skip_path_mismatch(fcfg: &ForgeConfig, path: &std::path::Path) -> bool {
    skip_dir_mismatch(fcfg, path.parent().unwrap_or(std::path::Path::new(".")))
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
    if skip_path_mismatch(fcfg, path) {
        return Ok(());
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        warn_inactive(fcfg, true, "wrote the ADR locally only");
        return Ok(());
    };
    // Optional forge-artifact templates (`<templates_dir>/{issue,pr}.md`), so a
    // team can encode its conventions once. Placeholders: {{title}} {{file}} {{issue}}.
    let issue_tmpl = read_forge_template(cfg, "issue");
    let pr_tmpl = read_forge_template(cfg, "pr");
    run_new(
        forge.as_ref(),
        tracker.as_ref(),
        path,
        title,
        fcfg,
        dry_run,
        issue_tmpl.as_deref(),
        pr_tmpl.as_deref(),
    )
}

/// Read an optional forge-artifact template (`<templates_dir>/<name>.md`).
fn read_forge_template(cfg: &Config, name: &str) -> Option<String> {
    let dir = cfg.templates_dir.as_ref()?;
    std::fs::read_to_string(dir.join(format!("{name}.md"))).ok()
}

/// Substitute `{{title}}` / `{{file}}` / `{{issue}}` in a forge template.
fn render_forge_template(tmpl: &str, title: &str, file: &str, issue: &str) -> String {
    tmpl.replace("{{title}}", title)
        .replace("{{file}}", file)
        .replace("{{issue}}", issue)
}

/// The provider-agnostic orchestration (testable with mock/noop adapters and a
/// scratch git repo). Separated from [`after_new`] so tests don't need real
/// config/env to construct an adapter.
#[allow(clippy::too_many_arguments)]
fn run_new(
    forge: &dyn Forge,
    tracker: &dyn Tracker,
    path: &std::path::Path,
    title: &str,
    fcfg: &ForgeConfig,
    dry_run: bool,
    issue_tmpl: Option<&str>,
    pr_tmpl: Option<&str>,
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
    let issue_body = match issue_tmpl {
        Some(t) => render_forge_template(t, title, file, ""),
        None => {
            format!("Tracking issue for ADR \u{201c}{title}\u{201d} (`{file}`), managed by adroit.")
        }
    };
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
    let pr_body = match pr_tmpl {
        Some(t) => render_forge_template(t, title, file, &issue.id),
        None => format!("ADR: {title}\n\nCloses #{}.", issue.id),
    };
    let pr = match forge.open_pr(&PrDraft {
        branch: branch.clone(),
        base: fcfg.base_branch.clone(),
        title: format!("ADR: {title}"),
        body: pr_body,
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
    quorum: Option<u32>,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(true);
    };
    if skip_path_mismatch(fcfg, path) {
        return Ok(true);
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        warn_inactive(fcfg, true, "doing the local status change only");
        return Ok(true);
    };
    let proceed = run_status_change(
        forge.as_ref(),
        tracker.as_ref(),
        &read_refs(path),
        new_status,
        quorum.unwrap_or(cfg.review_quorum),
        dry_run,
        yes,
    )?;
    // On an *applied* `accepted` the proposal PR just merged, landing the ADR in
    // `proposed/` on the base branch. Fast-forward the local base so the upcoming
    // move + relink can be committed onto it — #4's "push the relink commit"
    // (with [`after_status_change`] doing the commit/push). Best-effort.
    if proceed && yes && !dry_run && new_status == Status::Accepted {
        sync_base_for_heal(fcfg, path);
    }
    Ok(proceed)
}

/// Fast-forward the base branch to the just-merged remote tip so the heal commit
/// (`proposed/ → accepted/` + relink) can land on it. Best-effort: it skips on a
/// dirty tree (don't clobber edits) or no/​diverged remote, warning so the user
/// knows to commit the local move themselves. Never fails the status change.
fn sync_base_for_heal(fcfg: &ForgeConfig, path: &std::path::Path) {
    let start = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let base = &fcfg.base_branch;
    // Resolve the repo top-level *before* switching — a status subdir like
    // `proposed/` can disappear on the target branch, breaking later `-C` ops.
    let Ok(top) = crate::git::toplevel(start) else {
        return; // not a work tree → nothing to heal; the move stays local
    };
    if !matches!(crate::git::is_clean(&top), Ok(true)) {
        eprintln!(
            "adroit: merged the PR, but the working tree isn't clean — left the \
             accepted/ move local (commit + push it to {base} yourself)."
        );
        return;
    }
    let orig = crate::git::current_branch(&top).ok();
    let synced = crate::git::fetch(&top, "origin")
        .and_then(|()| crate::git::switch(&top, base))
        .and_then(|()| crate::git::merge_ff_only(&top, "origin", base));
    if let Err(e) = synced {
        // Restore the original branch so the local move still applies there.
        if let Some(orig) = &orig {
            let _ = crate::git::switch(&top, orig);
        }
        eprintln!(
            "adroit: merged the PR, but couldn't fast-forward {base} ({e}) — left \
             the accepted/ move local."
        );
    }
}

/// Forge actions for `set-status`, run **after** the local move. On an applied
/// `accepted` where [`before_status_change`] put us on the (fast-forwarded) base
/// branch, this commits the move + relink and pushes it — #4's "push the relink
/// commit", so `accepted/` lands on `main` in one command. No-op otherwise;
/// best-effort (a rejected push leaves the move committed locally).
pub fn after_status_change(
    cfg: &Config,
    new_path: &std::path::Path,
    new_status: Status,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<()> {
    if !(yes && !dry_run && new_status == Status::Accepted) {
        return Ok(());
    }
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    // The ADR dir root (e.g. `<repo>/adrs`): the moved file is `<root>/<status>/<file>`.
    let status_dir = new_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let adr_root = status_dir.parent().unwrap_or(status_dir);
    let base = &fcfg.base_branch;
    let Ok(top) = crate::git::toplevel(adr_root) else {
        return Ok(());
    };
    // Only heal when before_status_change left us on the synced base branch;
    // otherwise the move is on the proposal branch — leave it local.
    if crate::git::current_branch(&top).ok().as_deref() != Some(base.as_str()) {
        return Ok(());
    }
    let label = new_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("adr");
    let heal = (|| -> Result<(), crate::git::GitError> {
        crate::git::add(&top, adr_root)?; // stages the move + relinks (ADR dir only)
        crate::git::commit(&top, &format!("ADR: accept {label}"))?;
        crate::git::push(&top, "origin", base)
    })();
    match heal {
        Ok(()) => println!("Forge: pushed the accepted/ relink commit to {base}"),
        Err(e) => eprintln!(
            "adroit: committed the accepted/ move locally but couldn't push {base} \
             ({e}) — push it yourself."
        ),
    }
    Ok(())
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
            // Read live PR state to verify quorum + CI. In *preview* mode this is
            // best-effort: a preview must never require valid credentials, so an
            // auth/API failure still prints a (credential-free) plan. Only an
            // actual apply surfaces auth/API errors; offline always degrades to a
            // local-only change.
            let state = match forge.pr_state(&pr) {
                Ok(s) => Some(s),
                Err(e) if e.is_offline() => {
                    if apply {
                        eprintln!("adroit: forge unreachable ({e}); local status change only.");
                        return Ok(true);
                    }
                    None
                }
                Err(e) => {
                    if apply {
                        return Err(e.into());
                    }
                    eprintln!(
                        "adroit: couldn't read live PR state for the preview ({e}); \
                         showing the plan without approval/CI status."
                    );
                    None
                }
            };
            if !apply {
                println!("Forge plan (accept):");
                match &state {
                    Some(st) => {
                        let ok = st.approvals >= quorum
                            && matches!(st.ci, CiStatus::Success | CiStatus::None);
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
                    }
                    None => {
                        println!(
                            "  - verify PR {pr_url} has >= {quorum} approvals + passing CI, then merge"
                        );
                        if let Some((_, iu)) = &refs.issue {
                            println!("  - close issue {iu}");
                        }
                        println!(
                            "  (live PR state unavailable — set a valid token to see approvals/CI)"
                        );
                    }
                }
                println!("\nPreview — re-run with --yes to apply.");
                return Ok(false);
            }
            // apply == true: offline/auth already returned above, so state is Some;
            // fall back to a local-only change if it somehow isn't.
            let Some(st) = state else {
                return Ok(true);
            };
            let ok = st.approvals >= quorum && matches!(st.ci, CiStatus::Success | CiStatus::None);
            if !st.merged {
                if !ok {
                    anyhow::bail!(
                        "refusing to accept: PR {pr_url} has {}/{quorum} approvals, CI {:?}",
                        st.approvals,
                        st.ci
                    );
                }
                // A draft PR can't be merged — ensure it's ready first (no-op if
                // `review` already un-drafted it). Best-effort.
                if let Err(e) = forge.mark_ready(&pr) {
                    eprintln!("adroit: couldn't mark PR ready before merge ({e})");
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
    if skip_path_mismatch(fcfg, old_path) {
        return Ok(true);
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        warn_inactive(fcfg, true, "doing the local supersede only");
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

/// Comment `body` on whichever of the ADR's linked PR + issue exist (best-effort,
/// warn-and-continue). Shared by the `review`/`set-review` forge paths.
/// Post `body` on the linked PR **and** issue **idempotently**, tagged with
/// `marker` so a re-run edits adroit's own comment in place instead of adding a
/// duplicate (converge, don't accumulate). Graceful-offline: a failure on one
/// side warns and the other still posts.
fn upsert_both(
    forge: &dyn Forge,
    tracker: &dyn Tracker,
    refs: &ForgeRefs,
    marker: &str,
    body: &str,
) {
    if let Some((pr, _)) = &refs.pr
        && let Err(e) = forge.upsert_pr_comment(pr, marker, body)
    {
        eprintln!("adroit: couldn't comment on PR ({e})");
    }
    if let Some((issue, _)) = &refs.issue
        && let Err(e) = tracker.upsert_issue_comment(issue, marker, body)
    {
        eprintln!("adroit: couldn't comment on issue ({e})");
    }
}

/// A ` @handle` mention suffix for each configured reviewer (a missing `@` is
/// added); empty when no reviewers are configured.
fn mention_suffix(reviewers: &[String]) -> String {
    reviewers
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .map(|r| {
            if r.starts_with('@') {
                format!(" {r}")
            } else {
                format!(" @{r}")
            }
        })
        .collect()
}

/// `review --forge`: post the kickoff `body` on the linked issue **and** PR with
/// the reviewer pool @-mentioned, and tag the PR with a `review-by:<deadline>`
/// label. Opt-in, dry-run/--yes gated, graceful-offline.
pub fn review_kickoff(
    cfg: &Config,
    path: &std::path::Path,
    body: &str,
    deadline: &str,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    if skip_path_mismatch(fcfg, path) {
        return Ok(());
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        warn_inactive(fcfg, true, "skipping the review kickoff");
        return Ok(());
    };
    let refs = read_refs(path);
    if refs.issue.is_none() && refs.pr.is_none() {
        eprintln!(
            "adroit: --forge: this ADR has no linked issue/PR in `## References` \
             (create one with `new --forge`); nothing to post."
        );
        return Ok(());
    }
    let label = format!("review-by:{deadline}");
    let mentions = mention_suffix(&fcfg.reviewers);
    let apply = yes && !dry_run;
    if !apply {
        println!("Forge plan (review kickoff):");
        if let Some((_, pu)) = &refs.pr {
            println!("  - mark PR ready for review (un-draft)");
            println!("  - comment on PR {pu}");
            println!("  - label PR `{label}`");
        }
        if let Some((_, iu)) = &refs.issue {
            println!("  - comment on issue {iu}");
        }
        if !mentions.is_empty() {
            println!("  - @-mention reviewers:{mentions}");
        }
        println!("\nPreview — re-run with --yes to apply.");
        return Ok(());
    }
    let full = if mentions.is_empty() {
        body.to_string()
    } else {
        format!("{body}\n\ncc:{mentions}")
    };
    upsert_both(
        forge.as_ref(),
        tracker.as_ref(),
        &refs,
        MARKER_REVIEW_KICKOFF,
        &full,
    );
    if let Some((pr, _)) = &refs.pr {
        // Opening for formal review → un-draft the PR so it can be reviewed + merged.
        if let Err(e) = forge.mark_ready(pr) {
            eprintln!("adroit: couldn't mark PR ready for review ({e})");
        }
        if let Err(e) = forge.add_label(pr, &label) {
            eprintln!("adroit: couldn't label PR ({e})");
        }
    }
    Ok(())
}

/// `set-review --forge`: mirror the deadline to the linked issue/PR — post `note`
/// as a comment **and** set the tracker's native due/target date (`date`, `None`
/// clears). Opt-in, dry-run/--yes gated, graceful-offline.
pub fn set_review_deadline(
    cfg: &Config,
    path: &std::path::Path,
    note: &str,
    date: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    if skip_path_mismatch(fcfg, path) {
        return Ok(());
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        warn_inactive(fcfg, true, "skipping the review deadline");
        return Ok(());
    };
    let refs = read_refs(path);
    if refs.issue.is_none() && refs.pr.is_none() {
        eprintln!(
            "adroit: --forge: this ADR has no linked issue/PR in `## References` \
             (create one with `new --forge`); nothing to post."
        );
        return Ok(());
    }
    let apply = yes && !dry_run;
    if !apply {
        println!("Forge plan (review deadline):");
        if let Some((_, pu)) = &refs.pr {
            println!("  - comment on PR {pu}");
        }
        if let Some((id, iu)) = &refs.issue {
            println!("  - comment on issue {iu}");
            match date {
                Some(d) => println!("  - set issue {id} due date → {d}"),
                None => println!("  - clear issue {id} due date"),
            }
        }
        println!("\nPreview — re-run with --yes to apply.");
        return Ok(());
    }
    upsert_both(
        forge.as_ref(),
        tracker.as_ref(),
        &refs,
        MARKER_REVIEW_DEADLINE,
        note,
    );
    if let Some((issue, _)) = &refs.issue
        && let Err(e) = tracker.set_due_date(issue, date)
    {
        eprintln!("adroit: couldn't set issue due date ({e})");
    }
    Ok(())
}

/// Refresh an ADR's linked PR **description** to the current ADR content (an
/// adroit-managed, marked body). Backs `adroit sync` (MR-description sync) and
/// `relink --forge` (re-point the PR after a status move). Opt-in,
/// dry-run/--yes gated, graceful-offline. Returns `Ok(true)` to proceed.
pub fn sync_pr(
    cfg: &Config,
    path: &std::path::Path,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(true);
    };
    if skip_path_mismatch(fcfg, path) {
        return Ok(true);
    }
    let (forge, _tracker) = open(fcfg);
    let Some(forge) = forge else {
        warn_inactive(fcfg, false, "skipping PR sync");
        return Ok(true);
    };
    let refs = read_refs(path);
    let Some((pr, pr_url)) = refs.pr else {
        return Ok(true); // no PR linked → nothing to sync
    };
    let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("adr");
    let apply = yes && !dry_run;
    if !apply {
        println!("Forge plan (sync): refresh PR {pr_url} description from `{file}`");
        println!("\nPreview — re-run with --yes to apply.");
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)?;
    let closes = refs
        .issue
        .as_ref()
        .map(|(id, _)| format!("Closes #{id}\n\n"))
        .unwrap_or_default();
    let body = format!("{closes}<!-- adroit:adr={file} -->\n{content}");
    match forge.set_pr_body(&pr, &body) {
        Ok(()) => {
            println!("Forge: synced PR {pr_url}");
            Ok(true)
        }
        Err(e) if e.is_offline() => {
            eprintln!("adroit: forge unreachable ({e}); PR left unchanged.");
            Ok(true)
        }
        Err(e) => Err(e.into()),
    }
}

/// POST `text` to a Slack/Teams-compatible incoming webhook (the `{ "text": … }`
/// shape both accept). Best-effort: a non-2xx or offline webhook warns and
/// returns `Ok` (a notification failure shouldn't fail the command).
/// POST the message to `webhook`. Returns `true` only on a 2xx — a non-2xx or
/// an unreachable host warns and returns `false` (graceful, but the caller must
/// not then claim it was delivered).
pub fn notify(webhook: &str, text: &str) -> anyhow::Result<bool> {
    let body = serde_json::to_vec(&serde_json::json!({ "text": text })).expect("serialize");
    match UreqTransport.request(
        "POST",
        webhook,
        &[("Content-Type", "application/json")],
        Some(&body),
    ) {
        Ok(resp) if (200..300).contains(&resp.status) => Ok(true),
        Ok(resp) => {
            eprintln!("adroit: notify webhook returned HTTP {}", resp.status);
            Ok(false)
        }
        Err(e) => {
            eprintln!("adroit: notify webhook unreachable ({e})");
            Ok(false)
        }
    }
}

/// Forge-aware repo checks (issue #4 lifecycle map): flag drift between an ADR
/// and its linked issue/PR. Network reads degrade gracefully (warn-once offline,
/// skip). Returns `Warning`-severity problems so `check` reports but doesn't fail.
pub fn check_repo(
    cfg: &Config,
    entries: &[(std::path::PathBuf, crate::adr::Adr)],
) -> anyhow::Result<Vec<crate::view::Problem>> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(vec![]);
    };
    let dir = entries
        .first()
        .and_then(|(p, _)| p.parent())
        .unwrap_or(std::path::Path::new("."));
    if skip_dir_mismatch(fcfg, dir) {
        return Ok(vec![]);
    }
    let (forge, tracker) = open(fcfg);
    let (Some(forge), Some(tracker)) = (forge, tracker) else {
        return Ok(vec![]);
    };
    let mut problems = Vec::new();
    let mut warned = false;
    for (path, adr) in entries {
        let refs = read_refs(path);
        let rel = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if let Some((pr, url)) = &refs.pr {
            match forge.pr_state(pr) {
                Ok(st) => {
                    if adr.status == Status::Accepted && !st.merged {
                        problems.push(forge_problem(
                            &rel,
                            "accepted ADR but its PR is not merged",
                            url,
                        ));
                    } else if adr.status == Status::Proposed && st.merged {
                        problems.push(forge_problem(
                            &rel,
                            "PR is merged but the ADR is still proposed",
                            url,
                        ));
                    }
                }
                Err(e) if e.is_offline() => {
                    if !warned {
                        eprintln!("adroit: forge unreachable ({e}); skipping forge checks");
                        warned = true;
                    }
                }
                Err(_) => {}
            }
        }
        if let Some((issue, url)) = &refs.issue
            && let Ok(st) = tracker.issue_state(issue)
            && !st.open
            && adr.status == Status::Proposed
        {
            problems.push(forge_problem(
                &rel,
                "tracker issue is closed but the ADR is still proposed",
                url,
            ));
        }
    }
    Ok(problems)
}

fn forge_problem(label: &str, summary: &str, url: &str) -> crate::view::Problem {
    crate::view::Problem {
        severity: crate::view::Severity::Warning,
        kind: crate::view::ProblemKind::ForgeIntegration,
        label: label.to_string(),
        summary: summary.to_string(),
        paths: Vec::new(),
        message: format!("{label}: {summary} ({url})"),
    }
}

/// Attach live forge state (issue/PR URLs + PR approvals/CI/merged) to each
/// summary for `list --forge` / the dashboard. Opt-in; warn-once offline.
pub fn enrich(
    cfg: &Config,
    store: &crate::store::Store,
    summaries: &mut [crate::view::AdrSummary],
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    enrich_with(fcfg, store, summaries)
}

/// As [`enrich`], but driven by a [`ForgeConfig`] directly — the dashboard's
/// read-only forge panel uses this to enrich one ADR on demand without holding
/// a whole [`Config`].
pub fn enrich_with(
    fcfg: &crate::config::ForgeConfig,
    store: &crate::store::Store,
    summaries: &mut [crate::view::AdrSummary],
) -> anyhow::Result<()> {
    if skip_dir_mismatch(fcfg, store.root()) {
        return Ok(());
    }
    let (forge, _tracker) = open(fcfg);
    let Some(forge) = forge else {
        return Ok(());
    };
    let naming = store.options().naming;
    let mut warned = false;
    for s in summaries.iter_mut() {
        let Some(r) = naming.parse_ref(&s.address) else {
            continue;
        };
        let Ok(path) = store.find_path_by_ref(&r) else {
            continue;
        };
        let refs = read_refs(&path);
        if refs.issue.is_none() && refs.pr.is_none() {
            continue;
        }
        let mut data = crate::view::ForgeData {
            issue_url: refs.issue.as_ref().map(|(_, u)| u.clone()),
            pr_url: refs.pr.as_ref().map(|(_, u)| u.clone()),
            pr_approvals: None,
            pr_ci: None,
            pr_merged: None,
        };
        if let Some((pr, _)) = &refs.pr {
            match forge.pr_state(pr) {
                Ok(st) => {
                    data.pr_approvals = Some(st.approvals);
                    data.pr_ci = Some(format!("{:?}", st.ci).to_lowercase());
                    data.pr_merged = Some(st.merged);
                }
                Err(e) if e.is_offline() => {
                    if !warned {
                        eprintln!("adroit: forge unreachable ({e}); showing links only");
                        warned = true;
                    }
                }
                Err(_) => {}
            }
        }
        s.forge_data = Some(data);
    }
    Ok(())
}

/// Reconcile local ADR status with the forge after an out-of-band change (an MR
/// merged or a tracker issue closed *outside* adroit). Reports drift; with
/// `apply`, fixes the unambiguous case — a **merged PR whose ADR isn't accepted**
/// — by moving it to `accepted/` locally (+ relink). A closed issue on a
/// still-proposed ADR is reported only (accepted vs won't-fix is ambiguous).
/// **Read-only on the forge** — it never merges/closes anything; it only syncs
/// the local record to forge reality.
pub fn reconcile(
    cfg: &Config,
    store: &crate::store::Store,
    summaries: &[crate::view::AdrSummary],
    apply: bool,
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        eprintln!("adroit: reconcile needs a configured forge (set forge.provider + forge.repo).");
        return Ok(());
    };
    if skip_dir_mismatch(fcfg, store.root()) {
        return Ok(());
    }
    let (forge, tracker) = open(fcfg);
    let Some(forge) = forge else {
        warn_inactive(fcfg, false, "skipping reconcile");
        return Ok(());
    };
    run_reconcile(forge.as_ref(), tracker.as_deref(), store, summaries, apply)
}

/// Provider-agnostic reconcile core (testable with mock adapters + a scratch store).
fn run_reconcile(
    forge: &dyn Forge,
    tracker: Option<&dyn Tracker>,
    store: &crate::store::Store,
    summaries: &[crate::view::AdrSummary],
    apply: bool,
) -> anyhow::Result<()> {
    let naming = store.options().naming;
    let mut warned = false;
    let mut drift = 0u32;
    let mut fixed = 0u32;
    for s in summaries {
        let Some(r) = naming.parse_ref(&s.address) else {
            continue;
        };
        let Ok(path) = store.find_path_by_ref(&r) else {
            continue;
        };
        let refs = read_refs(&path);
        // 1. A merged PR whose ADR isn't accepted — the clear out-of-band case.
        if let Some((pr, pr_url)) = &refs.pr {
            match forge.pr_state(pr) {
                Ok(st) if st.merged && s.status != Status::Accepted => {
                    drift += 1;
                    println!(
                        "{}: PR {pr_url} is merged but status is {} -> accepted",
                        s.reference, s.status
                    );
                    if apply {
                        store.set_status_ref(&r, Status::Accepted)?;
                        fixed += 1;
                    }
                    continue; // resolved by the PR; don't double-report on the issue
                }
                Ok(_) => {}
                Err(e) if e.is_offline() => {
                    if !warned {
                        eprintln!("adroit: forge unreachable ({e}); skipping live checks.");
                        warned = true;
                    }
                }
                Err(_) => {}
            }
        }
        // 2. A closed issue on a still-proposed ADR — report only (direction is
        //    ambiguous: a closed issue could mean accepted *or* won't-fix).
        if s.status == Status::Proposed
            && let Some((issue, issue_url)) = &refs.issue
            && let Some(tracker) = tracker
            && let Ok(state) = tracker.issue_state(issue)
            && !state.open
        {
            drift += 1;
            println!(
                "{}: issue {issue_url} is closed but status is still proposed (resolve: set-status accepted|rejected)",
                s.reference
            );
        }
    }
    match (drift, apply) {
        (0, _) => println!("Reconcile: no drift — local statuses match the forge."),
        (_, true) => println!("\nReconcile: {fixed} fixed, {} reported.", drift - fixed),
        (_, false) => println!(
            "\nReconcile: {drift} drift item(s). Re-run with --yes to apply the fixable ones."
        ),
    }
    Ok(())
}

/// Dashboard aggregate counts: `(proposed_without_pr, approved_unmerged)`.
/// `None` when the forge is inactive (no token) so the caller hides the tiles.
/// `proposed_without_pr` (a Proposed ADR with no PR in `## References`) is local;
/// `approved_unmerged` (a PR with >= `quorum` approvals that isn't merged) reads
/// live PR state — offline ADRs are skipped.
pub fn dashboard_summary(
    fcfg: &ForgeConfig,
    store: &crate::store::Store,
    summaries: &[crate::view::AdrSummary],
    quorum: u32,
) -> anyhow::Result<Option<(u32, u32)>> {
    if skip_dir_mismatch(fcfg, store.root()) {
        return Ok(None);
    }
    let (forge, _tracker) = open(fcfg);
    let Some(forge) = forge else {
        return Ok(None);
    };
    let naming = store.options().naming;
    let mut proposed_without_pr = 0u32;
    let mut approved_unmerged = 0u32;
    let mut warned = false;
    for s in summaries {
        let Some(r) = naming.parse_ref(&s.address) else {
            continue;
        };
        let Ok(path) = store.find_path_by_ref(&r) else {
            continue;
        };
        let refs = read_refs(&path);
        if s.status == Status::Proposed && refs.pr.is_none() {
            proposed_without_pr += 1;
        }
        if let Some((pr, _)) = &refs.pr {
            match forge.pr_state(pr) {
                Ok(st) if !st.merged && st.approvals >= quorum => approved_unmerged += 1,
                Ok(_) => {}
                Err(e) if e.is_offline() => {
                    if !warned {
                        eprintln!(
                            "adroit: forge unreachable ({e}); approval counts may be partial."
                        );
                        warned = true;
                    }
                }
                Err(_) => {}
            }
        }
    }
    Ok(Some((proposed_without_pr, approved_unmerged)))
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
    fn mention_suffix_at_prefixes_each_handle_once() {
        let r = vec!["@alice".to_string(), "bob".to_string(), "  ".to_string()];
        assert_eq!(mention_suffix(&r), " @alice @bob");
        assert_eq!(mention_suffix(&[]), "");
    }

    fn comment(id: &str, body: &str) -> ForgeComment {
        ForgeComment {
            id: id.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn plan_upsert_creates_when_no_marked_comment_exists() {
        let tagged = tag_body(MARKER_REVIEW_KICKOFF, "kickoff");
        // Empty, and an unrelated comment, both → create.
        assert_eq!(
            plan_upsert(&[], MARKER_REVIEW_KICKOFF, &tagged),
            UpsertAction::Create
        );
        let others = [comment("1", "someone else's note")];
        assert_eq!(
            plan_upsert(&others, MARKER_REVIEW_KICKOFF, &tagged),
            UpsertAction::Create
        );
    }

    #[test]
    fn plan_upsert_is_a_noop_when_the_marked_comment_is_unchanged() {
        let tagged = tag_body(MARKER_REVIEW_KICKOFF, "kickoff");
        let existing = [comment("1", "noise"), comment("7", &tagged)];
        // Same body already posted → converged, no API write.
        assert_eq!(
            plan_upsert(&existing, MARKER_REVIEW_KICKOFF, &tagged),
            UpsertAction::Noop
        );
    }

    #[test]
    fn plan_upsert_updates_the_marked_comment_when_the_body_changed() {
        let old = tag_body(MARKER_REVIEW_KICKOFF, "old deadline 2026-06-10");
        let new = tag_body(MARKER_REVIEW_KICKOFF, "new deadline 2026-06-20");
        let existing = [comment("42", &old)];
        // Found by marker, body differs → edit comment 42 (don't post a duplicate).
        assert_eq!(
            plan_upsert(&existing, MARKER_REVIEW_KICKOFF, &new),
            UpsertAction::Update("42".to_string())
        );
    }

    #[test]
    fn parse_rest_comments_tolerates_numeric_and_string_ids() {
        let v = serde_json::json!([
            { "id": 12, "body": "a" },
            { "id": "c-uuid", "body": "b" },
            { "id": null, "body": "skipped" },
            "garbage",
        ]);
        let parsed = parse_rest_comments(&v, "body");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "12");
        assert_eq!(parsed[1].id, "c-uuid");
        // A non-array (hostile shape) is an empty list, never a panic.
        assert!(parse_rest_comments(&serde_json::json!({}), "body").is_empty());
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
    fn dir_matches_forge_compares_origin_to_configured_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q"]);
        git(
            dir,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widgets.git",
            ],
        );

        let gh = |repo: Option<&str>| ForgeConfig {
            provider: Provider::Github,
            repo: repo.map(str::to_string),
            ..Default::default()
        };

        // Same repo (slug is case-insensitive) → the config applies here.
        assert!(dir_matches_forge(&gh(Some("acme/widgets")), dir));
        assert!(dir_matches_forge(&gh(Some("acme/WIDGETS")), dir));
        // A different repo, or a different provider → does not apply (this is the
        // "switched to another repo's ADR dir" case that must hide forge data).
        assert!(!dir_matches_forge(&gh(Some("acme/other")), dir));
        assert!(!dir_matches_forge(
            &ForgeConfig {
                provider: Provider::Gitlab,
                repo: Some("acme/widgets".into()),
                ..Default::default()
            },
            dir
        ));
        // Can't tell — no repo configured, or no recognizable remote → assume it
        // applies (don't block non-git or unrecognized-host ADR dirs).
        assert!(dir_matches_forge(&gh(None), dir));
        let bare = tempfile::tempdir().unwrap();
        assert!(dir_matches_forge(&gh(Some("acme/widgets")), bare.path()));

        // The file-path wrapper the mutating verbs use resolves the repo from the
        // file's directory: skip == true means "don't touch this repo's forge".
        let file = dir.join("0001-x.md");
        assert!(!skip_path_mismatch(&gh(Some("acme/widgets")), &file));
        assert!(skip_path_mismatch(&gh(Some("acme/other")), &file));
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
            None,
            None,
        )
        .unwrap();

        let body = std::fs::read_to_string(&adr).unwrap();
        assert!(body.contains("## References"), "got:\n{body}");
        assert!(body.contains("- Issue: (dry-run)"), "got:\n{body}");
    }

    #[test]
    fn forge_template_substitutes_placeholders() {
        let out = render_forge_template(
            "Issue for {{title}} ({{file}}) — see #{{issue}}",
            "Adopt PG",
            "0007-pg.md",
            "42",
        );
        assert_eq!(out, "Issue for Adopt PG (0007-pg.md) — see #42");
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
            None,
            None,
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
        fn set_pr_body(&self, _: &str, _: &str) -> Result<(), ForgeError> {
            Ok(())
        }
        fn describe(&self) -> String {
            "mock".into()
        }
    }

    /// A forge whose PR read fails with an *auth* error — used to prove a
    /// preview is credential-free, and that an apply still surfaces the error.
    struct AuthFailForge;
    impl Forge for AuthFailForge {
        fn open_pr(&self, _: &PrDraft) -> Result<PrRef, ForgeError> {
            unreachable!()
        }
        fn pr_state(&self, _: &str) -> Result<PrState, ForgeError> {
            Err(ForgeError::Auth("Bad credentials".into()))
        }
        fn merge_pr(&self, _: &str) -> Result<(), ForgeError> {
            unreachable!("apply must abort before merging on an auth failure")
        }
        fn close_pr(&self, _: &str) -> Result<(), ForgeError> {
            unreachable!()
        }
        fn comment_pr(&self, _: &str, _: &str) -> Result<(), ForgeError> {
            unreachable!()
        }
        fn set_pr_body(&self, _: &str, _: &str) -> Result<(), ForgeError> {
            unreachable!()
        }
        fn describe(&self) -> String {
            "authfail".into()
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

    #[test]
    fn run_reconcile_accepts_a_merged_pr_locally() {
        use crate::config::Layout;
        use crate::format::Format;
        use crate::store::{Store, StoreOptions};
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // A proposed ADR (flat layout) carrying a PR reference.
        std::fs::write(
            dir.join("0001-x.md"),
            "# ADR-0001: X\n\n## Status\n\nProposed\n\n## References\n\n- Pull Request: https://example.com/pull/1\n",
        )
        .unwrap();
        let store = Store::open_or_create_with(
            dir,
            StoreOptions {
                layout: Layout::Flat,
                format: Format::Markdown,
                ..Default::default()
            },
        )
        .unwrap();
        let summaries = crate::query::summaries(&store, &crate::query::Filter::default()).unwrap();
        assert_eq!(summaries[0].status, Status::Proposed);

        // The PR reads as merged → reconcile moves the ADR to accepted locally.
        let forge = MockForge {
            state: PrState {
                approvals: 0,
                ci: CiStatus::None,
                merged: true,
                draft: false,
            },
            merged: false.into(),
        };
        run_reconcile(&forge, None, &store, &summaries, true).unwrap();

        let after = crate::query::summaries(&store, &crate::query::Filter::default()).unwrap();
        assert_eq!(after[0].status, Status::Accepted);
    }

    #[test]
    fn accept_dry_run_preview_is_credential_free() {
        // `set-status accepted --forge --dry-run` (dry_run=true, no --yes):
        // a bad/missing token must NOT abort the preview — it prints the plan
        // without live approval/CI status and stops before any local move.
        let proceed = run_status_change(
            &AuthFailForge,
            &noop::NoopTracker,
            &refs_with_pr(),
            Status::Accepted,
            3,
            true,  // --dry-run
            false, // no --yes
        )
        .unwrap();
        assert!(!proceed); // preview only: no local move
    }

    #[test]
    fn accept_apply_surfaces_auth_error() {
        // With --yes, an auth failure reading the PR is fatal (you can't merge an
        // approval-gated PR without credentials) — and must not reach merge_pr.
        let err = run_status_change(
            &AuthFailForge,
            &noop::NoopTracker,
            &refs_with_pr(),
            Status::Accepted,
            3,
            false,
            true, // --yes → apply
        )
        .unwrap_err();
        assert!(err.to_string().contains("auth"));
    }
}
