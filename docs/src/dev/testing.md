# Testing & fuzzing

How adroit is tested — what the suites are, how to run and soak them, how to
extend them as the code changes, and how to drive an AI assistant to do it. For
the bug-finding campaign that built these suites, see
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
| Unit tests | `#[cfg(test)]` in each `src/*.rs` | pure functions behave | instant |
| CLI integration | `tests/cli.rs` | the real binary does X on a temp repo (incl. every regression) | fast |
| Model-based oracle | `tests/model.rs` | random command sequences never violate the invariants, across the format × layout × scheme × relink_scope matrix | ~40s |
| Parser properties | `tests/parsers.rs` | the parsers never panic + obey round-trip/idempotence laws (random) | ~1s |
| Coverage-guided fuzz | `tests/fuzz_parsers.rs` (bolero) | same parser laws, coverage-guided under `cargo bolero` | ~1s |
| Config precedence | `tests/config_precedence.rs` | a setting resolves flag > env > `.env` > `config.yaml` > default | fast |
| `date_source=git` | `tests/date_source_git.rs` | the git-history timeline reconstruction is correct on real git repos | ~0.5s |
| Forge fault-injection | `tests/forge_faults.rs` (`--features forge`) | the GitHub/GitLab/Jira HTTP adapters never panic on hostile responses | ~1s |
| Forge CLI graceful | `tests/forge_cli.rs` (`--features forge`) | a down/inactive forge keeps the local ADR (never loses it) | ~0.1s |
| Web security | `src/serve/mod.rs` tests (`--features web`) | the dashboard's markdown renderer can't be XSS'd; the dir picker can't crash | ~0.5s |

The centerpiece is the **oracle** (`tests/model.rs`): it generates a random matrix
cell (format × layout × naming × relink_scope) and a random sequence of mutating
CLI commands (`new`, `set-status`, `supersede`, `set-review`, `renumber`,
`relink`), runs each against the **real binary** on a throwaway `TempDir`, and
asserts a battery of invariants after **every** command — on-disk state agrees
with an in-memory oracle, `adroit check` is clean, the repo stays link-canonical
(scope-aware), and each ADR sits where its status implies.

## Running

```sh
just ci                   # what CI runs
just test                 # default features (tui)
just test-forge           # + forge adapters (tests/forge_faults.rs, forge_cli.rs)
just test-web             # + web dashboard (serve security tests)

cargo test --test model            # just the oracle
cargo test --test cli supersede    # CLI tests whose name contains "supersede"
just fmt-check && just lint && just lint-forge && just lint-web
```

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
