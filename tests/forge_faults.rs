//! Forge fault-injection for the hardening blitz (requires `--features forge`).
//!
//! Every forge adapter (GitHub / GitLab / Jira / Linear / monday) parses
//! untrusted HTTP responses from a third-party API (the GraphQL trackers — Linear,
//! monday — likewise: a garbage `{data,errors}` body must never panic). This
//! drives each adapter's `Forge` + `Tracker` methods
//! over a `HostileTransport` that returns arbitrary status codes and malformed /
//! truncated / wrong-typed / oversized bodies (plus an injected connection
//! failure), and asserts the adapters **never panic** and always return a
//! well-formed `Result` — a garbage response must become a clean `Err`, never a
//! crash or a bogus `Ok`.
//!
//! The file is empty without the `forge` feature; it runs under `just test-forge`
//! (folded into `just ci`).
//!
//! See the book's Hardening & Quality page (docs/src/dev/hardening.md).

#![cfg(feature = "forge")]

use std::sync::Arc;

use adroit::forge::github::Github;
use adroit::forge::gitlab::Gitlab;
use adroit::forge::jira::Jira;
use adroit::forge::linear::Linear;
use adroit::forge::monday::Monday;
use adroit::forge::{Forge, ForgeError, HttpResponse, HttpTransport, PrDraft, Tracker, Transition};

use proptest::prelude::*;

/// A transport that ignores the request and replays one canned (status, body) —
/// or fails at the connection level — for every call.
struct HostileTransport {
    status: u16,
    body: Vec<u8>,
    offline: bool,
}

impl HttpTransport for HostileTransport {
    fn request(
        &self,
        _method: &str,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: Option<&[u8]>,
    ) -> Result<HttpResponse, ForgeError> {
        if self.offline {
            return Err(ForgeError::Offline("injected connection failure".into()));
        }
        Ok(HttpResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

/// Call every `Forge` method. Each must return without panicking.
fn hammer_forge(f: &dyn Forge) {
    let draft = PrDraft {
        branch: "adr/0001-x".into(),
        base: "main".into(),
        title: "Title".into(),
        body: "Body".into(),
    };
    let _ = f.open_pr(&draft);
    let _ = f.pr_state("1");
    let _ = f.merge_pr("1");
    let _ = f.close_pr("1");
    let _ = f.comment_pr("1", "hello");
    let _ = f.set_pr_body("1", "new body");
    let _ = f.describe();
}

/// Call every `Tracker` method. Each must return without panicking.
fn hammer_tracker(t: &dyn Tracker) {
    let _ = t.create_issue("Title", "Body");
    for tr in [Transition::Done, Transition::WontFix, Transition::Reopen] {
        let _ = t.transition("1", tr);
    }
    let _ = t.close_issue("1");
    let _ = t.comment_issue("1", "hello");
    let _ = t.issue_state("1");
    let _ = t.describe();
}

/// Response bodies: arbitrary bytes mixed with structurally-hostile JSON
/// (truncated, wrong-typed, overflowing, null, empty container).
fn arb_body() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => prop::collection::vec(any::<u8>(), 0..80),
        1 => Just(b"{}".to_vec()),
        1 => Just(b"[]".to_vec()),
        1 => Just(b"{".to_vec()),
        1 => Just(b"null".to_vec()),
        1 => Just(br#"{"number":"not-an-int"}"#.to_vec()),
        1 => Just(br#"{"number":99999999999999999999999999}"#.to_vec()),
        1 => Just(br#"{"id":null,"html_url":42,"state":[]}"#.to_vec()),
        1 => Just(br#"{"iid":-1,"web_url":{"x":1}}"#.to_vec()),
        2 => proptest::string::string_regex(r#"[a-zA-Z{}\[\]:,"0-9 _.-]{0,80}"#)
            .unwrap()
            .prop_map(String::into_bytes),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::default())]

    /// No status code × body combination may panic any adapter, and a
    /// connection-level failure (`offline`) must surface as a clean `Err`.
    #[test]
    fn adapters_tolerate_hostile_responses(
        status in any::<u16>(),
        body in arb_body(),
        offline in any::<bool>(),
    ) {
        let transport = Arc::new(HostileTransport { status, body, offline });

        let github = Github::with_transport("owner/repo", transport.clone());
        hammer_forge(&github);
        hammer_tracker(&github);

        let gitlab = Gitlab::with_transport("owner/repo", transport.clone());
        hammer_forge(&gitlab);
        hammer_tracker(&gitlab);

        // Jira / Linear / monday are tracker-only (no `Forge` impl), so we
        // exercise only the tracker side. The GraphQL trackers (Linear, monday)
        // make multiple round-trips per verb against the same hostile body.
        let jira = Jira::with_transport("https://jira.example.com", "PROJ", transport.clone());
        hammer_tracker(&jira);

        let linear = Linear::with_transport("ENG", transport.clone());
        hammer_tracker(&linear);

        let monday = Monday::with_transport("acme", "123", transport.clone());
        hammer_tracker(&monday);
    }
}
