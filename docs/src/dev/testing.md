# Testing & fuzzing

How adroit is tested — what the suites are, how to run and soak them, how to
extend them as the code changes, and how to drive an AI assistant to do it. For
the approach behind these suites and where bugs tend to hide, see
[Hardening & quality](./hardening.md).

> **TL;DR**
> ```sh
> just ci       # the full gate: fmt, clippy, every suite, book, audit
> just test     # default-feature tests (unit + CLI + oracle + parsers)
> just model    # wide property soak (PROPTEST_CASES, default 2000)
> ```

## The test layers

| Layer | Where | What it proves | Speed |
|---|---|---|---|
| Unit tests | `#[cfg(test)]` in each `src/*.rs` (incl. the pure TUI `TuiState` / `apply_action` layer and the AI interview / compose builders) | pure functions behave | instant |
| AI authoring | `src/ai/` tests + the `ADROIT_AI_FAKE` seam | the interview / compose flow drafts *prose* only (identity / status stay mechanical) and degrades cleanly with no provider | instant |
| CLI integration | `tests/cli.rs` | the real binary does X on a temp repo (incl. every regression) | fast |
| Model-based oracle | `tests/model.rs` | random command sequences never violate the invariants, across the format × layout × scheme × relink_scope matrix | ~40s |
| Parser properties | `tests/parsers.rs` | the parsers never panic + obey round-trip/idempotence laws (random) | ~1s |
| Coverage-guided fuzz | `tests/fuzz_parsers.rs` (bolero) | same parser laws, coverage-guided under `cargo bolero` | ~1s |
| Config precedence | `tests/config_precedence.rs` | a setting resolves flag > env > `.env` > `config.yaml` > default | fast |
| `date_source=git` | `tests/date_source_git.rs` | the git-history timeline reconstruction is correct on real git repos | ~0.5s |
| Forge fault-injection | `tests/forge_faults.rs` (`forge`; default build) | the GitHub/GitLab/Jira HTTP adapters never panic on hostile responses | ~1s |
| Forge CLI graceful | `tests/forge_cli.rs` (`forge`; default build) | a down/inactive forge keeps the local ADR (never loses it) | ~0.1s |
| Web security | `src/serve/mod.rs` tests (`--features web`) | the dashboard's markdown renderer can't be XSS'd; the dir picker can't crash | ~0.5s |

The centerpiece is the **oracle** (`tests/model.rs`): it generates a random matrix
cell (format × layout × naming × relink_scope) and a random sequence of mutating
CLI commands (`new`, `import`, `set-status`, `supersede`, `set-review`,
`renumber`, `relink`, `link`, `draft`), runs each against the **real binary** on a throwaway
`TempDir`, and asserts a battery of invariants after **every** command — on-disk
state agrees with an in-memory oracle, `adroit check` is clean, the repo stays
link-canonical (scope-aware), and each ADR sits where its status implies. `link`
(frontmatter-only typed links) and `draft` (the AI body-splice, driven offline by
the `ADROIT_AI_FAKE` seam) aren't modeled — they're held to the same invariants,
so a typed link must heal when its target moves and a draft must keep identity /
status / links intact. After each sequence a **read-verb sweep** runs `list` /
`show` / `status` / `search` / `stats` / `graph` / `check` / `lint` / `related` /
`dedupe` / `summarize` / `plan` / `ask` / `publish --dry-run` against the
arbitrary final state, asserting they never crash and the `-o json` emitters stay
parseable.

## Running

```sh
just ci          # the full gate (see below)
just test        # default features (tui + ai + forge): unit + CLI + oracle + parsers
just test-core   # the bare core (--no-default-features): the cfg(not(feature)) paths
just test-ai     # the `ai` feature explicitly (rig adapter compiles; interview/compose)
just test-forge  # the `forge` adapters (tests/forge_faults.rs, forge_cli.rs)
just test-web    # the `web` dashboard (serve security tests; builds without the SPA)
just unit        # unit tests only (--lib)

cargo test --test model            # just the oracle
cargo test --test cli supersede    # CLI tests whose name contains "supersede"

# clippy across the feature matrix:
just lint        # default features (tui + ai + forge)
just lint-core   # --no-default-features — guards the core pulls in NO surface deps
just lint-ai     # the `ai` feature; also: just lint-forge, just lint-web
```

`just ci` runs `fmt-check → lint-core → lint → lint-web → test-core → test →
test-web → book → crate-outdated → crate-audit`. Because **`ai` and `forge` are in
the default build**, `lint` / `test` already exercise them — so `lint-ai` /
`test-ai` / `lint-forge` / `test-forge` are explicit single-feature checks you run
by hand, not separate CI steps.

### Soaking

The property suites explore a bounded number of random cases by default so the
gate stays fast. Search harder with `PROPTEST_CASES`:

```sh
just model                        # PROPTEST_CASES defaults to 2000
PROPTEST_CASES=10000 just model   # a longer soak before a release
```

### Determinism & replay

- proptest explores **different random cases each run**, so a soak finds new things
  over time — but a green gate is evidence, not proof.
- Every failure is **minimized** and its seed written to
  `tests/<suite>.proptest-regressions` (committed). Those replay first on every
  run, so a found bug can't silently return.
- The oracle pins `ADROIT_TODAY` (a test-only fixed-clock env var) and runs
  `date_source=filesystem` to stay git-free; the git path is covered separately by
  `tests/date_source_git.rs`.

## Coverage-guided fuzzing (bolero)

proptest generates *random* inputs. For the opaque parser surfaces a
**coverage-guided** fuzzer is far better — it instruments the binary and keeps
inputs that reach new code. [`bolero`](https://crates.io/crates/bolero) lets the
**same** target run both ways:

```sh
cargo test --test fuzz_parsers                       # stable property test (CI)
cargo install cargo-bolero
cargo +nightly bolero test fuzz_format_helpers -T 60sec   # coverage-guided, 60s
```

The targets are `fuzz_format_helpers`, `fuzz_link_rewriter`, `fuzz_naming_helpers`,
`fuzz_parse_remote_url`, `fuzz_oauth_token_parse` (the OAuth device-token response
parser — a hostile auth response must never panic), and `fuzz_parse_assessment`
(the assessment-import JSON/YAML parser + seed mapping). cargo-bolero builds
its instrumented target with
`--profile fuzz`, so the repo defines a `[profile.fuzz]` in `Cargo.toml` (inherits
`release`, keeps debug-assertions + overflow-checks on) — without it the run fails
with `error: profile 'fuzz' is not defined`.

A coverage-guided run finds crashes on its own; to catch *logic* bugs it uses the
same assertions the property targets do. When it finds something, minimize it, add
it to the corpus, and fix.

## Triaging a failure

A red property test prints the **minimal failing input**. Reproduce it against the
real binary and decide what kind of failure it is:

1. **Reproduce** — run the exact sequence on a `mktemp -d` with the same flags;
   inspect the files.
2. **Classify**:
   - *Real bug* → focused regression in `tests/cli.rs`, fix at root cause.
   - *Intended behavior change* → update the **model** in `tests/model.rs` (it
     encodes the intended semantics). A red oracle after a deliberate change means
     update the model, not the code.
   - *Harness/model gap* → fix the oracle; gate a known-deferred bug with a
     documented skip.
3. **Crystallize** — the committed regression seed makes it permanent.

## Extending the suites

The oracle is an **executable spec** — keep it in step with the code:

- **New verb** → add an `Op` variant, an arm in `Harness::apply`, a weight in
  `arb_op()`.
- **New scheme / layout / format** → add a weighted cell to `arb_profile()`.
  Identity is read back from disk, so a new scheme needs almost no prediction.
- **Behavior depends on a setting** → branch the model on it (it already branches
  on format and relink_scope).
- **New pure parser** → add a no-panic + round-trip property to `tests/parsers.rs`
  (and a bolero target in `tests/fuzz_parsers.rs`).
- **New forge provider** → add it to the adapter list in `tests/forge_faults.rs`.
- **Fixing a deferred bug** → delete its oracle skip.

## Working with an AI assistant

These suites were built AI-first (the assistant drove the binary, triaged
failures, crystallized regressions). That workflow is reusable — hand off the
tedious, high-leverage parts:

- *"Soak the oracle and triage anything it finds."*
- *"Widen the oracle to cover `<new verb / cell / setting>`."*
- *"Reproduce and minimize this failure"* (paste the minimal failing input).
- *"Turn this into a regression"* (+ the production fix).

The honest division of labor: **the deterministic suites are the bug *detector*;
the assistant (or you) is the input *generator* and *triager*.** Point it at a
soak whenever you've changed the write path, a parser, or the renderer.
