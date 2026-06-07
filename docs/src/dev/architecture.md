# Architecture & conventions

How adroit is structured, and the rules a change is expected to preserve. These
aren't aspirational — the codebase already follows them; the point here is to keep
it that way. See also [Design Principles](./design.md) for the statelessness /
idempotency invariants.

## Layers

adroit is **one core library behind three surfaces**:

- **Core** (`adr`, `format`/`frontmatter`, `store`, `naming`, `links`,
  `query`/`view`, `config`, `template`, `history`, `lint`, `similar`) — the model,
  the on-disk read/write engine, and the shared read API. No terminal, no network.
- **Surfaces** — the CLI (`main.rs`, thin), the TUI (`tui`, feature-gated), and the
  web dashboard (`serve`, feature-gated). Each *consumes* the core; none
  reimplements it.
- **Integrations** — forge (HTTP issue/PR adapters) and AI (LLM adapters), each an
  opt-in feature behind an always-compiled facade.

The single read seam is `query`, returning the serde `view` types; every surface —
CLI `-o json`, the TUI panes, the web JSON API — renders the *same* computed
results, so a stat / search / graph is identical everywhere. Writes go through the
`Store` write path (CLI + TUI only); the query layer never writes.

## Seams: extend by adding a variant, not editing call sites

adroit is built so a new capability edits **one module + one match arm**. The gold
standard is the **naming seam** (`src/naming.rs`): every scheme behavior is a method
on `NamingScheme` / `AdrRef`, so adding a scheme (`sequential` / `date` / `uuid` /
`per_category`) edits only that module. The same shape holds for `Format`, `Layout`,
and the pluggable backends (forge provider, tracker, AI provider, publish target).

- Behavior that varies by an enum belongs as a **named method on that enum**, not an
  ad-hoc `match` / `== Variant` scattered across files.
- A new backend is a **trait impl + one factory arm** (`forge::open`,
  `ai_hook::open_provider`).

## Pure core, effectful shell

Transform logic is **pure, terminal/network/git-free** and unit-tested headlessly:
`format::*`, `links::*`, `naming::*`, `lint::lint`, `similar::rank`, `template::*`,
`history::parse_log`, the TUI's `TuiState` / `apply_action` layer, and the forge / AI
orchestration cores. Effects (filesystem, git, HTTP) live in the shell (`Store`,
`main`, the TUI driver). Push the decidable part into a pure function; keep the I/O
thin around it.

## Feature gating & confinement

A feature's heavy dependencies stay out of the lean `--no-default-features` build,
and `just lint-core` guards that the core never accidentally pulls in a surface. A
feature's `#[cfg(feature = …)]` is confined to three places: the `mod` line in
`lib.rs`, the **hook facade** (`forge_hook` / `ai_hook` — always compiled, with twin
real / no-op defs), and the CLI surface. Verb handlers call the facade
unconditionally and carry **no `#[cfg]`**.

> `ai` is the one feature whose module is intentionally *not* gated: its trait,
> value types, interview / compose logic, and `FakeProvider` are always compiled (so
> they're testable with no feature and no network, and `ai_hook` depends on them);
> only the rig-backed adapter inside is gated.

## Errors: typed in the core, anyhow in the shell

The data / parse layer (`adr` / `format` / `store` / `query` / `naming` / `links` /
`config` / `template` / `git`) exposes `thiserror` enums that compose with `#[from]`
— never stringify a typed cause. The binary and the feature-gated surfaces
(`serve`, `tui`) plus the forge / AI orchestration use `anyhow` (they
warn-and-continue across git + HTTP + fs); the pure parsers stay `anyhow`-free.

## Rust idioms

- **Newtypes** for domain ids (`AdrId` / `Number` / `Created` / `ReviewBy` /
  `AdrRef`) — no primitive obsession; `strum` for enum `Display` / `FromStr`.
- **Search before adding** (DRY): a recurring concern has a single owner — reuse
  `links::rel_link`, `StoreOptions::from_config`, the `query` / `view` layer, the
  naming seam — rather than re-deriving it.
- **Test / production separation** (hard rule): no `is_test` field or `cfg!(test)`
  branch in production logic. Use documented runtime env seams (`ADROIT_AI_FAKE`,
  `ADROIT_TODAY`), `#[cfg(test)]` helpers, and injected fakes (`FakeProvider`,
  `FakeTransport`).
- Slice strings on **char** boundaries; degrade fallible runtime paths gracefully
  (`Option` / `Result`) rather than `unwrap`.

See [Testing & Fuzzing](./testing.md) for how these invariants are enforced, and
[Hardening & Quality](./hardening.md) for where they tend to break.
