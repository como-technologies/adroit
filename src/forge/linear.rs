//! Linear adapter — a **split** [`Tracker`] (Linear has no PRs, so it implements
//! no [`Forge`](super::Forge), like [`jira`](super::jira)). Pairs with a
//! GitHub/GitLab forge when `forge.tracker = linear`.
//!
//! **GraphQL, not REST.** Linear exposes a single GraphQL endpoint
//! (`POST https://api.linear.app/graphql`) — but a GraphQL request is just a
//! `POST` of `{query, variables}` returning JSON, so it reuses
//! [`super::rest_call`] with one extra check: GraphQL returns **HTTP 200 even on
//! a query error** (the failure is in an `errors` array), so [`Linear::gql`]
//! surfaces that as an `Api` error.
//!
//! **Auth:** a Linear *personal API key* placed in the `Authorization` header
//! **verbatim** (no `Bearer` scheme) — env `ADROIT_LINEAR_TOKEN`, or
//! `adroit auth linear`. **Config:** `forge.tracker_project` = the **team key**
//! (e.g. `ENG`); new issues file to that team. `forge.tracker_host` is unused
//! (Linear is a single hosted service).
//!
//! **Identity & the stateless recovery.** A Linear issue carries a human
//! `identifier` (`ENG-123`) plus an internal UUID; mutations need the UUID. adroit
//! re-derives an issue id on a *later* `set-status` run from the **trailing path
//! segment** of the URL in `## References` (see `read_refs` in `mod.rs`). Linear's
//! browser URL is `…/issue/ENG-123/<slug>`, whose trailing segment is the *slug* —
//! useless — so we record the **slug-stripped** URL (`…/issue/ENG-123`, which still
//! resolves), making the recovered id `ENG-123`. Each later method resolves
//! `ENG-123` → UUID by filtering on **team-key + number** (documented filter
//! fields), not by assuming `issue(id:)` accepts a non-UUID identifier.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    ForgeComment, ForgeError, HttpTransport, IssueRef, IssueState, Tracker, Transition,
    UreqTransport,
};

const API: &str = "https://api.linear.app/graphql";

/// A Linear GraphQL client scoped to one team.
#[derive(Clone)]
pub struct Linear {
    team: String,  // team key, e.g. ENG
    token: String, // personal API key — the Authorization header value, verbatim
    transport: Arc<dyn HttpTransport>,
}

impl Linear {
    pub fn new(team: String, token: &str) -> Self {
        Self {
            team,
            token: token.to_string(),
            transport: Arc::new(UreqTransport),
        }
    }

    /// Build a client over an injected transport. Exposed (like the other
    /// adapters' `with_transport`) for the forge fault-injection suite
    /// (tests/forge_faults.rs) as well as unit tests.
    pub fn with_transport(team: &str, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            team: team.to_string(),
            token: "test".to_string(),
            transport,
        }
    }

    fn gql(&self, query: &str, variables: Value) -> Result<Value, ForgeError> {
        let headers = [
            ("Authorization", self.token.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
            ("User-Agent", "adroit"),
        ];
        let v = super::rest_call(
            self.transport.as_ref(),
            "POST",
            API,
            &headers,
            Some(json!({ "query": query, "variables": variables })),
            "Linear",
            message_of,
        )?;
        // GraphQL signals failure with an `errors` array under a 200.
        if let Some(errs) = v.get("errors").and_then(Value::as_array)
            && !errs.is_empty()
        {
            return Err(ForgeError::Api {
                status: 200,
                message: errs[0]["message"]
                    .as_str()
                    .unwrap_or("Linear GraphQL error")
                    .to_string(),
            });
        }
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Resolve a human identifier (`ENG-123`) to the issue node carrying its UUID
    /// (`id`), `url`, current `state.type`, and the team's workflow `states`.
    fn resolve(&self, identifier: &str) -> Result<Value, ForgeError> {
        let (key, number) = split_identifier(identifier).ok_or_else(|| ForgeError::Api {
            status: 0,
            message: format!("Linear: unrecognized issue identifier `{identifier}`"),
        })?;
        let data = self.gql(RESOLVE, json!({ "key": key, "num": number }))?;
        data["issues"]["nodes"]
            .get(0)
            .filter(|n| n.is_object())
            .cloned()
            .ok_or_else(|| ForgeError::Api {
                status: 0,
                message: format!("Linear: no issue {identifier}"),
            })
    }
}

const TEAM_ID: &str =
    "query($key: String!) { teams(filter: { key: { eq: $key } }, first: 1) { nodes { id } } }";

const CREATE: &str = "\
mutation($teamId: String!, $title: String!, $description: String) {
  issueCreate(input: { teamId: $teamId, title: $title, description: $description }) {
    issue { identifier url }
  }
}";

const RESOLVE: &str = "\
query($key: String!, $num: Float!) {
  issues(filter: { team: { key: { eq: $key } }, number: { eq: $num } }, first: 1) {
    nodes { id url state { type } team { states { nodes { id type } } } }
  }
}";

const UPDATE_STATE: &str = "\
mutation($id: String!, $stateId: String!) {
  issueUpdate(id: $id, input: { stateId: $stateId }) { success }
}";

const COMMENT: &str = "\
mutation($id: String!, $body: String!) {
  commentCreate(input: { issueId: $id, body: $body }) { success }
}";

const SET_DUE: &str = "\
mutation($id: String!, $due: TimelessDate) {
  issueUpdate(id: $id, input: { dueDate: $due }) { success }
}";

const COMMENTS: &str = "\
query($key: String!, $num: Float!) {
  issues(filter: { team: { key: { eq: $key } }, number: { eq: $num } }, first: 1) {
    nodes { comments { nodes { id body } } }
  }
}";

const UPDATE_COMMENT: &str = "\
mutation($id: String!, $body: String!) {
  commentUpdate(id: $id, input: { body: $body }) { success }
}";

/// Construct a Linear tracker, or `None` if inactive (missing `tracker_project`
/// or `ADROIT_LINEAR_TOKEN`). `tracker_project` is the **team key** (e.g. `ENG`).
pub fn open(cfg: &crate::config::ForgeConfig) -> Option<Box<dyn Tracker>> {
    let team = cfg.tracker_project.clone()?;
    let token = std::env::var("ADROIT_LINEAR_TOKEN")
        .ok()
        .or_else(|| crate::config::load_credential("linear"))?;
    Some(Box::new(Linear::new(team, &token)))
}

fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["errors"][0]["message"]
                .as_str()
                .map(str::to_string)
                .or_else(|| v["message"].as_str().map(str::to_string))
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

/// Split `ENG-123` into (`ENG`, `123.0`). Linear's `IssueFilter.number` is a
/// float comparator, so the number is returned as `f64`.
fn split_identifier(s: &str) -> Option<(&str, f64)> {
    let (key, num) = s.rsplit_once('-')?;
    let n: f64 = num.trim().parse().ok()?;
    (!key.is_empty()).then_some((key, n))
}

/// Strip Linear's decorative trailing slug so the URL's last path segment is the
/// `identifier` (what `read_refs` recovers on a later status change). The
/// slug-less URL still resolves in a browser.
fn canonical_url(url: &str, identifier: &str) -> String {
    match url.split_once("/issue/") {
        Some((prefix, _)) => format!("{prefix}/issue/{identifier}"),
        None => url.to_string(),
    }
}

/// The Linear workflow-state `type` for a [`Transition`]. Linear states have a
/// `type` ∈ {triage, backlog, unstarted, started, completed, canceled}.
fn target_type(to: Transition) -> &'static str {
    match to {
        Transition::Done => "completed",
        Transition::WontFix => "canceled",
        Transition::Reopen => "unstarted",
    }
}

impl Tracker for Linear {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let team = self.gql(TEAM_ID, json!({ "key": self.team }))?;
        let team_id = team["teams"]["nodes"][0]["id"]
            .as_str()
            .ok_or_else(|| ForgeError::Api {
                status: 0,
                message: format!(
                    "Linear: no team with key `{}`. `forge.tracker_project` must be the Linear \
                     team **key** (its Identifier — the prefix on issue ids like ENG-123, under \
                     Settings → Teams) — NOT a Linear Project or the repo name.",
                    self.team
                ),
            })?
            .to_string();
        let data = self.gql(
            CREATE,
            json!({ "teamId": team_id, "title": title, "description": body }),
        )?;
        let issue = &data["issueCreate"]["issue"];
        let identifier = super::want_str(issue, "identifier", "Linear")?;
        let url = issue["url"]
            .as_str()
            .map(|u| canonical_url(u, &identifier))
            .unwrap_or_else(|| format!("https://linear.app/issue/{identifier}"));
        Ok(IssueRef {
            id: identifier.clone(),
            url,
            title: title.to_string(),
        })
    }

    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError> {
        let node = self.resolve(issue)?;
        let uuid = super::want_str(&node, "id", "Linear")?;
        let wanted = target_type(to);
        let states = node["team"]["states"]["nodes"].as_array();
        let state_id = states.and_then(|ns| {
            ns.iter()
                .find(|s| s["type"].as_str() == Some(wanted))
                // Reopen tolerates a `backlog` state when no `unstarted` exists.
                .or_else(|| {
                    matches!(to, Transition::Reopen)
                        .then(|| ns.iter().find(|s| s["type"].as_str() == Some("backlog")))
                        .flatten()
                })
                .and_then(|s| s["id"].as_str().map(str::to_string))
        });
        match state_id {
            Some(sid) => self
                .gql(UPDATE_STATE, json!({ "id": uuid, "stateId": sid }))
                .map(drop),
            None => {
                eprintln!("adroit: no Linear `{wanted}` state for {issue}; left unchanged");
                Ok(())
            }
        }
    }

    fn close_issue(&self, issue: &str) -> Result<(), ForgeError> {
        self.transition(issue, Transition::Done)
    }

    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError> {
        let node = self.resolve(issue)?;
        let uuid = super::want_str(&node, "id", "Linear")?;
        self.gql(COMMENT, json!({ "id": uuid, "body": body }))
            .map(drop)
    }

    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError> {
        let node = self.resolve(issue)?;
        let ty = node["state"]["type"].as_str().unwrap_or_default();
        let url = node["url"]
            .as_str()
            .map(|u| canonical_url(u, issue))
            .unwrap_or_default();
        Ok(IssueState {
            open: !matches!(ty, "completed" | "canceled"),
            url,
        })
    }

    fn set_due_date(&self, issue: &str, date: Option<&str>) -> Result<(), ForgeError> {
        // Linear issues carry a `dueDate` (a `TimelessDate`, ISO YYYY-MM-DD); the
        // issue's "target date". `null` clears it.
        let node = self.resolve(issue)?;
        let uuid = super::want_str(&node, "id", "Linear")?;
        self.gql(SET_DUE, json!({ "id": uuid, "due": date }))
            .map(drop)
    }

    fn comments_on_issue(&self, issue: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        // Resolve by team-key + number (one call), pulling the issue's comments —
        // ids are UUIDs (strings), which `parse_rest_comments` handles.
        let (key, number) = split_identifier(issue).ok_or_else(|| ForgeError::Api {
            status: 0,
            message: format!("Linear: unrecognized issue identifier `{issue}`"),
        })?;
        let data = self.gql(COMMENTS, json!({ "key": key, "num": number }))?;
        Ok(super::parse_rest_comments(
            &data["issues"]["nodes"][0]["comments"]["nodes"],
            "body",
        ))
    }

    fn update_issue_comment(
        &self,
        _issue: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        // `commentUpdate` takes the comment's own UUID directly.
        self.gql(UPDATE_COMMENT, json!({ "id": comment_id, "body": body }))
            .map(drop)
    }

    fn describe(&self) -> String {
        format!("linear:{}", self.team)
    }
}

// Linear is tracker-only — it implements `Tracker`, not `Forge` (the factory only
// boxes it as `Box<dyn Tracker>`). Giving it a panicking `Forge` impl would be a
// Liskov violation; the `(Option<dyn Forge>, Option<dyn Tracker>)` pair keeps the
// two roles independent.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A transport that replays queued `(status, body)` responses in order and
    /// records the request bodies — GraphQL sends every call to the same URL, so
    /// routing is by call order, not path.
    struct SeqFake {
        responses: Mutex<VecDeque<(u16, String)>>,
        bodies: Mutex<Vec<String>>,
    }
    impl HttpTransport for SeqFake {
        fn request(
            &self,
            _: &str,
            _: &str,
            _: &[(&str, &str)],
            body: Option<&[u8]>,
        ) -> Result<HttpResponse, ForgeError> {
            self.bodies
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(body.unwrap_or_default()).into_owned());
            let (status, b) = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or((404, "{}".into()));
            Ok(HttpResponse {
                status,
                body: b.into_bytes(),
            })
        }
    }

    fn linear(responses: &[(u16, &str)]) -> (Linear, Arc<SeqFake>) {
        let fake = Arc::new(SeqFake {
            responses: Mutex::new(responses.iter().map(|(s, b)| (*s, b.to_string())).collect()),
            bodies: Mutex::new(vec![]),
        });
        (Linear::with_transport("ENG", fake.clone()), fake)
    }

    #[test]
    fn split_identifier_splits_key_and_number() {
        assert_eq!(split_identifier("ENG-123"), Some(("ENG", 123.0)));
        assert_eq!(split_identifier("ABC-1"), Some(("ABC", 1.0)));
        assert_eq!(split_identifier("nope"), None);
        assert_eq!(split_identifier("ENG-x"), None);
    }

    #[test]
    fn canonical_url_strips_the_slug() {
        assert_eq!(
            canonical_url(
                "https://linear.app/acme/issue/ENG-7/adopt-postgres",
                "ENG-7"
            ),
            "https://linear.app/acme/issue/ENG-7"
        );
        // No `/issue/` → left as-is.
        assert_eq!(canonical_url("https://x/y", "ENG-7"), "https://x/y");
    }

    #[test]
    fn create_issue_resolves_team_then_returns_identifier_and_slugless_url() {
        let (l, _f) = linear(&[
            (200, r#"{"data":{"teams":{"nodes":[{"id":"team-uuid"}]}}}"#),
            (
                200,
                r#"{"data":{"issueCreate":{"issue":{"identifier":"ENG-7","url":"https://linear.app/acme/issue/ENG-7/adopt-pg"}}}}"#,
            ),
        ]);
        let issue = l.create_issue("Adopt PG", "desc").unwrap();
        assert_eq!(issue.id, "ENG-7");
        // Slug stripped → trailing segment is the identifier (read_refs recovers it).
        assert_eq!(issue.url, "https://linear.app/acme/issue/ENG-7");
        assert_eq!(issue.url.rsplit('/').next(), Some("ENG-7"));
    }

    #[test]
    fn transition_picks_the_completed_state_and_updates() {
        let (l, f) = linear(&[
            (
                200,
                r#"{"data":{"issues":{"nodes":[{"id":"issue-uuid","url":"u","state":{"type":"started"},"team":{"states":{"nodes":[{"id":"s-todo","type":"unstarted"},{"id":"s-done","type":"completed"},{"id":"s-cancel","type":"canceled"}]}}}]}}}"#,
            ),
            (200, r#"{"data":{"issueUpdate":{"success":true}}}"#),
        ]);
        l.transition("ENG-7", Transition::Done).unwrap();
        // The second request (the mutation) carries the completed state's id.
        let bodies = f.bodies.lock().unwrap();
        assert!(bodies[1].contains("s-done"), "got: {}", bodies[1]);
    }

    #[test]
    fn transition_without_a_matching_state_is_a_no_op() {
        let (l, _f) = linear(&[(
            200,
            r#"{"data":{"issues":{"nodes":[{"id":"issue-uuid","state":{"type":"started"},"team":{"states":{"nodes":[{"id":"s-todo","type":"unstarted"}]}}}]}}}"#,
        )]);
        // No `completed` state → left unchanged, no error.
        assert!(l.transition("ENG-7", Transition::Done).is_ok());
    }

    #[test]
    fn issue_state_maps_state_type_to_open() {
        let (l, _f) = linear(&[(
            200,
            r#"{"data":{"issues":{"nodes":[{"id":"u","url":"https://linear.app/acme/issue/ENG-7/x","state":{"type":"completed"}}]}}}"#,
        )]);
        let st = l.issue_state("ENG-7").unwrap();
        assert!(!st.open);
        assert_eq!(st.url, "https://linear.app/acme/issue/ENG-7");
    }

    #[test]
    fn graphql_error_array_becomes_an_api_error() {
        let (l, _f) = linear(&[(200, r#"{"errors":[{"message":"Bad team"}]}"#)]);
        let err = l.create_issue("t", "b").unwrap_err();
        assert!(matches!(err, ForgeError::Api { .. }));
    }

    #[test]
    fn missing_issue_is_a_clean_error_not_a_panic() {
        let (l, _f) = linear(&[(200, r#"{"data":{"issues":{"nodes":[]}}}"#)]);
        assert!(l.issue_state("ENG-9").is_err());
    }

    #[test]
    fn set_due_date_resolves_then_updates_with_the_date() {
        let (l, f) = linear(&[
            (
                200,
                r#"{"data":{"issues":{"nodes":[{"id":"issue-uuid"}]}}}"#,
            ),
            (200, r#"{"data":{"issueUpdate":{"success":true}}}"#),
        ]);
        l.set_due_date("ENG-7", Some("2026-06-20")).unwrap();
        let bodies = f.bodies.lock().unwrap();
        // The mutation carries the resolved UUID and the date.
        assert!(bodies[1].contains("issue-uuid"), "got: {}", bodies[1]);
        assert!(bodies[1].contains("2026-06-20"), "got: {}", bodies[1]);
    }

    #[test]
    fn upsert_issue_comment_edits_the_marked_comment_by_uuid() {
        let marker = "<!-- adroit:review-kickoff -->";
        // 1st call: list comments (one carries the marker, id c1). 2nd: commentUpdate.
        let listing = format!(
            r#"{{"data":{{"issues":{{"nodes":[{{"comments":{{"nodes":[{{"id":"c1","body":"old\n\n{marker}"}}]}}}}]}}}}}}"#
        );
        let (l, f) = linear(&[
            (200, listing.as_str()),
            (200, r#"{"data":{"commentUpdate":{"success":true}}}"#),
        ]);
        l.upsert_issue_comment("ENG-7", marker, "new body").unwrap();
        let bodies = f.bodies.lock().unwrap();
        // The 2nd call is `commentUpdate` carrying the comment's own UUID.
        assert!(bodies[1].contains("c1"), "got: {}", bodies[1]);
        assert!(bodies[1].contains("commentUpdate"), "got: {}", bodies[1]);
    }
}
