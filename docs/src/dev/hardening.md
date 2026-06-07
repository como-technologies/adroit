# Hardening & quality

adroit is hardened with an **AI-driven, property-first** approach: a deterministic
Rust backbone encodes adroit's documented invariants, and an AI assistant acts as
the live explorer that drives the binary, triages anomalies, and crystallizes each
finding into a permanent regression. This page describes that approach and the
classes of bug it targets. The suites themselves live in
[Testing & Fuzzing](./testing.md); the repeatable procedure is the `/harden` skill
([Project Skills](./skills.md)).

The honest division of labor: **the deterministic suites are the bug _detector_;
the assistant (or you) is the input _generator_ and _triager_.**

## Approach

Several modalities, each aimed at a different high-yield surface:

- **Model-based oracle** (`tests/model.rs`) — random mutating-command sequences
  against the real binary, asserting state-agreement, status↔directory, clean
  `check`, and link-canonicality after every command, across the full
  **format × layout × scheme × relink_scope** matrix; plus migrate round-trips
  (`by_status ↔ flat` byte-identical, `markdown ↔ frontmatter` logically lossless).
- **Parser properties + coverage-guided fuzz** (`tests/parsers.rs`,
  `tests/fuzz_parsers.rs` via bolero) — no-panic + round-trip / idempotence laws on
  arbitrary and multibyte input.
- **Forge fault-injection** (`tests/forge_faults.rs`) — every adapter method over a
  hostile transport returning malformed HTTP.
- **Web security** (`src/serve/`) — markdown-render XSS + directory-picker
  crash-safety.
- **Targeted harnesses** — config precedence, `date_source = git`, and forge CLI
  graceful degradation.

A test-only `ADROIT_TODAY` env override pins "today" so the `date` scheme is
deterministic; the default path is unchanged.

## Where bugs hide in adroit

The recurring failure classes — worth a second look in any change to the write
path, the parsers, or a renderer:

- **Scheme-agnostic resolution.** Numeric-only or path-only link/identity
  resolution silently breaks for the slug schemes (`date` / `uuid` /
  `per_category`). Route every ref→ADR resolution through the naming seam
  (`ref_in_link` / `ref_in_link_from`), the way `relink` does — never hand-parse a
  number out of a link.
- **Canonical link form.** Ad-hoc relative-path helpers diverge from the canonical
  `links::rel_link` (e.g. by dropping the same-dir `./`). All supersession and
  cross-ADR links must go through that one engine; the oracle's "relink is always a
  no-op" invariant catches the class that example-based tests miss.
- **Robustness on hostile input.** Byte-slicing a string (instead of `.chars()`),
  newline detection that misses a lone `\r`, and trusting external text in a
  renderer are all latent panics or injection. Operate on char boundaries,
  normalize newlines, escape raw HTML, and neutralize dangerous URL schemes. The
  same rule covers *imported files* — the assessment-import JSON/YAML parser and its
  seed mapping take untrusted input and must only ever yield `Ok`/`Err`, never panic
  (fuzzed via `fuzz_parse_assessment` + a structural-invariant property).
- **Auth, tokens & external responses.** Anything touching credentials gets extra
  scrutiny: HTTP/forge/OAuth responses are untrusted (parsers must never panic —
  fuzzed via `fuzz_oauth_token_parse` + property tests over arbitrary bytes); values
  interpolated into requests must be encoded so they can't inject (the `form_encode`
  injection-safety property); secrets must never reach stdout/logs (a token is
  never echoed by `auth`, asserted end-to-end); and the credential store degrades
  (keychain unavailable → file) deterministically, tested with fakes.

## Crystallize every finding

A bug isn't done when it's fixed — it's done when it can't come back. Every defect
becomes a focused regression in `tests/cli.rs`, fixed at root cause; and where a
property suite surfaced it, a committed, minimized seed
(`tests/<suite>.proptest-regressions`) replays first on every run so it can't
silently return. See
[Testing & Fuzzing → Triaging a failure](./testing.md#triaging-a-failure) for the
explore → classify → crystallize loop.
