//! monday.com adapter — a **split** [`Tracker`] (monday has no PRs, so it
//! implements no [`Forge`](super::Forge), like [`jira`](super::jira) and
//! [`linear`](super::linear)). Pairs with a GitHub/GitLab forge when
//! `forge.tracker = monday`.
//!
//! **GraphQL, not REST.** monday exposes a single endpoint
//! (`POST https://api.monday.com/v2`) — reused through [`super::rest_call`] with a
//! GraphQL `errors`-array check (a query error returns HTTP 200). The account's
//! default API version is used (no version header pinned); the queries here are
//! stable across versions. **Auth:** an API token in the `Authorization` header
//! **verbatim** (no `Bearer`) — env `ADROIT_MONDAY_TOKEN`, or `adroit auth monday`.
//!
//! **Model.** monday is a work-OS: a **board** holds **items** (rows), each with
//! column values. An ADR's "issue" is an item; its lifecycle is a **Status**
//! column of board-defined *labels*. So transitions are a keyword→label match
//! (like Jira's transition-by-name lookup) over the board's first status column —
//! best-effort, warn-and-leave when nothing matches. **Config:**
//! `forge.tracker_project` = the numeric **board id**; `forge.tracker_host` = the
//! account **subdomain** (`acme` → `acme.monday.com`), used to build the item URL
//! (`…/boards/<board>/pulses/<item>`), whose trailing segment is the item id that
//! `read_refs` recovers on a later status change.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    ForgeComment, ForgeError, HttpTransport, IssueRef, IssueState, Tracker, Transition,
    UreqTransport,
};

const API: &str = "https://api.monday.com/v2";

/// A monday GraphQL client scoped to one board.
#[derive(Clone)]
pub struct Monday {
    host: String,  // account subdomain → <host>.monday.com
    board: String, // numeric board id
    token: String, // API token — the Authorization header value, verbatim
    transport: Arc<dyn HttpTransport>,
}

impl Monday {
    pub fn new(host: String, board: String, token: &str) -> Self {
        Self {
            host,
            board,
            token: token.to_string(),
            transport: Arc::new(UreqTransport),
        }
    }

    /// Build a client over an injected transport (forge fault-injection + unit
    /// tests), mirroring the other adapters' `with_transport`.
    pub fn with_transport(host: &str, board: &str, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            host: host.to_string(),
            board: board.to_string(),
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
            "monday",
            message_of,
        )?;
        if let Some(errs) = v.get("errors").and_then(Value::as_array)
            && !errs.is_empty()
        {
            return Err(ForgeError::Api {
                status: 200,
                message: errs[0]["message"]
                    .as_str()
                    .unwrap_or("monday GraphQL error")
                    .to_string(),
            });
        }
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    }

    fn item_url(&self, item: &str) -> String {
        format!(
            "https://{}.monday.com/boards/{}/pulses/{item}",
            self.host, self.board
        )
    }
}

const CREATE_ITEM: &str = "\
mutation($board: ID!, $name: String!) {
  create_item(board_id: $board, item_name: $name) { id }
}";

const CREATE_UPDATE: &str = "\
mutation($item: ID!, $body: String!) { create_update(item_id: $item, body: $body) { id } }";

const BOARD_COLUMNS: &str =
    "query($ids: [ID!]) { boards(ids: $ids) { columns { id type settings_str } } }";

const SET_STATUS: &str = "\
mutation($board: ID!, $item: ID!, $col: String!, $val: String!) {
  change_simple_column_value(board_id: $board, item_id: $item, column_id: $col, value: $val) { id }
}";

const ITEM_STATUS: &str =
    "query($ids: [ID!]) { items(ids: $ids) { column_values { id type text } } }";

const ITEM_UPDATES: &str = "query($ids: [ID!]) { items(ids: $ids) { updates { id body } } }";

/// Construct a monday tracker, or `None` if inactive (missing `tracker_project`,
/// `tracker_host`, or `ADROIT_MONDAY_TOKEN`). `tracker_project` = board id,
/// `tracker_host` = account subdomain.
pub fn open(cfg: &crate::config::ForgeConfig) -> Option<Box<dyn Tracker>> {
    let board = cfg.tracker_project.clone()?;
    let host = cfg.tracker_host.clone()?;
    let token = std::env::var("ADROIT_MONDAY_TOKEN")
        .ok()
        .or_else(|| crate::config::load_credential("monday"))?;
    Some(Box::new(Monday::new(host, board, &token)))
}

fn message_of(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["errors"][0]["message"]
                .as_str()
                .map(str::to_string)
                .or_else(|| v["error_message"].as_str().map(str::to_string))
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string())
}

/// monday status columns report `type` as `status` (or legacy `color`).
fn is_status_col(col: &Value) -> bool {
    matches!(col["type"].as_str(), Some("status") | Some("color"))
}

/// Keywords that map a [`Transition`] onto a board-defined status label.
fn keywords(to: Transition) -> &'static [&'static str] {
    match to {
        Transition::Done => &[
            "done", "complete", "resolved", "closed", "finished", "accepted",
        ],
        Transition::WontFix => &[
            "won't", "wont", "reject", "cancel", "decline", "stuck", "blocked",
        ],
        Transition::Reopen => &[
            "working",
            "progress",
            "to-do",
            "todo",
            "not started",
            "open",
            "reopen",
            "new",
        ],
    }
}

/// Pick `(column_id, label)` for `to`: the first status column carrying a label
/// whose text matches one of `to`'s keywords. `None` ⇒ leave the item unchanged.
fn pick_status_label(columns: &Value, to: Transition) -> Option<(String, String)> {
    let words = keywords(to);
    columns
        .as_array()?
        .iter()
        .filter(|c| is_status_col(c))
        .find_map(|c| {
            let id = c["id"].as_str()?;
            let settings = c["settings_str"].as_str()?;
            let parsed: Value = serde_json::from_str(settings).ok()?;
            let label = parsed["labels"].as_object()?.values().find_map(|v| {
                let text = v.as_str()?;
                let lower = text.to_ascii_lowercase();
                words
                    .iter()
                    .any(|w| lower.contains(w))
                    .then(|| text.to_string())
            })?;
            Some((id.to_string(), label))
        })
}

/// The id of the board's first **date** column (monday's native due-date field).
fn first_date_col(columns: &Value) -> Option<String> {
    columns
        .as_array()?
        .iter()
        .find(|c| c["type"].as_str() == Some("date"))
        .and_then(|c| c["id"].as_str().map(str::to_string))
}

/// Whether a status label's text reads as a closed (done- or wontfix-) state.
fn label_is_closed(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    keywords(Transition::Done)
        .iter()
        .chain(keywords(Transition::WontFix))
        .any(|w| lower.contains(w))
}

impl Tracker for Monday {
    fn create_issue(&self, title: &str, body: &str) -> Result<IssueRef, ForgeError> {
        let data = self.gql(CREATE_ITEM, json!({ "board": self.board, "name": title }))?;
        let id = super::want_str(&data["create_item"], "id", "monday")?;
        // monday items have no description field; the body becomes the first
        // update (best-effort — the item URL is the durable record).
        if !body.trim().is_empty()
            && let Err(e) = self.gql(CREATE_UPDATE, json!({ "item": id, "body": body }))
        {
            eprintln!("adroit: monday: created item {id} but couldn't post the body: {e}");
        }
        Ok(IssueRef {
            url: self.item_url(&id),
            id,
            title: title.to_string(),
        })
    }

    fn transition(&self, issue: &str, to: Transition) -> Result<(), ForgeError> {
        let data = self.gql(BOARD_COLUMNS, json!({ "ids": [self.board] }))?;
        match pick_status_label(&data["boards"][0]["columns"], to) {
            Some((col, label)) => self
                .gql(
                    SET_STATUS,
                    json!({ "board": self.board, "item": issue, "col": col, "val": label }),
                )
                .map(drop),
            None => {
                eprintln!("adroit: no matching monday status label for {issue}; left unchanged");
                Ok(())
            }
        }
    }

    fn close_issue(&self, issue: &str) -> Result<(), ForgeError> {
        self.transition(issue, Transition::Done)
    }

    fn comment_issue(&self, issue: &str, body: &str) -> Result<(), ForgeError> {
        self.gql(CREATE_UPDATE, json!({ "item": issue, "body": body }))
            .map(drop)
    }

    fn issue_state(&self, issue: &str) -> Result<IssueState, ForgeError> {
        let data = self.gql(ITEM_STATUS, json!({ "ids": [issue] }))?;
        let label = data["items"][0]["column_values"]
            .as_array()
            .and_then(|cvs| cvs.iter().find(|cv| is_status_col(cv)))
            .and_then(|cv| cv["text"].as_str());
        // No status column / no value ⇒ assume still open.
        let open = label.map(|t| !label_is_closed(t)).unwrap_or(true);
        Ok(IssueState {
            open,
            url: self.item_url(issue),
        })
    }

    fn comments_on_issue(&self, issue: &str) -> Result<Vec<ForgeComment>, ForgeError> {
        // An item's "comments" are its updates. monday has no edit-update mutation,
        // so `update_issue_comment` stays the default no-op: the upsert still finds
        // adroit's tagged update and avoids posting a duplicate (it just can't
        // refresh a changed body in place — best-effort, unlike GitHub/GitLab).
        let data = self.gql(ITEM_UPDATES, json!({ "ids": [issue] }))?;
        Ok(super::parse_rest_comments(
            &data["items"][0]["updates"],
            "body",
        ))
    }

    fn set_due_date(&self, issue: &str, date: Option<&str>) -> Result<(), ForgeError> {
        // monday has no fixed due-date field; set the board's first `date` column
        // (the simple value is `YYYY-MM-DD`; an empty string clears it).
        let data = self.gql(BOARD_COLUMNS, json!({ "ids": [self.board] }))?;
        match first_date_col(&data["boards"][0]["columns"]) {
            Some(col) => self
                .gql(
                    SET_STATUS,
                    json!({ "board": self.board, "item": issue, "col": col, "val": date.unwrap_or_default() }),
                )
                .map(drop),
            None => {
                eprintln!(
                    "adroit: no monday date column on board {}; left unchanged",
                    self.board
                );
                Ok(())
            }
        }
    }

    fn describe(&self) -> String {
        format!("monday:{}", self.board)
    }
}

// monday is tracker-only — `Tracker`, not `Forge` (see the note in `linear.rs`).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::collections::VecDeque;
    use std::sync::Mutex;

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

    fn monday(responses: Vec<(u16, String)>) -> (Monday, Arc<SeqFake>) {
        let fake = Arc::new(SeqFake {
            responses: Mutex::new(responses.into_iter().collect()),
            bodies: Mutex::new(vec![]),
        });
        (Monday::with_transport("acme", "123", fake.clone()), fake)
    }

    fn columns_response() -> String {
        let settings =
            json!({ "labels": { "0": "To-Do", "1": "Working on it", "2": "Done", "5": "Stuck" } })
                .to_string();
        json!({ "data": { "boards": [ { "columns": [
            { "id": "name", "type": "text", "settings_str": "{}" },
            { "id": "status", "type": "status", "settings_str": settings }
        ] } ] } })
        .to_string()
    }

    #[test]
    fn create_issue_returns_item_id_and_pulse_url() {
        let (m, _f) = monday(vec![
            (200, r#"{"data":{"create_item":{"id":"42"}}}"#.into()),
            (200, r#"{"data":{"create_update":{"id":"u1"}}}"#.into()),
        ]);
        let issue = m.create_issue("Adopt PG", "desc").unwrap();
        assert_eq!(issue.id, "42");
        assert_eq!(issue.url, "https://acme.monday.com/boards/123/pulses/42");
        assert_eq!(issue.url.rsplit('/').next(), Some("42"));
    }

    #[test]
    fn transition_done_sets_the_matching_status_label() {
        let (m, f) = monday(vec![
            (200, columns_response()),
            (
                200,
                r#"{"data":{"change_simple_column_value":{"id":"42"}}}"#.into(),
            ),
        ]);
        m.transition("42", Transition::Done).unwrap();
        let bodies = f.bodies.lock().unwrap();
        // The mutation targets the status column and the "Done" label.
        assert!(bodies[1].contains("status"), "got: {}", bodies[1]);
        assert!(bodies[1].contains("Done"), "got: {}", bodies[1]);
    }

    #[test]
    fn transition_without_a_matching_label_is_a_no_op() {
        let only_todo = json!({ "data": { "boards": [ { "columns": [
                { "id": "status", "type": "status",
                  "settings_str": json!({"labels":{"0":"To-Do"}}).to_string() }
            ] } ] } })
        .to_string();
        let (m, _f) = monday(vec![(200, only_todo)]);
        assert!(m.transition("42", Transition::Done).is_ok());
    }

    #[test]
    fn issue_state_reads_the_status_label() {
        let done = json!({ "data": { "items": [ { "column_values": [
            { "id": "status", "type": "status", "text": "Done" }
        ] } ] } })
        .to_string();
        let (m, _f) = monday(vec![(200, done)]);
        let st = m.issue_state("42").unwrap();
        assert!(!st.open);
        assert_eq!(st.url, "https://acme.monday.com/boards/123/pulses/42");
    }

    #[test]
    fn graphql_error_array_becomes_an_api_error() {
        let (m, _f) = monday(vec![(
            200,
            r#"{"errors":[{"message":"Bad board"}]}"#.into(),
        )]);
        assert!(matches!(
            m.create_issue("t", "b").unwrap_err(),
            ForgeError::Api { .. }
        ));
    }

    #[test]
    fn set_due_date_sets_the_first_date_column() {
        let cols = json!({ "data": { "boards": [ { "columns": [
            { "id": "status", "type": "status", "settings_str": "{}" },
            { "id": "date4", "type": "date", "settings_str": "{}" }
        ] } ] } })
        .to_string();
        let (m, f) = monday(vec![
            (200, cols),
            (
                200,
                r#"{"data":{"change_simple_column_value":{"id":"42"}}}"#.into(),
            ),
        ]);
        m.set_due_date("42", Some("2026-06-20")).unwrap();
        let bodies = f.bodies.lock().unwrap();
        assert!(bodies[1].contains("date4"), "got: {}", bodies[1]);
        assert!(bodies[1].contains("2026-06-20"), "got: {}", bodies[1]);
    }

    #[test]
    fn upsert_issue_comment_creates_an_update_when_none_is_marked() {
        let marker = "<!-- adroit:review-deadline -->";
        let (m, f) = monday(vec![
            (200, r#"{"data":{"items":[{"updates":[]}]}}"#.into()), // list: none
            (200, r#"{"data":{"create_update":{"id":"u1"}}}"#.into()), // create
        ]);
        m.upsert_issue_comment("42", marker, "deadline").unwrap();
        let bodies = f.bodies.lock().unwrap();
        assert!(bodies[1].contains("create_update"), "got: {}", bodies[1]);
    }

    #[test]
    fn upsert_issue_comment_does_not_duplicate_an_existing_marked_update() {
        let marker = "<!-- adroit:review-deadline -->";
        // monday has no edit-update mutation, so a changed body is a no-op — but
        // crucially it must NOT post a duplicate. Only the list call should run.
        let listing = format!(
            r#"{{"data":{{"items":[{{"updates":[{{"id":"u9","body":"old\n\n{marker}"}}]}}]}}}}"#
        );
        let (m, f) = monday(vec![(200, listing)]);
        assert!(m.upsert_issue_comment("42", marker, "new deadline").is_ok());
        assert_eq!(
            f.bodies.lock().unwrap().len(),
            1,
            "must not create a duplicate update"
        );
    }
}
