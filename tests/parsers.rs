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
//! See the book's Hardening & Quality page (docs/src/dev/hardening.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use adroit::adr::Status;
use adroit::format;
use adroit::import::{self, SEED_MARKER, parse_assessment, seed_drafts, seed_fragment};
use adroit::links;
use adroit::naming::{AdrRef, NamingScheme};
use adroit::publish;

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

fn arb_ref() -> impl Strategy<Value = AdrRef> {
    prop_oneof![
        any::<u32>().prop_map(AdrRef::Number),
        arb_token().prop_map(AdrRef::Slug),
    ]
}

// An arbitrary `assessments`-export model, built directly (pub fields) with
// adversarial unicode in every text field, to stress the seed mapping.
fn arb_question() -> impl Strategy<Value = import::Question> {
    (arb_text(), prop::option::of(arb_token()))
        .prop_map(|(text, polarity)| import::Question { text, polarity })
}

fn arb_practice() -> impl Strategy<Value = import::Practice> {
    (
        arb_text(),
        arb_text(),
        arb_text(),
        arb_text(),
        prop::collection::vec(arb_question(), 0..3),
        prop::option::of(arb_token()),
    )
        .prop_map(
            |(name, context, value, risk, questions, effort)| import::Practice {
                name,
                context,
                value,
                risk,
                questions,
                effort,
            },
        )
}

fn arb_domain() -> impl Strategy<Value = import::Domain> {
    (
        arb_text(),
        arb_text(),
        arb_text(),
        arb_text(),
        prop::collection::vec(arb_practice(), 0..3),
    )
        .prop_map(|(name, context, value, risk, practices)| import::Domain {
            name,
            context,
            value,
            risk,
            practices,
        })
}

fn arb_assessment() -> impl Strategy<Value = import::Assessment> {
    (arb_text(), prop::collection::vec(arb_domain(), 0..3)).prop_map(|(name, domains)| {
        import::Assessment {
            name,
            description: String::new(),
            goal: String::new(),
            domains,
        }
    })
}

/// A resolver mapping any link target carrying a number to a fixed canonical
/// path, used to drive `rewrite_links` idempotence.
fn fixed_resolver(target: &str) -> Option<PathBuf> {
    links::number_in_target(target).map(|n| PathBuf::from(format!("/r/accepted/{n:04}-x.md")))
}

/// A shared MCP server (built once from the manifest over a throwaway dir) for the
/// `handle_line` no-panic property — rebuilding per case would churn the clap walk.
#[cfg(feature = "mcp")]
fn mcp_server() -> &'static adroit::mcp::Server {
    use std::sync::OnceLock;
    static SERVER: OnceLock<adroit::mcp::Server> = OnceLock::new();
    SERVER.get_or_init(|| {
        adroit::mcp::Server::new(&adroit::config::Config::default(), &std::env::temp_dir())
    })
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
            let _ = format::parse_markdown_section_supersession(&text, scheme, None);
            let _ = format::parse_markdown_section_supersession(&text, scheme, Some("cat"));
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

    /// The publish cross-link rewriter tolerates arbitrary, adversarial input
    /// (multibyte, lone `\r`, nested/adjacent brackets) for any scheme + published
    /// set — the byte-index scan must never slice a backwards or non-boundary range.
    #[test]
    fn publish_rewriter_never_panics(
        doc in arb_link_doc(),
        text in arb_text(),
        r1 in arb_ref(),
        r2 in arb_ref(),
        page in arb_token(),
    ) {
        let published: HashMap<AdrRef, PathBuf> = [
            (r1, PathBuf::from(format!("{page}.md"))),
            (r2, PathBuf::from("docs/sub/0001-x.md")),
        ]
        .into_iter()
        .collect();
        for input in [&doc, &text] {
            for scheme in SCHEMES {
                for cat in [None, Some("data")] {
                    let _ = publish::rewrite_published_links(
                        input,
                        Path::new("a/page.md"),
                        &scheme,
                        &published,
                        cat,
                    );
                }
            }
        }
    }

    /// `strip_h1` tolerates any input and only ever drops lines (never adds).
    #[test]
    fn publish_strip_h1_never_panics_and_only_drops_lines(text in arb_text()) {
        let out = publish::strip_h1(&text);
        prop_assert!(out.lines().count() <= text.lines().count());
    }

    /// The MCP JSON-RPC line handler tolerates arbitrary stdin (hostile JSON,
    /// multibyte, lone `\r`) — a malformed request yields an error response, never
    /// a panic.
    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_handle_line_never_panics(line in arb_text()) {
        let _ = adroit::mcp::handle_line(mcp_server(), &line);
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

    /// The assessment parser tolerates arbitrary input (JSON and YAML extensions),
    /// and the seed mapping never panics on whatever parses.
    #[test]
    fn import_parser_and_mapping_never_panic(text in arb_text()) {
        for ext in ["a.json", "a.yaml", "a.toml"] {
            if let Ok(a) = parse_assessment(&text, Path::new(ext)) {
                for d in seed_drafts(&a) {
                    let _ = seed_fragment(&d);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Seed-mapping invariants (assessment → proposed ADRs)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::default())]

    /// `seed_drafts` yields exactly one draft per practice with a non-blank name,
    /// each with a non-empty title; and every `seed_fragment` is a well-formed MADR
    /// body — marked, with the required sections, newline-terminated — for any
    /// (adversarial-unicode) assessment.
    #[test]
    fn seed_mapping_holds_structural_invariants(a in arb_assessment()) {
        let drafts = seed_drafts(&a);
        let expected = a
            .domains
            .iter()
            .flat_map(|d| &d.practices)
            .filter(|p| !p.name.trim().is_empty())
            .count();
        prop_assert_eq!(drafts.len(), expected, "one draft per non-blank practice");
        for d in &drafts {
            prop_assert!(!d.title.trim().is_empty(), "seed title must be non-empty");
            let body = seed_fragment(d);
            prop_assert!(body.starts_with(SEED_MARKER), "fragment must start with the seed marker");
            prop_assert!(body.contains("## Context and Problem Statement"));
            prop_assert!(body.contains("## Decision Drivers"));
            prop_assert!(body.contains("## Implementation"));
            prop_assert!(body.ends_with('\n'), "fragment must be newline-terminated");
        }
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
    fn rewrite_status_is_idempotent(text in arb_text(), idx in 0usize..5) {
        let status = STATUSES[idx];
        let once = format::rewrite_status(&text, status, None);
        let twice = format::rewrite_status(&once, status, None);
        prop_assert_eq!(&once, &twice, "rewrite_status not idempotent");
    }

    /// Removing the `Review by:` line twice is the same as removing it once.
    #[test]
    fn rewrite_review_by_clear_is_idempotent(text in arb_text()) {
        let once = format::rewrite_review_by(&text, None);
        let twice = format::rewrite_review_by(&once, None);
        prop_assert_eq!(&once, &twice, "rewrite_review_by(None) not idempotent");
    }

    /// Upserting the same `label: url` reference twice is byte-identical to once.
    #[test]
    fn upsert_reference_is_idempotent(
        text in arb_text(),
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
