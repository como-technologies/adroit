//! GitLab adapter — implements both [`Forge`] (Merge Requests) and [`Tracker`]
//! (GitLab Issues) over one client + one token (`ADROIT_GITLAB_TOKEN`), via the
//! same [`HttpTransport`] seam as GitHub. Issues/MRs are addressed by their
//! project-scoped `iid`. Host is configurable for self-managed instances.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    CiStatus, Forge, ForgeError, HttpResponse, HttpTransport, IssueRef, IssueState, PrDraft, PrRef,
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
        let bytes = body.map(|b| serde_json::to_vec(&b).expect("serialize JSON body"));
        let resp = self
            .transport
            .request(method, &url, &headers, bytes.as_deref())?;
        classify(&resp)?;
        if resp.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&resp.body).map_err(|e| ForgeError::Api {
            status: resp.status,
            message: format!("invalid JSON from GitLab: {e}"),
        })
    }
}

/// Construct the GitLab `(forge, tracker)` adapters from config, or
/// `(None, None)` when inactive. GitLab owns its own token env var + slug.
pub fn open(cfg: &crate::config::ForgeConfig) -> super::Adapters {
    let token = cfg
        .token
        .clone()
        .or_else(|| std::env::var("ADROIT_GITLAB_TOKEN").ok());
    match (token, cfg.repo.clone()) {
        (Some(token), Some(project)) => {
            let gl = Gitlab::new(cfg.host.clone(), project, token);
            (Some(Box::new(gl.clone())), Some(Box::new(gl)))
        }
        _ => (None, None),
    }
}

fn classify(resp: &HttpResponse) -> Result<(), ForgeError> {
    match resp.status {
        200..=299 => Ok(()),
        401 | 403 => Err(ForgeError::Auth(message_of(&resp.body))),
        status => Err(ForgeError::Api {
            status,
            message: message_of(&resp.body),
        }),
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

fn want_str(v: &Value, key: &str) -> Result<String, ForgeError> {
    v[key].as_str().map(str::to_string).ok_or(ForgeError::Api {
        status: 0,
        message: format!("GitLab response missing `{key}`"),
    })
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
            url: want_str(&v, "web_url")?,
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
            url: want_str(&v, "web_url")?,
        })
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
            url: want_str(&v, "web_url")?,
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

    fn set_pr_body(&self, pr: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "PUT",
            &format!("projects/{}/merge_requests/{pr}", self.proj()),
            Some(json!({ "description": body })),
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
}
