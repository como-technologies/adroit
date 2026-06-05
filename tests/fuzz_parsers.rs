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

use std::path::{Path, PathBuf};

use adroit::adr::Status;
use adroit::naming::NamingScheme;
use adroit::{format, links};

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
