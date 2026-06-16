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
use adroit::lint;
use adroit::naming::{AdrRef, NamingScheme};
use adroit::plan;
use adroit::publish;
use adroit::view::Severity;

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

/// A whole-line bracket placeholder in the run-2 wart's shape — a curated
/// opener (mixed case), an arbitrary bracket-free tail, optionally behind a
/// list marker or light indentation: `[Insert …]`, `- [Your …]`, `1. [TBD …]`.
fn arb_placeholder_line() -> impl Strategy<Value = String> {
    let opener = prop::sample::select(vec![
        "Insert",
        "insert",
        "INSERT",
        "Add",
        "Describe",
        "List",
        "Enter",
        "Provide",
        "Specify",
        "Replace",
        "Fill",
        "Include",
        "Your",
        "your",
        "Name of",
        "To be",
        "TODO",
        "TBD",
        "FIXME",
        "Placeholder",
        "Optional",
    ]);
    let tail = prop::string::string_regex("[A-Za-z0-9 ,.'-]{0,40}").expect("valid regex");
    let marker = prop::sample::select(vec!["", "- ", "* ", "1. ", "12. ", "  "]);
    (marker, opener, tail).prop_map(|(m, o, t)| format!("{m}[{o} {t}]"))
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
        let _ = format::rewrite_created(&text, None);
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

    /// The `Created:` stamp (ADR-0011) obeys the same algebra as `Review by:`:
    /// upserting a date is idempotent, the stamped document parses the date
    /// back (round-trip on any document that parses as a markdown ADR), and
    /// clearing twice equals clearing once.
    #[test]
    fn rewrite_created_round_trips_and_is_idempotent(text in arb_text(), y in 2000i32..2100, o in 1u8..13, d in 1u8..29) {
        let date = time::Date::from_calendar_date(y, time::Month::try_from(o).unwrap(), d).unwrap();
        let stamp = adroit::adr::CreatedOn::new(date);
        let once = format::rewrite_created(&text, Some(stamp));
        let twice = format::rewrite_created(&once, Some(stamp));
        prop_assert_eq!(&once, &twice, "rewrite_created not idempotent");
        // Round-trip: a stamped document parses the date back verbatim. Mixed
        // `\r\n`/`\n` documents are excluded — the rewriters pick one newline
        // convention per document (the same documented limitation
        // `rewrite_review_by` has; adroit never writes mixed-newline files).
        if !text.contains('\r') {
            let doc = format!("# ADR-0001: T\n\n## Status\n\nProposed\n\n{text}");
            let stamped = format::rewrite_created(&doc, Some(stamp));
            if let Ok(adr) = format::parse_markdown(&stamped, None, NamingScheme::Sequential) {
                prop_assert_eq!(adr.created_on, Some(stamp), "{}", stamped);
            }
        }
        let cleared_once = format::rewrite_created(&text, None);
        let cleared_twice = format::rewrite_created(&cleared_once, None);
        prop_assert_eq!(&cleared_once, &cleared_twice, "rewrite_created(None) not idempotent");
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

    /// The plan-persistence helpers (ADR-0008) tolerate arbitrary bodies
    /// without panicking — they scan untrusted ADR documents.
    #[test]
    fn plan_helpers_never_panic(text in arb_text()) {
        let _ = plan::extract(&text);
        let _ = plan::has_hand_written_section(&text);
        let _ = plan::splice(&text, &text);
    }

    /// Splicing a (marker-free) plan into any body stores it verbatim: the
    /// stored read returns exactly the trimmed plan, and a second identical
    /// splice is byte-identical (the converge property `--save --force` rides).
    #[test]
    fn plan_splice_round_trips_and_is_idempotent(body in arb_text(), p in arb_text()) {
        // A plan carrying its own begin/end marker line is not representable
        // verbatim (documented limitation) — keep the law to marker-free plans.
        prop_assume!(!p.contains("<!-- adroit:plan -->") && !p.contains("<!-- /adroit:plan -->"));
        prop_assume!(!p.trim().is_empty());
        let once = plan::splice(&body, &p);
        prop_assert_eq!(plan::extract(&once), Some(p.trim()), "stored plan must read back verbatim");
        let twice = plan::splice(&once, &p);
        prop_assert_eq!(&once, &twice, "splice not idempotent");
    }

    /// An AI draft can never read as a hand-written `## Implementation`
    /// section — the M5 dogfood-rehearsal regression: a model-emitted bare
    /// heading with real content would block `plan --save` forever. The draft
    /// sanitizer (shared by every AI splice flow) holds this for arbitrary
    /// model output. Plan-marker lines are excluded: a full echoed span is
    /// managed content (covered by unit tests), and a *partial* marker pair is
    /// plan.rs's documented non-representable limitation.
    #[test]
    fn ai_drafts_never_block_plan_save(text in arb_text()) {
        prop_assume!(!text.contains("<!-- adroit:plan -->") && !text.contains("<!-- /adroit:plan -->"));
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let fake = adroit::ai::FakeProvider { canned: text };
        let draft = adroit::ai::draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        prop_assert!(!plan::has_hand_written_section(&draft), "{}", draft);
    }

    /// An AI draft can never re-introduce the mechanical preamble the splice
    /// preserves — the run-1 skeleton-echo regression generalized: whatever the
    /// model emits (plan spans excluded, as above), the sanitized draft carries
    /// no `## Status` / `## Stakeholders` section heading and exactly the one
    /// ai-suggested marker the wrapper prepends (no seeded-from-assessment
    /// echo either).
    #[test]
    fn ai_drafts_never_duplicate_the_mechanical_preamble(text in arb_text()) {
        prop_assume!(!text.contains("<!-- adroit:plan -->") && !text.contains("<!-- /adroit:plan -->"));
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let fake = adroit::ai::FakeProvider { canned: text };
        let draft = adroit::ai::draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        for line in draft.lines() {
            let t = line.trim();
            prop_assert!(!t.eq_ignore_ascii_case("## Status"), "{}", draft);
            prop_assert!(!t.eq_ignore_ascii_case("## Stakeholders"), "{}", draft);
        }
        prop_assert_eq!(draft.matches("<!-- adroit:ai-suggested -->").count(), 1, "{}", draft);
        prop_assert_eq!(draft.matches("<!-- adroit:seeded-from-assessment -->").count(), 0, "{}", draft);
    }

    /// An AI draft never carries a whole-line bracket placeholder — the run-2
    /// regression (playbook ADR-0010's "[Insert implementation plan…]")
    /// generalized: wherever the model drops `[Insert …]` / `[Your …]`-shaped
    /// filler in its output (plan spans and fenced code excluded — those stay
    /// verbatim by design), the sanitized draft is free of it.
    #[test]
    fn ai_drafts_never_carry_bracket_placeholder_lines(
        pre in arb_text(), ph in arb_placeholder_line(), post in arb_text()
    ) {
        let text = format!("{pre}\n{ph}\n{post}");
        prop_assume!(!text.contains("<!-- adroit:plan -->") && !text.contains("<!-- /adroit:plan -->"));
        prop_assume!(!text.contains("```") && !text.contains("~~~"));
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let fake = adroit::ai::FakeProvider { canned: text };
        let draft = adroit::ai::draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        for line in draft.lines() {
            prop_assert!(!lint::bracket_placeholder(line),
                "placeholder survived: {:?} in {}", line, draft);
        }
    }

    /// Dropping a tail placeholder never strands the horizontal rule above it
    /// (the run-2 artifact's exact `---` + placeholder closing shape): the
    /// sanitized draft never ends on a rule.
    #[test]
    fn trailing_placeholder_never_orphans_a_rule(
        body in arb_text(), ph in arb_placeholder_line()
    ) {
        let text = format!("{body}\n\n---\n\n{ph}");
        prop_assume!(!text.contains("<!-- adroit:plan -->") && !text.contains("<!-- /adroit:plan -->"));
        prop_assume!(!text.contains("```") && !text.contains("~~~"));
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let fake = adroit::ai::FakeProvider { canned: text };
        let draft = adroit::ai::draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        let last = draft.trim_end().lines().last().unwrap_or("").trim();
        prop_assert!(!matches!(last, "---" | "***" | "___"), "orphaned rule: {}", draft);
    }

    /// Fenced code is exempt from the placeholder rule — an example config
    /// showing `[insert API key]`-style lines survives the sanitizer verbatim.
    #[test]
    fn fenced_placeholder_lookalikes_stay_verbatim(ph in arb_placeholder_line()) {
        let text = format!("## Notes\n\nReal content.\n\n```\n{ph}\n```\n");
        let fake = adroit::ai::FakeProvider { canned: text };
        let draft = adroit::ai::draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        prop_assert!(draft.contains(&ph), "fenced line lost: {:?} in {}", ph, draft);
    }

    /// The drop-counting sanitizer (`draft_compose_counted`, behind `import
    /// --ai`'s telemetry) never alters the body the count-free `draft_compose`
    /// produces — the counts are a pure observation over arbitrary model output,
    /// not a behavior change. (No-panic + body-stability in one property.)
    #[test]
    fn counted_draft_body_matches_the_plain_draft(text in arb_text()) {
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let plain = adroit::ai::draft_compose(
            &adroit::ai::FakeProvider { canned: text.clone() }, "T", "i", "old", &[]).unwrap();
        let (counted, _drops) = adroit::ai::draft_compose_counted(
            &adroit::ai::FakeProvider { canned: text }, "T", "i", "old", &[]).unwrap();
        prop_assert_eq!(plain, counted);
    }

    /// The telemetry never undercounts a real wart: a whole-line bracket
    /// placeholder dropped from the body is always tallied (≥ 1) — so the
    /// `import --ai` artifacts can't read "the model emitted none" when the
    /// sanitizer in fact ate one (the run-3 observability wart). Plan spans and
    /// fenced code excluded (those stay verbatim, uncounted, by design).
    #[test]
    fn a_dropped_bracket_placeholder_is_always_counted(
        pre in arb_text(), ph in arb_placeholder_line(), post in arb_text()
    ) {
        let text = format!("{pre}\n{ph}\n{post}");
        prop_assume!(!text.contains("<!-- adroit:plan -->") && !text.contains("<!-- /adroit:plan -->"));
        prop_assume!(!text.contains("```") && !text.contains("~~~"));
        prop_assume!(text != "__ERROR__"); // the FakeProvider failure hook
        let fake = adroit::ai::FakeProvider { canned: text };
        let (_body, drops) = adroit::ai::draft_compose_counted(&fake, "T", "i", "old", &[]).unwrap();
        prop_assert!(drops.bracket_placeholder >= 1, "uncounted placeholder: {:?}", ph);
    }

    /// `lint` flags every whole-line bracket placeholder on an otherwise-clean
    /// body — and only as a Warning (a CI lint gate stays green).
    #[test]
    fn lint_warns_on_any_bracket_placeholder_line(ph in arb_placeholder_line()) {
        let body = format!(
            "## Context and Problem Statement\n\nReal context.\n\n\
             ## Considered Options\n\n1. A\n2. B\n\n\
             ## Decision Outcome\n\nChosen: A, because reasons.\n\n\
             ### Negative Consequences\n\n- A real downside.\n\n{ph}\n"
        );
        let f = lint::lint(&body);
        prop_assert!(
            f.iter().any(|x| x.severity == Severity::Warning
                && x.message.contains("bracket placeholder")),
            "no placeholder warning for {:?}: {:?}", ph, f
        );
        prop_assert!(f.iter().all(|x| x.severity == Severity::Warning), "{:?}", f);
    }

    /// `lint` and the placeholder detector scan untrusted ADR bodies — they
    /// must tolerate arbitrary input without panicking.
    #[test]
    fn lint_never_panics(text in arb_text()) {
        let _ = lint::lint(&text);
        for line in text.lines() {
            let _ = lint::bracket_placeholder(line);
        }
    }
}
