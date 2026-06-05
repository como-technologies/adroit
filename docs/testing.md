# Testing & fuzzing adroit

This is the contributor guide to adroit's automated testing — what the suites are,
how to run and soak them, how to extend them as you change the code, and how to
work with an AI assistant (Claude) to do all of that. It is the practical
companion to the campaign write-up in
[`docs/superpowers/hardening-blitz-findings.md`](superpowers/hardening-blitz-findings.md).

> **TL;DR for the impatient**
> ```sh
> just ci          # everything the CI gate runs (fmt, clippy, all test suites, book, audit)
> just test        # default-feature tests (unit + CLI + oracle + parsers)
> just model       # wide property soak (PROPTEST_CASES, default 2000)
> ```

## The test layers

adroit is tested in layers, cheapest/most-deterministic first:

| Layer | Where | What it proves | Speed |
|---|---|---|---|
| **Unit tests** | `#[cfg(test)]` in each `src/*.rs` | pure functions behave | instant |
| **CLI integration** | `tests/cli.rs` | the real binary does X on a temp repo (incl. every regression) | fast |
| **Model-based oracle** | `tests/model.rs` | random command sequences never violate the invariants, across the format × layout × scheme matrix | ~40s |
| **Parser properties / fuzz** | `tests/parsers.rs` | the parsers never panic and obey round-trip/idempotence laws on arbitrary input | ~1s |
| **Forge fault-injection** | `tests/forge_faults.rs` (`--features forge`) | the GitHub/GitLab/Jira HTTP adapters never panic on hostile responses | ~1s |
| **Web security** | `src/serve/mod.rs` tests (`--features web`) | the dashboard's markdown renderer can't be XSS'd; the dir picker can't crash | ~0.5s |

The big one is the **oracle** (`tests/model.rs`): it generates a random matrix
cell (format × layout × naming) and a random sequence of mutating CLI commands
(`new`, `set-status`, `supersede`, `set-review`, `renumber`, `relink`), runs each
against the **real binary** on a throwaway `TempDir`, and asserts a battery of
invariants after **every** command — on-disk state agrees with an in-memory
oracle, `adroit check` is clean, the repo stays link-canonical, and each ADR sits
where its status implies. See the module header for the full contract.

## Running things

```sh
# Everything (what CI runs):
just ci

# Per-feature test suites:
just test                 # default features (tui)
just test-forge           # + forge adapters (runs tests/forge_faults.rs)
just test-web             # + web dashboard (runs the serve security tests)

# One suite / one test:
cargo test --test model            # just the oracle
cargo test --test parsers          # just the parser properties
cargo test --test cli supersede    # CLI tests whose name contains "supersede"
cargo test --features web --lib serve   # serve (web) unit tests

# Lints / format (also part of just ci):
just fmt-check && just lint && just lint-forge && just lint-web
```

### Soaking (running deeper)

The property suites (`model`, `parsers`) explore a **bounded** number of random
cases by default so the gate stays fast. To search harder, raise the budget with
`PROPTEST_CASES`:

```sh
just model                       # PROPTEST_CASES defaults to 2000
PROPTEST_CASES=10000 just model  # a longer soak before a release
PROPTEST_CASES=20000 cargo test --test parsers
```

The CI gate uses small defaults (the oracle ~192 cases, parsers/forge ~256); the
soak is where you spend real CPU looking for new bugs.

### Determinism & replay

- proptest explores **different random cases each run**, so a soak finds new
  things over time — but it also means a green gate isn't a proof, just evidence.
- Every failure proptest finds is **minimized** and its seed is written to
  `tests/<suite>.proptest-regressions` (committed). Those seeds **replay first on
  every run**, so a bug we've found can never silently come back, regardless of
  the random seed.
- The oracle pins a few things for reproducibility: `ADROIT_TODAY` (a test-only
  fixed-clock env var read by `query::today`/`store::today_local`), and it runs
  `date_source=filesystem` to stay git-free.

## Triaging a failure

When a property test goes red it prints the **minimal failing input** (a profile +
a short command sequence, or a parser input). Reproduce it against the real binary
and decide what kind of failure it is:

1. **Reproduce** — run the exact sequence by hand on a `mktemp -d` with the same
   `--format/--layout/--naming` flags, and inspect the files.
2. **Classify**:
   - *Real bug* → write a focused regression in `tests/cli.rs`, fix the production
     code at root cause, confirm green.
   - *Intended behavior change* → update the **model** in `tests/model.rs` (it
     encodes the intended semantics, e.g. "frontmatter `set-status` keeps
     `superseded_by`"). A red oracle after a deliberate change means update the
     model, not the code.
   - *Harness/model gap* → fix the oracle; if it's a known-but-deferred bug,
     gate it with a documented skip (see the existing `#8`/`per_category` skips).
3. **Crystallize** — the committed `proptest-regressions` seed makes the case a
   permanent deterministic test. Always keep it.

Worked examples of this loop (10 findings) are in
[`hardening-blitz-worklog.md`](superpowers/hardening-blitz-worklog.md).

## Extending the suites as you change adroit

The oracle is an **executable spec** — keep it in step with the code:

- **New verb** → add a variant to `Op` in `tests/model.rs`, an arm in
  `Harness::apply` (drive the binary + update the model), and a weight in
  `arb_op()`.
- **New naming scheme / layout / format** → add a weighted cell to
  `arb_profile()`. Identity is **read back from disk** after `new`, so a new
  scheme needs almost no oracle prediction.
- **Behavior depends on a config setting** → make the model branch on it (it
  already branches on format for supersession semantics).
- **New pure parser / seam** → add a no-panic + round-trip property in
  `tests/parsers.rs`.
- **New forge provider** → add it to the adapter list in `tests/forge_faults.rs`;
  the hostile-response loop covers it automatically.
- **Fixing a deferred bug** → delete the corresponding oracle skip so it starts
  exercising the case.

## Working with Claude on the suite

These suites were built AI-first (Claude drove the binary, triaged failures, and
crystallized regressions). That workflow is reusable — you can hand Claude the
high-leverage, tedious parts:

- **"Soak the oracle and triage anything it finds."** Claude runs
  `PROPTEST_CASES=…`, reproduces the minimal failing sequence on a temp repo,
  classifies it (real bug / model gap / intended change), and reports — or fixes
  it with a regression if you ask.
- **"Widen the oracle to cover `<new verb / cell / setting>`."** Claude adds the
  `Op`/`arb_profile` arm and the model logic, then soaks it to confirm.
- **"Reproduce and minimize this failure."** Paste the `minimal failing input`;
  Claude reruns it by hand, narrows it, and explains the root cause.
- **"Turn this into a regression."** Claude writes the focused `tests/cli.rs` test
  and (if you want) the production fix.

Claude works against the same commands in this guide. The honest division of
labor: **the deterministic suites are the bug *detector*; Claude (or you) is the
input *generator* and *triager*.** A green run is not a proof — point Claude at a
soak when you've changed the write path, a parser, or the renderer.

## Coverage-guided fuzzing (bolero)

proptest generates **random** inputs. For the opaque parser surfaces, a
**coverage-guided** fuzzer is far better — it instruments the binary and keeps
inputs that reach new code, discovering structure instead of guessing. We use
[`bolero`](https://crates.io/crates/bolero) so the **same** target runs two ways:

```sh
# As a normal (stable) property test — runs in CI:
cargo test --test fuzz_parsers

# Coverage-guided (needs nightly + the bolero CLI):
cargo install cargo-bolero
cargo +nightly bolero test parser_status_region -T 60sec    # libFuzzer, 60s
```

A coverage-guided run finds crashes/panics/hangs on its own; to catch *logic*
bugs it uses the same invariant assertions the property targets do. When it finds
something, minimize it (`cargo bolero` writes a corpus + crash file), drop the
input into the target's regression seeds, and fix.

## What is and isn't covered

The campaign went **deep on the write-path core** and the parser/forge/web seams,
but it is not exhaustive. The current coverage map, the known gaps (e.g.
`relink_scope=self/none`, `date_source=git`, config-precedence, forge CLI
orchestration), and the deferred defects are documented in
[`hardening-blitz-findings.md`](superpowers/hardening-blitz-findings.md) §4.

### Roadmap (in progress)

These are being added to close the highest-yield gaps; each lands as its own suite
+ a section here:

1. **Config/env precedence** — `tests/config_precedence.rs`: flag > env > `.env` >
   `config.yaml` > default.
2. **`relink_scope` in the oracle** — `all`/`self`/`none`, with the
   link-canonicality invariant conditioned on scope.
3. **Coverage-guided parser fuzzing** — `tests/fuzz_parsers.rs` (bolero). *(above)*
4. **`date_source=git`** — a git-backed oracle variant exercising the timeline
   reconstruction in `src/history.rs`.
5. **Forge CLI orchestration** — the `--forge` flows end-to-end against a mock
   HTTP server.
