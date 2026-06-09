//! GitLab adapter — implements both [`Forge`] (Merge Requests) and [`Tracker`]
//! (GitLab Issues) over one client + one token (`ADROIT_GITLAB_TOKEN`), via the
//! same [`HttpTransport`] seam as GitHub. Issues/MRs are addressed by their
//! project-scoped `iid`. Host is configurable for self-managed instances.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    CiStatus, Forge, ForgeComment, ForgeError, HttpTransport, IssueRef, IssueState, PrDraft, PrRef,
    PrState, Tracker, Transition, UreqTransport,
};

/// A GitLab REST v4 client scoped to one project.
#[derive(Clone)]
pub struct Gitlab {
    /// API host (`gitlab.com`, or a self-managed host).
    host: String,
    /// `group/project` slug (URL-encoded in paths) or numeric id.
    project: String,
    token: String,
    transport: Arc<dyn HttpTransport>,
}

impl Gitlab {
    pub fn new(host: Option<String>, project: String, token: String) -> Self {
        Self {
            host: host.unwrap_or_else(|| "gitlab.com".to_string()),
            project,
            token,
            transport: Arc::new(UreqTransport),
        }
    }

    pub fn with_transport(project: &str, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            host: "gitlab.com".to_string(),
            project: project.to_string(),
            token: "test-token".to_string(),
            transport,
        }
    }

    /// `group/project` → `group%2Fproject` for the path segment.
    fn proj(&self) -> String {
        self.project.replace('/', "%2F")
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let url = format!(
            "https://{}/api/v4/{}",
            self.host,
            path.trim_start_matches('/')
        );
        let headers = [
            ("PRIVATE-TOKEN", self.token.as_str()),
            ("Content-Type", "application/json"),
            ("User-Agent", "adroit"),
        ];
        super::rest_call(
            self.transport.as_ref(),
            method,
            &url,
            &headers,
            body,
            "GitLab",
            message_of,
        )
    }
}

/// Construct the GitLab `(forge, tracker)` adapters from config, or
/// `(None, None)` when inactive. GitLab owns its own token env var + slug.
pub fn open(cfg: &crate::config::ForgeConfig) -> super::Adapters {
    let token = cfg
        .token
        .clone()
        .or_else(|| std::env::var("ADROIT_GITLAB_TOKEN").ok())
        .or_else(|| crate::config::load_credential("gitlab"));
    match (token, cfg.repo.clone()) {
        (Some(token), Some(project)) => {
            let gl = Gitlab::new(cfg.host.clone(), project, token);
            (Some(Box::new(gl.clone())), Some(Box::new(gl)))
        }
        _ => (None, None),
    }
}

/// GitLab error bodies use `{"message": …}` or `{"error": …}`.
fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

fn want_iid(v: &Value) -> Result<String, ForgeError> {
    v["iid"]
        .as_u64()
        .map(|n| n.to_string())
        .ok_or(ForgeError::Api {
            status: 0,
            message: "GitLab response missing `iid`".to_string(),
        })
}

impl Tracker for Gitlab {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let v = self.call(
            "POST",
            &format!("projects/{}/issues", self.proj()),
            Some(json!({ "title": title, "description": body })),
        )?;
        Ok(IssueRef {
            id: want_iid(&v)?,
            url: super::want_str(&v, "web_url", "GitLab")?,
            title: title.to_string(),
        })
    }

    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError> {
        let event = match to {
            Transition::Done | Transition::WontFix => "close",
            Transition::Reopen => "reopen",
        };
        self.call(
            "PUT",
            &format!("projects/{}/issues/{issue}", self.proj()),
            Some(json!({ "state_event": event })),
        )
        .map(drop)
    }

    fn close_issue(&self, issue: &str) -> Result<(), ForgeError> {
        self.transition(issue, Transition::Done)
    }

    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "POST",
            &format!("projects/{}/issues/{issue}/notes", self.proj()),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError> {
        let v = self.call(
            "GET",
            &format!("projects/{}/issues/{issue}", self.proj()),
            None,
        )?;
        Ok(IssueState {
            // GitLab issue state is "opened" / "closed".
            open: v["state"].as_str() == Some("opened"),
            url: super::want_str(&v, "web_url", "GitLab")?,
        })
    }

    fn set_due_date(&self, issue: &str, date: Option<&str>) -> Result<(), ForgeError> {
        // GitLab issues carry a native `due_date` (YYYY-MM-DD); `null` clears it.
        self.call(
            "PUT",
            &format!("projects/{}/issues/{issue}", self.proj()),
            Some(json!({ "due_date": date })),
        )
        .map(drop)
    }

    fn comments_on_issue(&self, issue: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        let v = self.call(
            "GET",
            &format!("projects/{}/issues/{issue}/notes?per_page=100", self.proj()),
            None,
        )?;
        Ok(super::parse_rest_comments(&v, "body"))
    }

    fn update_issue_comment(
        &self,
        issue: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        // GitLab edits a note under its parent issue (the iid is in the path).
        self.call(
            "PUT",
            &format!("projects/{}/issues/{issue}/notes/{comment_id}", self.proj()),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn describe(&self) -> String {
        format!("gitlab:{}", self.project)
    }
}

impl Forge for Gitlab {
    fn open_pr(&self, draft: &PrDraft) -> Result<PrRef, ForgeError> {
        // GitLab marks a draft MR with a `Draft:` title prefix.
        let v = self.call(
            "POST",
            &format!("projects/{}/merge_requests", self.proj()),
            Some(json!({
                "source_branch": draft.branch,
                "target_branch": draft.base,
                "title": format!("Draft: {}", draft.title),
                "description": draft.body,
            })),
        )?;
        Ok(PrRef {
            id: want_iid(&v)?,
            url: super::want_str(&v, "web_url", "GitLab")?,
            branch: draft.branch.clone(),
        })
    }

    fn pr_state(&self, pr: &str) -> Result<PrState, ForgeError> {
        let mr = self.call(
            "GET",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            None,
        )?;
        let merged = mr["state"].as_str() == Some("merged");
        let draft = mr["draft"].as_bool().unwrap_or(false);
        let ci = match mr["head_pipeline"]["status"].as_str() {
            Some("success") => CiStatus::Success,
            Some("running" | "pending" | "created") => CiStatus::Pending,
            Some("failed" | "canceled") => CiStatus::Failure,
            _ => CiStatus::None,
        };
        let approvals = self
            .call(
                "GET",
                &format!("projects/{}/merge_requests/{pr}/approvals", self.proj()),
                None,
            )
            .ok()
            .and_then(|a| a["approved_by"].as_array().map(|v| v.len() as u32))
            .unwrap_or(0);
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
            &format!("projects/{}/merge_requests/{pr}/merge", self.proj()),
            Some(json!({})),
        )
        .map(drop)
    }

    fn close_pr(&self, pr: &str) -> Result<(), ForgeError> {
        self.call(
            "PUT",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            Some(json!({ "state_event": "close" })),
        )
        .map(drop)
    }

    fn comment_pr(&self, pr: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "POST",
            &format!("projects/{}/merge_requests/{pr}/notes", self.proj()),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn comments_on_pr(&self, pr: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        let v = self.call(
            "GET",
            &format!(
                "projects/{}/merge_requests/{pr}/notes?per_page=100",
                self.proj()
            ),
            None,
        )?;
        Ok(super::parse_rest_comments(&v, "body"))
    }

    fn update_pr_comment(&self, pr: &str, comment_id: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "PUT",
            &format!(
                "projects/{}/merge_requests/{pr}/notes/{comment_id}",
                self.proj()
            ),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn set_pr_body(&self, pr: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "PUT",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            Some(json!({ "description": body })),
        )
        .map(drop)
    }

    fn add_label(&self, pr: &str, label: &str) -> Result<(), ForgeError> {
        // `add_labels` merges into the MR's labels (creating the label if new).
        self.call(
            "PUT",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            Some(json!({ "add_labels": label })),
        )
        .map(drop)
    }

    fn mark_ready(&self, pr: &str) -> Result<(), ForgeError> {
        // GitLab drafts an MR via a `Draft:` title prefix — un-draft by removing
        // it. Skip if it's already ready (idempotent).
        let v = self.call(
            "GET",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            None,
        )?;
        if v["draft"].as_bool() == Some(false) {
            return Ok(());
        }
        let title = v["title"].as_str().unwrap_or_default();
        let ready = title
            .strip_prefix("Draft: ")
            .or_else(|| title.strip_prefix("Draft:"))
            .unwrap_or(title)
            .trim_start();
        self.call(
            "PUT",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            Some(json!({ "title": ready })),
        )
        .map(drop)
    }

    fn describe(&self) -> String {
        format!("gitlab:{}", self.project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::sync::Mutex;

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

    fn gitlab(routes: &[(&str, u16, &str)]) -> Gitlab {
        Gitlab::with_transport("grp/proj", Arc::new(FakeTransport::new(routes)))
    }

    #[test]
    fn create_issue_uses_iid_and_encodes_project() {
        let gl = gitlab(&[(
            "POST /projects/grp%2Fproj/issues",
            201,
            r#"{"iid":7,"web_url":"https://gitlab.com/grp/proj/-/issues/7"}"#,
        )]);
        let issue = gl.create_issue("Adopt PG", "desc").unwrap();
        assert_eq!(issue.id, "7");
        assert!(issue.url.contains("/issues/7"));
    }

    #[test]
    fn open_mr_prefixes_draft_and_parses_iid() {
        let gl = gitlab(&[(
            "POST /merge_requests",
            201,
            r#"{"iid":42,"web_url":"https://gitlab.com/grp/proj/-/merge_requests/42"}"#,
        )]);
        let pr = gl
            .open_pr(&PrDraft {
                branch: "adr/0007-pg".into(),
                base: "main".into(),
                title: "ADR-0007".into(),
                body: "body".into(),
            })
            .unwrap();
        assert_eq!(pr.id, "42");
    }

    #[test]
    fn pr_state_reads_approvals_and_pipeline() {
        let gl = gitlab(&[
            (
                "GET /merge_requests/42/approvals",
                200,
                r#"{"approved_by":[{"user":{}},{"user":{}}]}"#,
            ),
            (
                "GET /merge_requests/42",
                200,
                r#"{"state":"opened","draft":true,"head_pipeline":{"status":"success"}}"#,
            ),
        ]);
        let st = gl.pr_state("42").unwrap();
        assert_eq!(st.approvals, 2);
        assert_eq!(st.ci, CiStatus::Success);
        assert!(!st.merged && st.draft);
    }

    #[test]
    fn add_label_updates_the_merge_request() {
        // A matching route confirms the endpoint+method (a wrong path 404s → Err).
        let gl = gitlab(&[("PUT /merge_requests/42", 200, "{}")]);
        assert!(gl.add_label("42", "review-by:2026-06-20").is_ok());
    }

    #[test]
    fn set_due_date_updates_the_issue() {
        let gl = gitlab(&[("PUT /issues/7", 200, "{}")]);
        assert!(gl.set_due_date("7", Some("2026-06-20")).is_ok());
        assert!(gl.set_due_date("7", None).is_ok()); // clear
    }

    #[test]
    fn mark_ready_strips_the_draft_title() {
        // GET the draft MR, then PUT the de-prefixed title.
        let gl = gitlab(&[
            (
                "GET /merge_requests/42",
                200,
                r#"{"draft":true,"title":"Draft: ADR-0007"}"#,
            ),
            ("PUT /merge_requests/42", 200, "{}"),
        ]);
        assert!(gl.mark_ready("42").is_ok());
    }

    #[test]
    fn mark_ready_is_a_noop_when_already_ready() {
        let gl = gitlab(&[(
            "GET /merge_requests/42",
            200,
            r#"{"draft":false,"title":"ADR-0007"}"#,
        )]);
        assert!(gl.mark_ready("42").is_ok());
    }

    #[test]
    fn upsert_pr_comment_edits_the_marked_mr_note() {
        let marker = "<!-- adroit:review-kickoff -->";
        let listing = format!(r#"[{{"id":9,"body":"old\n\n{marker}"}}]"#);
        // GET notes + PUT note/9 wired (no POST) → Ok proves the edit path.
        let gl = gitlab(&[
            ("GET /merge_requests/42/notes", 200, &listing),
            ("PUT /merge_requests/42/notes/9", 200, "{}"),
        ]);
        assert!(gl.upsert_pr_comment("42", marker, "new body").is_ok());
    }

    #[test]
    fn upsert_issue_comment_creates_a_note_when_none_is_marked() {
        let marker = "<!-- adroit:review-deadline -->";
        // GET notes (none marked) + POST notes wired (no PUT) → Ok proves create.
        let gl = gitlab(&[
            ("GET /issues/7/notes", 200, "[]"),
            ("POST /issues/7/notes", 201, "{}"),
        ]);
        assert!(gl.upsert_issue_comment("7", marker, "deadline").is_ok());
    }
}
