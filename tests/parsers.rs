//! Parser property tests for the adroit hardening blitz (stable proptest, no
//! nightly fuzzer).
//!
//! Two kinds of property:
//!  - **No panic** — every pure parse/serialize/rewrite helper must tolerate
//!    arbitrary input (including control chars, newlines, and multibyte unicode
//!    at byte-prefix boundaries) without panicking.
//!  - **Algebraic laws** — `rewrite_status` / `rewrite_review_by(.., None)` /
//!    `upsert_reference` / `links::rewrite_links` are idempotent, and a parsed
//!    markdown ADR's body round-trips (parse → body → re-parse is a fixpoint).
//!
//! Spec: docs/superpowers/specs/2026-06-04-adroit-hardening-blitz-design.md

use std::path::{Path, PathBuf};

use adroit::adr::Status;
use adroit::format;
use adroit::links;
use adroit::naming::{AdrRef, NamingScheme};

use proptest::prelude::*;

const SCHEMES: [NamingScheme; 4] = [
    NamingScheme::Sequential,
    NamingScheme::Date,
    NamingScheme::Uuid,
    NamingScheme::PerCategory,
];

const STATUSES: [Status; 5] = [
    Status::Proposed,
    Status::Accepted,
    Status::Rejected,
    Status::Deprecated,
    Status::Superseded,
];

// ---------------------------------------------------------------------------
// Strategies — biased toward structurally-relevant and boundary-stressing chars
// ---------------------------------------------------------------------------

/// A single character: arbitrary unicode, with extra weight on ASCII control,
/// markdown punctuation, and a multibyte char (to stress byte-boundary slicing).
fn arb_char() -> impl Strategy<Value = char> {
    prop_oneof![
        6 => any::<char>(),
        3 => prop::char::range('\u{0}', '\u{7f}'),
        1 => prop::char::range('\u{80}', '\u{2fff}'),
        1 => Just('\n'),
        1 => Just('#'),
        1 => Just(':'),
        1 => Just('['),
        1 => Just(']'),
        1 => Just('('),
        1 => Just(')'),
        1 => Just('>'),
        1 => Just('—'), // em-dash: 3 bytes, classic prefix-boundary hazard
    ]
}

/// An arbitrary text blob up to ~140 chars.
fn arb_text() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_char(), 0..140).prop_map(|v| v.into_iter().collect())
}

/// A short token usable as a link label / reference / url-ish string.
fn arb_token() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_char(), 0..24).prop_map(|v| v.into_iter().collect())
}

/// A markdown-ish document sprinkled with `[label](target)` links whose targets
/// are a mix of relative `.md`, `../`, absolute, anchor, and URL forms — to
/// actually exercise the link scanner/rewriter rather than no-op on linkless text.
fn arb_link_doc() -> impl Strategy<Value = String> {
    let target = prop_oneof![
        Just("0001-a.md".to_string()),
        Just("../accepted/0002-b.md".to_string()),
        Just("./0003-c.md".to_string()),
        Just("0009-dup.md#anchor".to_string()),
        Just("https://example.com/x.md".to_string()),
        Just("#section".to_string()),
        Just("/abs/0004-d.md".to_string()),
        arb_token(),
    ];
    let link = target.prop_map(|t| format!("[label]({t})"));
    prop::collection::vec(
        prop_oneof![link, arb_token().prop_map(|t| format!("{t}\n"))],
        0..12,
    )
    .prop_map(|parts| parts.join(" "))
}

/// LF-only arbitrary text. The rewrite/upsert idempotence laws assume a
/// consistent newline convention — adroit only ever writes `\n` (or preserves an
/// existing `\r\n`), both of which are idempotent. A *lone* `\r` defeats the
/// helpers' newline detection (it fuses with a joined `\n` into `\r\n` on the
/// next pass) and is a **known, deferred** robustness gap, recorded in
/// docs/superpowers/hardening-blitz-worklog.md — not exercised by these laws.
fn arb_lf_text() -> impl Strategy<Value = String> {
    arb_text().prop_map(|s| s.replace('\r', ""))
}

fn arb_ref() -> impl Strategy<Value = AdrRef> {
    prop_oneof![
        any::<u32>().prop_map(AdrRef::Number),
        arb_token().prop_map(AdrRef::Slug),
    ]
}

/// A resolver mapping any link target carrying a number to a fixed canonical
/// path, used to drive `rewrite_links` idempotence.
fn fixed_resolver(target: &str) -> Option<PathBuf> {
    links::number_in_target(target).map(|n| PathBuf::from(format!("/r/accepted/{n:04}-x.md")))
}

// ---------------------------------------------------------------------------
// No-panic: the markdown format helpers tolerate any input
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::default())]

    #[test]
    fn format_helpers_never_panic(text in arb_text(), token in arb_token()) {
        for scheme in SCHEMES {
            let _ = format::parse_markdown(&text, None, scheme);
            for st in STATUSES {
                let _ = format::parse_markdown(&text, Some(st), scheme);
            }
            let _ = format::parse_markdown_section_supersession(&text, scheme);
        }
        let _ = format::parse_markdown_section_status(&text);
        let _ = format::parse_references(&text);
        for st in STATUSES {
            let _ = format::rewrite_status(&text, st, None);
            let _ = format::rewrite_status(&text, st, Some((&token, &token)));
        }
        let _ = format::rewrite_review_by(&text, None);
        let _ = format::upsert_reference(&text, &token, &token);
        let _ = adroit::frontmatter::deserialize(&text);
    }

    #[test]
    fn link_helpers_never_panic(text in arb_text(), doc in arb_link_doc(), token in arb_token()) {
        for input in [&text, &doc] {
            let _ = links::rewrite_links(input, Path::new("/r/proposed"), fixed_resolver);
            let _ = links::relative_md_targets(input);
            let _ = links::relabel_links_to(input, "0001-a.md", "0007-a.md", "ADR-0001", "ADR-0007");
        }
        let _ = links::number_in_target(&token);
        let _ = links::is_relative_md(&token);
        let _ = links::rel_link(Path::new(&token), Path::new(&token));
    }

    #[test]
    fn naming_helpers_never_panic(text in arb_token(), r in arb_ref()) {
        for scheme in SCHEMES {
            let _ = scheme.parse_ref(&text);
            let _ = scheme.ref_in_link(&text);
            let _ = scheme.ref_in_note(&text);
            let _ = scheme.parse(Path::new(&text), &text);
            // Identity renderers must tolerate any stored ref (e.g. a crafted
            // slug under the uuid scheme, whose display slices the first bytes).
            let _ = scheme.filename(&r, &text);
            let _ = scheme.display(&r);
            let _ = scheme.heading(&r, &text);
            let _ = scheme.link_label(&r);
            let _ = scheme.ref_matches(&r, &r);
        }
    }

    #[test]
    fn config_parse_remote_url_never_panics(text in arb_text()) {
        let _ = adroit::config::parse_remote_url(&text);
    }
}

// ---------------------------------------------------------------------------
// Algebraic laws
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::default())]

    /// Rewriting a document's status to S, then to S again, is a no-op the second
    /// time (the rewrite is idempotent).
    #[test]
    fn rewrite_status_is_idempotent(text in arb_lf_text(), idx in 0usize..5) {
        let status = STATUSES[idx];
        let once = format::rewrite_status(&text, status, None);
        let twice = format::rewrite_status(&once, status, None);
        prop_assert_eq!(&once, &twice, "rewrite_status not idempotent");
    }

    /// Removing the `Review by:` line twice is the same as removing it once.
    #[test]
    fn rewrite_review_by_clear_is_idempotent(text in arb_lf_text()) {
        let once = format::rewrite_review_by(&text, None);
        let twice = format::rewrite_review_by(&once, None);
        prop_assert_eq!(&once, &twice, "rewrite_review_by(None) not idempotent");
    }

    /// Upserting the same `label: url` reference twice is byte-identical to once.
    #[test]
    fn upsert_reference_is_idempotent(
        text in arb_lf_text(),
        // A real bullet label has no newline or `:` and is non-empty; a url has no
        // newline. Generate valid ones directly rather than filtering.
        label in "[A-Za-z0-9._-][A-Za-z0-9 ._-]{0,14}",
        url in "[A-Za-z0-9:/._?=&#-]{0,30}",
    ) {
        let once = format::upsert_reference(&text, &label, &url);
        let twice = format::upsert_reference(&once, &label, &url);
        prop_assert_eq!(&once, &twice, "upsert_reference not idempotent");
    }

    /// `rewrite_links` reaches a fixpoint: after one canonicalizing pass, a second
    /// pass rewrites nothing.
    #[test]
    fn rewrite_links_is_idempotent(doc in arb_link_doc()) {
        let dir = Path::new("/r/proposed");
        let (once, _) = links::rewrite_links(&doc, dir, fixed_resolver);
        let (twice, changed) = links::rewrite_links(&once, dir, fixed_resolver);
        prop_assert_eq!(changed, 0, "rewrite_links second pass changed {} links", changed);
        prop_assert_eq!(once, twice, "rewrite_links not a fixpoint");
    }

    /// A parsed markdown ADR's body is itself a valid document that re-parses to
    /// the same title and status (parse → body → re-parse is a fixpoint).
    #[test]
    fn parse_markdown_body_round_trips(text in arb_text()) {
        for scheme in SCHEMES {
            if let Ok(a) = format::parse_markdown(&text, None, scheme) {
                let b = format::parse_markdown(&a.body, None, scheme)
                    .expect("a parsed ADR's body must re-parse");
                prop_assert_eq!(&a.title, &b.title, "title not stable across body round-trip");
                prop_assert_eq!(a.status, b.status, "status not stable across body round-trip");
            }
        }
    }
}
