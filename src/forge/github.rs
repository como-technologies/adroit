//! GitHub adapter — implements both [`Forge`] (Pull Requests) and [`Tracker`]
//! (GitHub Issues) over one client + one token (`ADROIT_GITHUB_TOKEN`). All
//! HTTP goes through the injectable [`HttpTransport`] seam, so the unit tests
//! drive it with recorded fixtures and never touch the network.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    CiStatus, Forge, ForgeError, HttpTransport, IssueRef, IssueState, PrDraft, PrRef, PrState,
    Tracker, Transition, UreqTransport,
};

/// A GitHub REST client scoped to one `owner/repo`.
#[derive(Clone)]
pub struct Github {
    /// API host (`api.github.com`, or `<host>/api/v3` for GitHub Enterprise).
    host: String,
    /// `owner/repo`.
    repo: String,
    token: String,
    transport: Arc<dyn HttpTransport>,
}

impl Github {
    /// Build a client using the production `ureq` transport.
    pub fn new(host: Option<String>, repo: String, token: String) -> Self {
        Self {
            host: host.unwrap_or_else(|| "api.github.com".to_string()),
            repo,
            token,
            transport: Arc::new(UreqTransport),
        }
    }

    /// Build a client over an injected transport (tests).
    pub fn with_transport(repo: &str, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            host: "api.github.com".to_string(),
            repo: repo.to_string(),
            token: "test-token".to_string(),
            transport,
        }
    }

    /// Issue one REST call, mapping status → [`ForgeError`] and parsing the JSON
    /// body of a 2xx (empty body → `Value::Null`).
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let url = format!("https://{}/{}", self.host, path.trim_start_matches('/'));
        let auth = format!("Bearer {}", self.token);
        let headers = [
            ("Authorization", auth.as_str()),
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "adroit"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ];
        super::rest_call(
            self.transport.as_ref(),
            method,
            &url,
            &headers,
            body,
            "GitHub",
            message_of,
        )
    }
}

/// Construct the GitHub `(forge, tracker)` adapters from config, or
/// `(None, None)` when inactive — no `ADROIT_GITHUB_TOKEN` (or `forge.token`)
/// or no `forge.repo`. GitHub owns its own token env var and slug requirements
/// here, so the central [`super::open`] factory stays a thin dispatcher.
pub fn open(cfg: &crate::config::ForgeConfig) -> super::Adapters {
    let token = cfg
        .token
        .clone()
        .or_else(|| std::env::var("ADROIT_GITHUB_TOKEN").ok())
        .or_else(|| crate::config::load_credential("github"));
    match (token, cfg.repo.clone()) {
        (Some(token), Some(repo)) => {
            let gh = Github::new(cfg.host.clone(), repo, token);
            (Some(Box::new(gh.clone())), Some(Box::new(gh)))
        }
        _ => (None, None),
    }
}

/// Roll up the Checks API `check_runs` array into one [`CiStatus`]. No runs ⇒
/// `None` (CI isn't configured — don't block an accept on a phantom "pending");
/// a definitive failing conclusion ⇒ `Failure`; any run not yet completed ⇒
/// `Pending`; otherwise `Success` (all completed success/neutral/skipped).
fn classify_check_runs(checks: &Value) -> CiStatus {
    let runs = match checks["check_runs"].as_array() {
        Some(r) if !r.is_empty() => r,
        _ => return CiStatus::None,
    };
    let mut any_pending = false;
    for r in runs {
        if r["status"].as_str() != Some("completed") {
            any_pending = true; // queued / in_progress
            continue;
        }
        match r["conclusion"].as_str() {
            Some("success" | "neutral" | "skipped") => {}
            // failure / cancelled / timed_out / action_required / startup_failure
            Some(_) => return CiStatus::Failure,
            None => any_pending = true,
        }
    }
    if any_pending {
        CiStatus::Pending
    } else {
        CiStatus::Success
    }
}

/// Pull GitHub's `{"message": …}` error string out of a body, else the raw text.
fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

impl Tracker for Github {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let v = self.call(
            "POST",
            &format!("repos/{}/issues", self.repo),
            Some(json!({ "title": title, "body": body })),
        )?;
        Ok(IssueRef {
            id: super::want_num(&v, "number", "GitHub")?,
            url: super::want_str(&v, "html_url", "GitHub")?,
            title: title.to_string(),
        })
    }

    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError> {
        let body = match to {
            Transition::Done => json!({ "state": "closed", "state_reason": "completed" }),
            Transition::WontFix => json!({ "state": "closed", "state_reason": "not_planned" }),
            Transition::Reopen => json!({ "state": "open" }),
        };
        self.call(
            "PATCH",
            &format!("repos/{}/issues/{issue}", self.repo),
            Some(body),
        )
        .map(drop)
    }

    fn close_issue(&self, issue: &str) -> Result<(), ForgeError> {
        self.transition(issue, Transition::Done)
    }

    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "POST",
            &format!("repos/{}/issues/{issue}/comments", self.repo),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError> {
        let v = self.call("GET", &format!("repos/{}/issues/{issue}", self.repo), None)?;
        Ok(IssueState {
            open: v["state"].as_str() == Some("open"),
            url: super::want_str(&v, "html_url", "GitHub")?,
        })
    }

    fn describe(&self) -> String {
        format!("github:{}", self.repo)
    }
}

impl Forge for Github {
    fn open_pr(&self, draft: &PrDraft) -> Result<PrRef, ForgeError> {
        let v = self.call(
            "POST",
            &format!("repos/{}/pulls", self.repo),
            Some(json!({
                "title": draft.title,
                "head": draft.branch,
                "base": draft.base,
                "body": draft.body,
                "draft": true,
            })),
        )?;
        Ok(PrRef {
            id: super::want_num(&v, "number", "GitHub")?,
            url: super::want_str(&v, "html_url", "GitHub")?,
            branch: draft.branch.clone(),
        })
    }

    fn pr_state(&self, pr: &str) -> Result<PrState, ForgeError> {
        let pull = self.call("GET", &format!("repos/{}/pulls/{pr}", self.repo), None)?;
        let merged = pull["merged"].as_bool().unwrap_or(false);
        let draft = pull["draft"].as_bool().unwrap_or(false);
        let sha = pull["head"]["sha"].as_str().unwrap_or_default().to_string();

        let reviews = self.call(
            "GET",
            &format!("repos/{}/pulls/{pr}/reviews", self.repo),
            None,
        )?;
        let approvals = reviews
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|r| r["state"].as_str() == Some("APPROVED"))
                    .count() as u32
            })
            .unwrap_or(0);

        let ci = if sha.is_empty() {
            CiStatus::None
        } else {
            // Legacy *commit statuses* first. The combined endpoint reports
            // `state: "pending"` with `total_count: 0` for a repo that has no
            // commit statuses, so only trust `state` when something reported one.
            let st = self.call(
                "GET",
                &format!("repos/{}/commits/{sha}/status", self.repo),
                None,
            )?;
            if st["total_count"].as_u64().unwrap_or(0) > 0 {
                match st["state"].as_str() {
                    Some("success") => CiStatus::Success,
                    Some("pending") => CiStatus::Pending,
                    Some("failure" | "error") => CiStatus::Failure,
                    _ => CiStatus::None,
                }
            } else {
                // No commit statuses — GitHub Actions (and other apps) report via
                // the Checks API, so consult it; truly no checks ⇒ `None`.
                let checks = self.call(
                    "GET",
                    &format!("repos/{}/commits/{sha}/check-runs", self.repo),
                    None,
                )?;
                classify_check_runs(&checks)
            }
        };
        Ok(PrState {
            approvals,
            ci,
            merged,
            draft,
        })
    }

    fn merge_pr(&self, pr: &str) -> Result<(), ForgeError> {
        self.call(
            "PUT",
            &format!("repos/{}/pulls/{pr}/merge", self.repo),
            Some(json!({ "merge_method": "squash" })),
        )
        .map(drop)
    }

    fn close_pr(&self, pr: &str) -> Result<(), ForgeError> {
        self.call(
            "PATCH",
            &format!("repos/{}/pulls/{pr}", self.repo),
            Some(json!({ "state": "closed" })),
        )
        .map(drop)
    }

    fn comment_pr(&self, pr: &str, body: &str) -> Result<(), ForgeError> {
        // PR comments are issue comments in GitHub's model.
        self.call(
            "POST",
            &format!("repos/{}/issues/{pr}/comments", self.repo),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn set_pr_body(&self, pr: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "PATCH",
            &format!("repos/{}/pulls/{pr}", self.repo),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn add_label(&self, pr: &str, label: &str) -> Result<(), ForgeError> {
        // Labels live on the issues endpoint (a PR is an issue); GitHub creates
        // the label if it doesn't exist.
        self.call(
            "POST",
            &format!("repos/{}/issues/{pr}/labels", self.repo),
            Some(json!({ "labels": [label] })),
        )
        .map(drop)
    }

    fn mark_ready(&self, pr: &str) -> Result<(), ForgeError> {
        // GitHub's REST API can't un-draft a PR — only the GraphQL
        // `markPullRequestReadyForReview` mutation can. Fetch the PR's GraphQL
        // node id, then call it; skip if it's already a real PR (idempotent).
        let v = self.call("GET", &format!("repos/{}/pulls/{pr}", self.repo), None)?;
        if v["draft"].as_bool() == Some(false) {
            return Ok(());
        }
        let node_id = super::want_str(&v, "node_id", "GitHub")?;
        // GraphQL lives at `/graphql` on the API host (GHE: `<host>/api/v3` →
        // `<host>/api/graphql`).
        let gql_url = if self.host == "api.github.com" {
            "https://api.github.com/graphql".to_string()
        } else {
            format!("https://{}/graphql", self.host.trim_end_matches("/v3"))
        };
        let auth = format!("Bearer {}", self.token);
        let headers = [
            ("Authorization", auth.as_str()),
            ("Accept", "application/json"),
            ("Content-Type", "application/json"),
            ("User-Agent", "adroit"),
        ];
        let query = "mutation($id: ID!) { markPullRequestReadyForReview(input: { pullRequestId: $id }) { pullRequest { isDraft } } }";
        let resp = super::rest_call(
            self.transport.as_ref(),
            "POST",
            &gql_url,
            &headers,
            Some(json!({ "query": query, "variables": { "id": node_id } })),
            "GitHub",
            message_of,
        )?;
        // GraphQL signals failure with a 200 + `errors` array.
        if let Some(errs) = resp.get("errors").and_then(Value::as_array)
            && !errs.is_empty()
        {
            return Err(ForgeError::Api {
                status: 200,
                message: errs[0]["message"]
                    .as_str()
                    .unwrap_or("GitHub GraphQL error")
                    .to_string(),
            });
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("github:{}", self.repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::sync::Mutex;

    /// A transport that records requests and replays canned responses keyed by
    /// `"METHOD /path-substring"` — no network.
    struct FakeTransport {
        routes: Vec<(String, u16, String)>,
        seen: Mutex<Vec<String>>,
    }
    impl FakeTransport {
        fn new(routes: &[(&str, u16, &str)]) -> Self {
            Self {
                routes: routes
                    .iter()
                    .map(|(m, s, b)| (m.to_string(), *s, b.to_string()))
                    .collect(),
                seen: Mutex::new(Vec::new()),
            }
        }
    }
    impl HttpTransport for FakeTransport {
        fn request(
            &self,
            method: &str,
            url: &str,
            _headers: &[(&str, &str)],
            _body: Option<&[u8]>,
        ) -> Result<HttpResponse, ForgeError> {
            self.seen.lock().unwrap().push(format!("{method} {url}"));
            for (needle, status, body) in &self.routes {
                let (m, frag) = needle.split_once(' ').unwrap();
                if method == m && url.contains(frag) {
                    return Ok(HttpResponse {
                        status: *status,
                        body: body.clone().into_bytes(),
                    });
                }
            }
            Ok(HttpResponse {
                status: 404,
                body: br#"{"message":"no fake route"}"#.to_vec(),
            })
        }
    }

    fn github(routes: &[(&str, u16, &str)]) -> Github {
        Github::with_transport("owner/repo", Arc::new(FakeTransport::new(routes)))
    }

    #[test]
    fn create_issue_parses_number_and_url() {
        let gh = github(&[(
            "POST /issues",
            201,
            r#"{"number":7,"html_url":"https://github.com/owner/repo/issues/7"}"#,
        )]);
        let issue = gh.create_issue("Adopt PG", "body").unwrap();
        assert_eq!(issue.id, "7");
        assert_eq!(issue.url, "https://github.com/owner/repo/issues/7");
        assert_eq!(issue.title, "Adopt PG");
    }

    #[test]
    fn open_pr_sends_draft_and_parses_ref() {
        let gh = github(&[(
            "POST /pulls",
            201,
            r#"{"number":42,"html_url":"https://github.com/owner/repo/pull/42"}"#,
        )]);
        let pr = gh
            .open_pr(&PrDraft {
                branch: "adr/0007-pg".into(),
                base: "main".into(),
                title: "ADR-0007".into(),
                body: "Closes #7".into(),
            })
            .unwrap();
        assert_eq!(pr.id, "42");
        assert_eq!(pr.branch, "adr/0007-pg");
    }

    #[test]
    fn auth_failure_maps_to_auth_error() {
        let gh = github(&[("POST /issues", 401, r#"{"message":"Bad credentials"}"#)]);
        let err = gh.create_issue("x", "y").unwrap_err();
        assert!(matches!(err, ForgeError::Auth(_)));
    }

    #[test]
    fn add_label_posts_to_the_issue_labels_endpoint() {
        // Labels share the issues endpoint (a PR is an issue); a matching route
        // confirms the path (a wrong one would 404 → Err).
        let gh = github(&[("POST /issues/42/labels", 200, "[]")]);
        assert!(gh.add_label("42", "review-by:2026-06-20").is_ok());
    }

    #[test]
    fn mark_ready_undrafts_via_graphql() {
        // Fetch node_id (REST), then call the GraphQL mutation.
        let gh = github(&[
            (
                "GET /pulls/42",
                200,
                r#"{"draft":true,"node_id":"PR_node_42"}"#,
            ),
            (
                "POST /graphql",
                200,
                r#"{"data":{"markPullRequestReadyForReview":{"pullRequest":{"isDraft":false}}}}"#,
            ),
        ]);
        assert!(gh.mark_ready("42").is_ok());
    }

    #[test]
    fn mark_ready_is_a_noop_when_already_ready() {
        // Already a real PR → no GraphQL call needed (the single route is enough).
        let gh = github(&[("GET /pulls/42", 200, r#"{"draft":false,"node_id":"x"}"#)]);
        assert!(gh.mark_ready("42").is_ok());
    }

    #[test]
    fn pr_state_counts_approvals_and_ci() {
        let gh = github(&[
            (
                "GET /pulls/42/reviews",
                200,
                r#"[{"state":"APPROVED"},{"state":"COMMENTED"},{"state":"APPROVED"}]"#,
            ),
            (
                "GET /commits/abc/status",
                200,
                r#"{"state":"success","total_count":1}"#,
            ),
            (
                "GET /pulls/42",
                200,
                r#"{"merged":false,"draft":true,"head":{"sha":"abc"}}"#,
            ),
        ]);
        let st = gh.pr_state("42").unwrap();
        assert_eq!(st.approvals, 2);
        assert_eq!(st.ci, CiStatus::Success);
        assert!(!st.merged && st.draft);
    }

    #[test]
    fn pr_state_maps_no_checks_to_none_not_pending() {
        // The dogfood case: a repo with no commit statuses and no check runs.
        // The combined-status endpoint returns pending/total_count:0; we must
        // fall through to check-runs and report `None`, not a phantom `Pending`.
        let gh = github(&[
            ("GET /pulls/9/reviews", 200, r#"[]"#),
            (
                "GET /commits/def/status",
                200,
                r#"{"state":"pending","total_count":0}"#,
            ),
            (
                "GET /commits/def/check-runs",
                200,
                r#"{"total_count":0,"check_runs":[]}"#,
            ),
            (
                "GET /pulls/9",
                200,
                r#"{"merged":false,"draft":true,"head":{"sha":"def"}}"#,
            ),
        ]);
        assert_eq!(gh.pr_state("9").unwrap().ci, CiStatus::None);
    }

    #[test]
    fn pr_state_reads_ci_from_check_runs_when_no_commit_status() {
        // GitHub Actions report via the Checks API, not commit statuses.
        let gh = github(&[
            ("GET /pulls/9/reviews", 200, r#"[]"#),
            (
                "GET /commits/def/status",
                200,
                r#"{"state":"pending","total_count":0}"#,
            ),
            (
                "GET /commits/def/check-runs",
                200,
                r#"{"total_count":1,"check_runs":[{"status":"completed","conclusion":"success"}]}"#,
            ),
            (
                "GET /pulls/9",
                200,
                r#"{"merged":false,"draft":false,"head":{"sha":"def"}}"#,
            ),
        ]);
        assert_eq!(gh.pr_state("9").unwrap().ci, CiStatus::Success);
    }

    #[test]
    fn classify_check_runs_rolls_up_states() {
        let none = serde_json::json!({"total_count":0,"check_runs":[]});
        assert_eq!(classify_check_runs(&none), CiStatus::None);

        let pending = serde_json::json!({"check_runs":[
            {"status":"completed","conclusion":"success"},
            {"status":"in_progress","conclusion":null}
        ]});
        assert_eq!(classify_check_runs(&pending), CiStatus::Pending);

        let failing = serde_json::json!({"check_runs":[
            {"status":"completed","conclusion":"success"},
            {"status":"completed","conclusion":"failure"}
        ]});
        assert_eq!(classify_check_runs(&failing), CiStatus::Failure);

        let success = serde_json::json!({"check_runs":[
            {"status":"completed","conclusion":"success"},
            {"status":"completed","conclusion":"skipped"}
        ]});
        assert_eq!(classify_check_runs(&success), CiStatus::Success);
    }
}
