//! Jira adapter — a **split** [`Tracker`] (Jira has no PRs, so it implements no
//! [`Forge`](super::Forge)). Pairs with a GitHub/GitLab forge when
//! `forge.tracker = jira`, reaching the GitLab-MRs + Jira-issues setup. REST v2
//! (plain-text descriptions), which both Jira Cloud and Server/Data Center
//! serve. **Auth follows the deployment:** Cloud uses Basic `email:token` (set
//! `ADROIT_JIRA_EMAIL`); Server/Data Center uses a Bearer Personal Access Token
//! (omit the email). Config: `forge.tracker_host` (`site.atlassian.net` for
//! Cloud, or a self-hosted host like `jira.example.com`) +
//! `forge.tracker_project` (the project key) + env `ADROIT_JIRA_TOKEN`
//! (+ `ADROIT_JIRA_EMAIL` for Cloud).

use std::sync::Arc;

use serde_json::{Value, json};

use super::{ForgeError, HttpTransport, IssueRef, IssueState, Tracker, Transition, UreqTransport};

/// A Jira REST client scoped to one project (Cloud or Server/Data Center).
#[derive(Clone)]
pub struct Jira {
    host: String,    // site.atlassian.net, or a self-hosted host
    project: String, // project key, e.g. OPS
    auth: String,    // pre-built header: Basic (Cloud) or Bearer PAT (Server/DC)
    transport: Arc<dyn HttpTransport>,
}

impl Jira {
    /// `email` selects the auth scheme: `Some` ⇒ Jira **Cloud** (Basic
    /// `email:token`); `None` ⇒ Jira **Server/Data Center** (Bearer PAT).
    pub fn new(host: String, project: String, email: Option<&str>, token: &str) -> Self {
        let auth = match email {
            Some(email) => format!("Basic {}", base64(format!("{email}:{token}").as_bytes())),
            None => format!("Bearer {token}"),
        };
        Self {
            host,
            project,
            auth,
            transport: Arc::new(UreqTransport),
        }
    }

    /// Build a client over an injected transport. Exposed (like the GitHub /
    /// GitLab adapters' `with_transport`) for the forge fault-injection suite
    /// (tests/forge_faults.rs) as well as unit tests.
    pub fn with_transport(host: &str, project: &str, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            host: host.to_string(),
            project: project.to_string(),
            auth: "Basic test".to_string(),
            transport,
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let url = format!(
            "https://{}/rest/api/2/{}",
            self.host,
            path.trim_start_matches('/')
        );
        let headers = [
            ("Authorization", self.auth.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
            ("User-Agent", "adroit"),
        ];
        super::rest_call(
            self.transport.as_ref(),
            method,
            &url,
            &headers,
            body,
            "Jira",
            message_of,
        )
    }

    fn browse_url(&self, key: &str) -> String {
        format!("https://{}/browse/{key}", self.host)
    }
}

/// Construct a Jira tracker, or `None` if inactive (missing `tracker_host` /
/// `tracker_project` / `ADROIT_JIRA_TOKEN`). `ADROIT_JIRA_EMAIL` is optional —
/// set it for Cloud (Basic auth); omit it for Server/Data Center (Bearer PAT).
pub fn open(cfg: &crate::config::ForgeConfig) -> Option<Box<dyn Tracker>> {
    let host = cfg.tracker_host.clone()?;
    let project = cfg.tracker_project.clone()?;
    let token = std::env::var("ADROIT_JIRA_TOKEN")
        .ok()
        .or_else(|| crate::config::load_credential("jira"))?;
    // Email is optional: present ⇒ Jira Cloud (Basic email:token); absent ⇒
    // Jira Server/Data Center (Bearer PAT).
    let email = std::env::var("ADROIT_JIRA_EMAIL")
        .ok()
        .or_else(|| crate::config::load_credential("jira_email"));
    Some(Box::new(Jira::new(host, project, email.as_deref(), &token)))
}

fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            // Jira errors: {"errorMessages":[..]} or {"errors":{..}}.
            v["errorMessages"][0]
                .as_str()
                .map(str::to_string)
                .or_else(|| v["message"].as_str().map(str::to_string))
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

impl Tracker for Jira {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let v = self.call(
            "POST",
            "issue",
            Some(json!({
                "fields": {
                    "project": { "key": self.project },
                    "summary": title,
                    "description": body,
                    "issuetype": { "name": "Task" },
                }
            })),
        )?;
        let key = super::want_str(&v, "key", "Jira")?;
        Ok(IssueRef {
            id: key.clone(),
            url: self.browse_url(&key),
            title: title.to_string(),
        })
    }

    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError> {
        // Jira transitions are workflow-specific: look one up by name, best-effort.
        let wanted: &[&str] = match to {
            Transition::Done => &["done", "close", "resolve", "complete"],
            Transition::WontFix => &["won't do", "wont do", "won't fix", "reject", "decline"],
            Transition::Reopen => &["reopen", "to do", "open"],
        };
        let list = self.call("GET", &format!("issue/{issue}/transitions"), None)?;
        let id = list["transitions"].as_array().and_then(|ts| {
            ts.iter().find_map(|t| {
                let name = t["name"].as_str()?.to_ascii_lowercase();
                wanted
                    .iter()
                    .any(|w| name.contains(w))
                    .then(|| t["id"].as_str().map(str::to_string))?
            })
        });
        match id {
            Some(id) => self
                .call(
                    "POST",
                    &format!("issue/{issue}/transitions"),
                    Some(json!({ "transition": { "id": id } })),
                )
                .map(drop),
            None => {
                eprintln!("adroit: no matching Jira transition for {issue}; left unchanged");
                Ok(())
            }
        }
    }

    fn close_issue(&self, issue: &str) -> Result<(), ForgeError> {
        self.transition(issue, Transition::Done)
    }

    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError> {
        self.call(
            "POST",
            &format!("issue/{issue}/comment"),
            Some(json!({ "body": body })),
        )
        .map(drop)
    }

    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError> {
        let v = self.call("GET", &format!("issue/{issue}?fields=status"), None)?;
        // statusCategory.key is "new"/"indeterminate"/"done".
        let done = v["fields"]["status"]["statusCategory"]["key"].as_str() == Some("done");
        Ok(IssueState {
            open: !done,
            url: self.browse_url(issue),
        })
    }

    fn set_due_date(&self, issue: &str, date: Option<&str>) -> Result<(), ForgeError> {
        // Jira's native `duedate` field (YYYY-MM-DD); `null` clears it.
        self.call(
            "PUT",
            &format!("issue/{issue}"),
            Some(json!({ "fields": { "duedate": date } })),
        )
        .map(drop)
    }

    fn describe(&self) -> String {
        format!("jira:{}", self.project)
    }
}

// Jira is tracker-only: it implements `Tracker`, not `Forge` (the factory only
// ever boxes it as `Box<dyn Tracker>`). The `(Option<dyn Forge>, Option<dyn
// Tracker>)` adapter pair keeps the two roles independent, so there's no need —
// and it would be a Liskov violation — to give Jira a `Forge` impl that panics.

/// Standard base64 (RFC 4648) — tiny, to avoid a dep for the one Basic-auth header.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::sync::Mutex;

    struct Fake(Vec<(String, u16, String)>, Mutex<Vec<String>>);
    impl HttpTransport for Fake {
        fn request(
            &self,
            method: &str,
            url: &str,
            _: &[(&str, &str)],
            _: Option<&[u8]>,
        ) -> Result<HttpResponse, ForgeError> {
            self.1.lock().unwrap().push(format!("{method} {url}"));
            for (needle, status, body) in &self.0 {
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
                body: br#"{"errorMessages":["no route"]}"#.to_vec(),
            })
        }
    }
    fn jira(routes: &[(&str, u16, &str)]) -> Jira {
        let r = routes
            .iter()
            .map(|(m, s, b)| (m.to_string(), *s, b.to_string()))
            .collect();
        Jira::with_transport(
            "site.atlassian.net",
            "OPS",
            Arc::new(Fake(r, Mutex::new(vec![]))),
        )
    }

    #[test]
    fn auth_scheme_follows_deployment() {
        // Cloud: email present → Basic email:token.
        let cloud = Jira::new(
            "x.atlassian.net".into(),
            "OPS".into(),
            Some("me@corp.com"),
            "tok",
        );
        assert!(cloud.auth.starts_with("Basic "));
        // Server / Data Center: no email → Bearer PAT.
        let server = Jira::new("jira.example.com".into(), "OPS".into(), None, "tok");
        assert_eq!(server.auth, "Bearer tok");
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b"foo:bar"), "Zm9vOmJhcg==");
        assert_eq!(base64(b"a@b.com:tok"), "YUBiLmNvbTp0b2s=");
    }

    #[test]
    fn create_issue_returns_key_and_browse_url() {
        let j = jira(&[("POST /issue", 201, r#"{"key":"OPS-123"}"#)]);
        let issue = j.create_issue("Adopt PG", "desc").unwrap();
        assert_eq!(issue.id, "OPS-123");
        assert_eq!(issue.url, "https://site.atlassian.net/browse/OPS-123");
    }

    #[test]
    fn issue_state_maps_status_category() {
        let j = jira(&[(
            "GET /issue/OPS-1",
            200,
            r#"{"fields":{"status":{"statusCategory":{"key":"done"}}}}"#,
        )]);
        assert!(!j.issue_state("OPS-1").unwrap().open);
    }

    #[test]
    fn set_due_date_updates_the_duedate_field() {
        // PUT /issue/OPS-1 returns 204 No Content; a matching route confirms it.
        let j = jira(&[("PUT /issue/OPS-1", 204, "")]);
        assert!(j.set_due_date("OPS-1", Some("2026-06-20")).is_ok());
        assert!(j.set_due_date("OPS-1", None).is_ok()); // clear
    }
}
