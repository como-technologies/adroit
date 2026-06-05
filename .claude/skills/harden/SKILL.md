---
name: harden
description: Use to run a bug-hunting / hardening campaign on adroit — build or extend the model-based oracle or a property / fuzz / fault-injection harness, soak it, and turn every finding into a root-cause fix with a regression. Invoke for "find bugs in X", "harden X", "fuzz X", widening test coverage, or before a release.
user-invocable: true
---

# Harden adroit

Run a bug-hunting campaign. Compose the global `systematic-debugging` and
`test-driven-development` skills; this skill adds adroit's machinery. Full
reference: the book's **Development → Testing & Fuzzing** (`docs/src/dev/testing.md`)
and **Hardening & Quality** (`docs/src/dev/hardening.md`).

## 1. Pick the surface + modality
- Core write-path invariants → extend the **oracle** (`tests/model.rs`).
- A pure parser / seam → a **property** in `tests/parsers.rs` + a bolero target in
  `tests/fuzz_parsers.rs` (coverage-guided).
- Forge HTTP adapters → `tests/forge_faults.rs`; forge `--forge` CLI flows →
  `tests/forge_cli.rs`.
- A config / setting axis (precedence, a flag, git history) → a focused harness
  (see `tests/config_precedence.rs`, `tests/date_source_git.rs`).

## 2. Design principle — do not violate
The oracle is an **outcome predictor, NOT a reimplementation** of adroit. It drives
the **real binary** on a `TempDir` and predicts only *observable* results (files
present, parsed fields, `check` output, link-canonicality). For non-deterministic
identity (uuid; date dedup) it **reads the assigned id back** from disk. Keep it
git-free (`date_source=filesystem`) and pin the clock (`ADROIT_TODAY`). Invariant
strength must match intent — e.g. link-canonicality is per-command under
`relink_scope=all`, but only converges after an explicit `relink` under `self/none`.

## 3. Soak
`PROPTEST_CASES=N just model` (or `cargo test --test <suite>`). The CI budget is
small; soak at 1500–5000 to find new things. Committed
`tests/*.proptest-regressions` seeds replay first, so a found bug can't silently
return. For deep parser coverage, `cargo +nightly bolero test <target>`.

## 4. Triage every finding (explore → triage → crystallize)
Reproduce the **minimal failing input** by hand on `mktemp -d` with the same
`--format/--layout/--naming/...` flags; inspect the files. Then classify:
- **Real bug** → write a focused *red* regression in `tests/cli.rs`, fix the
  production code at root cause, go *green*.
- **Intended behavior change** → update the *model* in `tests/model.rs` (it encodes
  the semantics), not the code.
- **Model / harness gap** → fix the oracle; gate a known-deferred bug with a
  documented skip.
Always keep the committed regression seed.

## 5. Finish
Run `gate` (the ship gate). Record each finding (bug → fix → regression) on the
**Hardening & Quality** page. `doc-sync` any behavior change. Then **ask before
pushing**.

## Where bugs hide in adroit (learned the hard way)
- **Scheme-agnostic resolution** — numeric-only logic breaks date/uuid/per_category;
  route link/identity resolution through the naming seam (`ref_in_link_from`).
- **Canonical links** — use `links::rel_link` (it adds the same-dir `./`); never an
  ad-hoc relative-path helper. A "relink is always a no-op" invariant catches this
  class.
- **Format divergence** — markdown vs frontmatter differ on supersession; frontmatter
  is sequential-only.
- **Hostile input** — char-boundary slicing, raw HTML in the dashboard render, lone
  `\r` in the rewriters.
