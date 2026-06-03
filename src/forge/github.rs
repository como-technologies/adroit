//! GitHub adapter — implements both [`Forge`] (Pull Requests) and [`Tracker`]
//! (GitHub Issues) over one client + one token (`ADROIT_GITHUB_TOKEN`). All
//! HTTP goes through the injectable [`HttpTransport`] seam, so the unit tests
//! drive it with recorded fixtures and never touch the network.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    CiStatus, Forge, ForgeError, HttpResponse, HttpTransport, IssueRef, IssueState, PrDraft, PrRef,
    PrState, Tracker, Transition, UreqTransport,
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
            message: format!("invalid JSON from GitHub: {e}"),
        })
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

/// Map a response status to an error (or `Ok` for 2xx).
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

/// Pull GitHub's `{"message": …}` error string out of a body, else the raw text.
fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

/// Require a string field, else an `Api` error naming the missing key.
fn want_str(v: &Value, key: &str) -> Result<String, ForgeError> {
    v[key].as_str().map(str::to_string).ok_or(ForgeError::Api {
        status: 0,
        message: format!("GitHub response missing `{key}`"),
    })
}

/// A numeric id rendered as a string (GitHub issue/PR numbers).
fn want_num(v: &Value, key: &str) -> Result<String, ForgeError> {
    v[key]
        .as_u64()
        .map(|n| n.to_string())
        .ok_or(ForgeError::Api {
            status: 0,
            message: format!("GitHub response missing numeric `{key}`"),
        })
}

impl Tracker for Github {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let v = self.call(
            "POST",
            &format!("repos/{}/issues", self.repo),
            Some(json!({ "title": title, "body": body })),
        )?;
        Ok(IssueRef {
            id: want_num(&v, "number")?,
            url: want_str(&v, "html_url")?,
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
            url: want_str(&v, "html_url")?,
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
            id: want_num(&v, "number")?,
            url: want_str(&v, "html_url")?,
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
            let st = self.call(
                "GET",
                &format!("repos/{}/commits/{sha}/status", self.repo),
                None,
            )?;
            match st["state"].as_str() {
                Some("success") => CiStatus::Success,
                Some("pending") => CiStatus::Pending,
                Some("failure" | "error") => CiStatus::Failure,
                _ => CiStatus::None,
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

    fn describe(&self) -> String {
        format!("github:{}", self.repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn pr_state_counts_approvals_and_ci() {
        let gh = github(&[
            (
                "GET /pulls/42/reviews",
                200,
                r#"[{"state":"APPROVED"},{"state":"COMMENTED"},{"state":"APPROVED"}]"#,
            ),
            ("GET /commits/abc/status", 200, r#"{"state":"success"}"#),
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
}
