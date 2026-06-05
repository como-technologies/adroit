# Hardening & quality

A record of the AI-driven hardening campaign that built adroit's
[test suites](./testing.md) and the defects it found. The goal was *bugs found and
fixed per unit effort*, optimizing for the highest-yield surfaces.

## Approach

A deterministic Rust backbone that encodes adroit's documented invariants, with an
AI assistant as the live explorer that drives the binary, triages anomalies, and
crystallizes each finding into a permanent regression. Four modalities, plus
targeted harnesses for the trickier settings:

- **Model-based oracle** (`tests/model.rs`) — random mutating-command sequences
  against the real binary, asserting state-agreement, status↔directory,
  clean `check`, and link-canonicality after every command, across the full
  **format × layout × scheme × relink_scope** matrix; plus migrate round-trips
  (`by_status↔flat` byte-identical, `markdown↔frontmatter` logically lossless).
- **Parser properties + coverage-guided fuzz** (`tests/parsers.rs`,
  `tests/fuzz_parsers.rs` via bolero) — no-panic + round-trip/idempotence laws on
  arbitrary/multibyte input.
- **Forge fault-injection** (`tests/forge_faults.rs`) — every adapter method over a
  `HostileTransport` returning malformed HTTP.
- **Web security** (`src/serve/mod.rs`) — markdown-render XSS + directory-picker
  crash-safety.
- **Targeted harnesses** — config precedence (`tests/config_precedence.rs`),
  `date_source=git` (`tests/date_source_git.rs`), and forge CLI graceful
  degradation (`tests/forge_cli.rs`).

A test-only `ADROIT_TODAY` env override (read by `query::today` /
`store::today_local`) pins "today" so the date scheme is deterministic; the
default path is unchanged.

## Findings

12 defects found; **all fixed**, each guarded by a regression.

| # | Defect | Surface | Status |
|---|--------|---------|--------|
| 1 | `supersede` reciprocal note used a non-canonical same-dir link | core | Fixed |
| 2 | In-place `supersede` wrote a non-canonical `## Status` link | core | Fixed |
| 3 | `NamingScheme::display` panicked on a multibyte uuid slug | parser | Fixed |
| 4 | `upsert_reference` non-idempotent on a lone-`\r` document | parser | Fixed |
| 5 | `uuid` supersede produced a repo that failed `check` | core | Fixed |
| 6 | `frontmatter` + a slug scheme failed with a cryptic error | core | Fixed |
| 7 | `by_category` supersede wrote a broken link | core | Fixed |
| 8 | `renumber` strands a frontmatter supersession ref | core | Fixed¹ |
| 9 | `per_category` same-category cross-ADR links didn't resolve | core | Fixed |
| 10 | Stored XSS — dashboard rendered raw HTML / `javascript:` | web | Fixed |
| 11 | `config show`/`get naming` ignored `--naming` / `ADROIT_NAMING` | config | Fixed |
| 12 | `check`'s cross-ADR link validation was numeric-only | core | Fixed |
| — | `Jira::with_transport` was `#[cfg(test)]`-gated unlike siblings | forge | Fixed |

¹ Two-layer fix: `renumber` now retargets the bare-number frontmatter ref through
the model (`frontmatter::remap_numeric_refs`), reaching what the markdown-link
relabeler can't, and `check`'s frontmatter-supersession validation remains as a
backstop.

### Notable themes

- **Scheme-agnostic resolution** (#5, #9, #12) — several bugs came from numeric-only
  or path-only link/identity resolution that broke for the slug schemes
  (date/uuid/per_category). The fix in each case was to route through the naming
  seam (`ref_in_link` / `ref_in_link_from`) like `relink` already did.
- **Canonical link form** (#1, #2, #7) — ad-hoc relative-path helpers diverged from
  the canonical `links::rel_link` by dropping the same-dir `./`; supersession links
  now go through the one engine. A model-based invariant ("relink is always a
  no-op") catches this class that example tests miss.
- **Robustness on hostile input** (#3, #4, #10) — `display` byte-slicing, the
  rewriters' newline detection (a lone `\r`), and the dashboard markdown renderer
  all trusted their input; fixed to operate on chars, normalize a lone `\r`, and
  escape raw HTML / neutralize dangerous URL schemes.

## Coverage & remaining gaps

Covered: the write-path core across all valid matrix cells (soaked to ~1500
cases/cell), the parsers (random + coverage-guided), forge response parsing (~5000
cases) and graceful degradation, config precedence, the git timeline path, and the
dashboard XSS surface.

Still open:

- **Forge happy-path live wiring** — issue+PR creation against a mock HTTP server
  with a git remote (the orchestration cores are unit-tested with mock adapters and
  the adapters are fuzzed; this is the live-glue gap).
