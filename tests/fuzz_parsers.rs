//! Coverage-guided parser fuzzing via [`bolero`].
//!
//! The same targets run two ways:
//!  - as **stable property tests** under `cargo test --test fuzz_parsers` (so they
//!    run in CI, replaying any committed corpus + a bounded random sample), and
//!  - **coverage-guided** under `cargo bolero test <name>` (nightly + the
//!    `cargo-bolero` CLI), which instruments the binary and keeps inputs that reach
//!    new code — far better at exploring the parsers' input space than the random
//!    proptest generation in `tests/parsers.rs`.
//!
//! A coverage-guided run finds panics/hangs on its own; the round-trip/idempotence
//! assertions below also catch logic drift. When a run finds something, minimize it
//! and add it to the target's corpus, then fix.
//!
//! See the book's Development pages: Testing & Fuzzing (docs/src/dev/testing.md)
//! and Hardening & Quality (docs/src/dev/hardening.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use adroit::adr::Status;
use adroit::naming::{AdrRef, NamingScheme};
use adroit::{format, links, plan, publish};

use bolero::check;

const SCHEMES: [NamingScheme; 4] = [
    NamingScheme::Sequential,
    NamingScheme::Date,
    NamingScheme::Uuid,
    NamingScheme::PerCategory,
];

/// The markdown format helpers tolerate any input and `rewrite_status` is
/// idempotent (modulo the documented lone-`\r` gap, #4).
#[test]
fn fuzz_format_helpers() {
    check!().with_type::<String>().for_each(|input: &String| {
        for scheme in SCHEMES {
            let _ = format::parse_markdown(input, None, scheme);
            let _ = format::parse_markdown_section_supersession(input, scheme, None);
        }
        let _ = format::parse_markdown_section_status(input);
        let _ = format::parse_references(input);
        let _ = adroit::frontmatter::deserialize(input);

        let once = format::rewrite_status(input, Status::Accepted, None);
        let twice = format::rewrite_status(&once, Status::Accepted, None);
        assert_eq!(once, twice, "rewrite_status not idempotent");

        // The `Created:` stamp (ADR-0011): no panic on arbitrary input, and
        // upserting the same date is idempotent.
        let stamp = adroit::adr::CreatedOn::new(time::macros::date!(2026 - 06 - 12));
        let once = format::rewrite_created(input, Some(stamp));
        let twice = format::rewrite_created(&once, Some(stamp));
        assert_eq!(once, twice, "rewrite_created not idempotent");
        let _ = format::rewrite_created(input, None);
    });
}

/// The plan-persistence helpers (ADR-0008) never panic on arbitrary bodies,
/// and a marker-free splice stores the plan verbatim and idempotently.
#[test]
fn fuzz_plan_helpers() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(body, p): &(String, String)| {
            let _ = plan::extract(body);
            let _ = plan::has_hand_written_section(body);
            let once = plan::splice(body, p);
            // The managed section survives any body; with a marker-free,
            // non-empty plan it reads back verbatim and re-splicing converges.
            if !p.contains("<!-- adroit:plan -->")
                && !p.contains("<!-- /adroit:plan -->")
                && !p.trim().is_empty()
            {
                assert_eq!(
                    plan::extract(&once),
                    Some(p.trim()),
                    "stored plan must read back verbatim"
                );
                assert_eq!(plan::splice(&once, p), once, "splice not idempotent");
            }
        });
}

/// `links::rewrite_links` never panics and reaches a fixpoint.
#[test]
fn fuzz_link_rewriter() {
    check!().with_type::<String>().for_each(|input: &String| {
        let resolve = |target: &str| {
            links::number_in_target(target)
                .map(|n| PathBuf::from(format!("/r/accepted/{n:04}-x.md")))
        };
        let dir = Path::new("/r/proposed");
        let (once, _) = links::rewrite_links(input, dir, resolve);
        let (twice, changed) = links::rewrite_links(&once, dir, resolve);
        assert_eq!(changed, 0, "rewrite_links second pass changed links");
        assert_eq!(once, twice, "rewrite_links not a fixpoint");
        let _ = links::relative_md_targets(input);
        let _ = links::number_in_target(input);
        let _ = links::is_relative_md(input);
    });
}

/// The publish cross-link rewriter never panics on hostile input (nested /
/// adjacent brackets, multibyte, lone `\r`) under any scheme — the byte-index
/// link scan must never slice a backwards or non-char-boundary range. `strip_h1`
/// tolerates any input too.
#[test]
fn fuzz_publish_rewriter() {
    check!().with_type::<String>().for_each(|input: &String| {
        let page = Path::new("a/page.md");
        let published: HashMap<AdrRef, PathBuf> = [
            (AdrRef::Number(1), PathBuf::from("docs/0001-x.md")),
            (
                AdrRef::Slug("20260601-y".into()),
                PathBuf::from("docs/20260601-y.md"),
            ),
        ]
        .into_iter()
        .collect();
        for scheme in SCHEMES {
            let _ = publish::rewrite_published_links(input, page, &scheme, &published, Some("cat"));
        }
        // Nothing published → exercise the unlink path (no-panic on hostile input).
        let empty: HashMap<AdrRef, PathBuf> = HashMap::new();
        let _ =
            publish::rewrite_published_links(input, page, &NamingScheme::Sequential, &empty, None);
        let _ = publish::strip_h1(input);
    });
}

/// The naming seam's parse/link helpers tolerate any input under every scheme.
#[test]
fn fuzz_naming_helpers() {
    check!().with_type::<String>().for_each(|input: &String| {
        for scheme in SCHEMES {
            let _ = scheme.parse_ref(input);
            let _ = scheme.ref_in_link(input);
            let _ = scheme.ref_in_link_from(input, Some("cat"));
            let _ = scheme.ref_in_note(input);
            let _ = scheme.parse(Path::new(input), input);
        }
    });
}

/// `config::parse_remote_url` tolerates any input.
#[test]
fn fuzz_parse_remote_url() {
    check!().with_type::<String>().for_each(|input: &String| {
        let _ = adroit::config::parse_remote_url(input);
    });
}

/// The assessment parser + seed mapping tolerate any input — a hostile/garbage
/// assessment export (driven through both the JSON and YAML paths) must never
/// panic, only yield Ok/Err, and whatever parses must map to seed ADRs cleanly.
#[test]
fn fuzz_parse_assessment() {
    use adroit::import::{parse_assessment, seed_drafts, seed_fragment};
    check!().with_type::<String>().for_each(|input: &String| {
        for ext in ["a.json", "a.yaml", "a.toml"] {
            if let Ok(a) = parse_assessment(input, Path::new(ext)) {
                for d in seed_drafts(&a) {
                    let _ = seed_fragment(&d);
                }
            }
        }
    });
}

/// The OAuth device-token response parser tolerates any HTTP body bytes — a
/// hostile/garbage auth response must never panic, only yield Ok/Err. Drives the
/// public `oauth::poll_token` through a transport that returns the fuzzed bytes.
#[cfg(feature = "forge")]
#[test]
fn fuzz_oauth_token_parse() {
    use adroit::forge::oauth;
    use adroit::forge::{ForgeError, HttpResponse, HttpTransport};

    struct Body(Vec<u8>);
    impl HttpTransport for Body {
        fn request(
            &self,
            _m: &str,
            _u: &str,
            _h: &[(&str, &str)],
            _b: Option<&[u8]>,
        ) -> Result<HttpResponse, ForgeError> {
            Ok(HttpResponse {
                status: 200,
                body: self.0.clone(),
            })
        }
    }
    check!().with_type::<Vec<u8>>().for_each(|bytes: &Vec<u8>| {
        let t = Body(bytes.clone());
        let _ = oauth::poll_token(&t, "https://x/token", "cid", "dc");
        let _ = oauth::request_device_code(&t, "https://x/device", "cid", "repo");
    });
}

/// `lint` (and its bracket-placeholder detector) scans untrusted ADR bodies —
/// arbitrary input must never panic, and a fully fenced body never yields a
/// placeholder finding (fenced code is exempt by design).
#[test]
fn fuzz_lint() {
    use adroit::lint;
    check!().with_type::<String>().for_each(|input: &String| {
        let _ = lint::lint(input);
        for line in input.lines() {
            let _ = lint::bracket_placeholder(line);
        }
        if !input.contains("```") && !input.contains("~~~") {
            let fenced = format!("```\n{input}\n```");
            assert!(
                lint::lint(&fenced)
                    .iter()
                    .all(|f| !f.message.contains("bracket placeholder")),
                "placeholder finding from fenced content: {input:?}"
            );
        }
    });
}

/// The AI draft sanitizer consumes untrusted model output — arbitrary input
/// must never panic, and (plan spans / fenced code aside, which stay verbatim
/// by design) the sanitized draft carries exactly one ai-suggested marker and
/// no whole-line bracket placeholder. The drop-counting variant
/// (`draft_compose_counted`, behind `import --ai`'s telemetry) is fuzzed in
/// lockstep: it must never panic and must yield a body byte-identical to the
/// count-free draft — the counts are a pure observation, never a behavior change.
#[test]
fn fuzz_ai_sanitizer() {
    use adroit::{ai, lint};
    check!().with_type::<String>().for_each(|input: &String| {
        if input == "__ERROR__" {
            return; // the FakeProvider failure hook
        }
        let fake = ai::FakeProvider {
            canned: input.clone(),
        };
        let draft = ai::draft_compose(&fake, "T", "i", "old", &[]).expect("fake never fails");
        let (counted, _drops) =
            ai::draft_compose_counted(&fake, "T", "i", "old", &[]).expect("fake never fails");
        assert_eq!(counted, draft, "counted body diverged: {input:?}");
        if !input.contains("<!-- adroit:plan -->")
            && !input.contains("<!-- /adroit:plan -->")
            && !input.contains("```")
            && !input.contains("~~~")
        {
            assert_eq!(
                draft.matches("<!-- adroit:ai-suggested -->").count(),
                1,
                "marker count drifted: {input:?}"
            );
            for line in draft.lines() {
                assert!(
                    !lint::bracket_placeholder(line),
                    "placeholder survived: {line:?} from {input:?}"
                );
            }
        }
    });
}

/// The MCP request handler tolerates any stdin line — a hostile / garbage JSON-RPC
/// message must never panic, only yield an error response. The server (projected
/// from the manifest) is built once; the fuzzer drives `handle_line` over it.
#[cfg(feature = "mcp")]
#[test]
fn fuzz_mcp_request() {
    let server =
        adroit::mcp::Server::new(&adroit::config::Config::default(), &std::env::temp_dir());
    check!().with_type::<String>().for_each(|input: &String| {
        let _ = adroit::mcp::handle_line(&server, input);
    });
}
