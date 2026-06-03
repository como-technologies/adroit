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

use crate::config::{Config, ForgeConfig, Provider};

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
    match cfg.provider {
        Provider::None => (None, None),
        // TODO(phase 1 / 1b): construct the GitHub / GitLab adapter here from
        // `cfg` + the env token; until then, enabling a provider is a no-op.
        Provider::Github | Provider::Gitlab => (None, None),
    }
}

/// Orchestrate the forge side of `adroit new` (issue + draft PR + `## References`).
///
/// **Phase 0 stub:** the adapters aren't wired yet ([`open`] returns `(None, None)`),
/// so this reports that an enabled provider is a no-op and leaves the local ADR
/// untouched. Phase 1 replaces this body with the real GitHub orchestration.
pub fn after_new(
    cfg: &Config,
    _path: &std::path::Path,
    _dry_run: bool,
    _yes: bool,
) -> anyhow::Result<()> {
    let Some(fcfg) = cfg.forge.as_ref() else {
        return Ok(());
    };
    let (forge, tracker) = open(fcfg);
    if forge.is_none() || tracker.is_none() {
        eprintln!(
            "adroit: forge provider `{}` isn't wired up yet — wrote the ADR locally only",
            fcfg.provider
        );
    }
    Ok(())
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
}
