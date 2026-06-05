---
name: gate
description: Use before finishing or committing work on adroit — the full quality gate (fmt + clippy and tests across default/forge/web + book build), then commit only the specific files. STOPS before pushing. Invoke for "verify before commit", "is this ready to commit", "run the gate", or after any change.
user-invocable: true
---

# adroit ship gate

The pre-commit quality gate. Compose the global `verification-before-completion`
skill; this is adroit's concrete version. Run the **whole** gate — a fail-fast
pipeline hides everything after the first break (the lesson from a CI that died on
the first step).

## Gate — all must pass
```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --features forge --all-targets -- -D warnings
cargo clippy --features web --all-targets -- -D warnings
cargo test                       # default features
cargo test --features forge      # incl. forge_faults.rs, forge_cli.rs
cargo test --features web         # incl. the serve security tests
just book                         # the manual builds, no broken links
```
(`just ci` runs the full pipeline including `crate-audit`/`crate-outdated`.)

## Web feature: reproduce a clean checkout
`--features web` needs `web/dist/` to exist at compile time (rust-embed); it's kept
by the committed `web/dist/.gitkeep`. If you touched the web feature, confirm it
builds with **only** the `.gitkeep` present (move your local `web/dist` aside, then
restore) — CI has no Vue build, and a missing dir breaks the derive.

## New dependency? re-audit
Adding a crate can pull a transitive advisory — run `cargo audit` (exit 0, no
vulnerabilities) before relying on it.

## Commit
- Stage **only the specific files you changed** — NEVER `git add -A` (the tree may
  hold others' uncommitted work, e.g. a `web/` redesign).
- Run dependent steps sequentially: verify → THEN commit. A failing check must
  block the commit (never batch verify+commit in one parallel block).
- If behavior changed, update docs in the same change (`doc-sync`).

## STOP — do not push
**NEVER `git push` / force-push / open / merge / comment on a PR without the
user's explicit, in-the-moment permission.** A one-time "push"/"create a PR"
authorizes that one action only. Commit locally, say it's committed, and ask.
