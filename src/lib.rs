pub mod adr;
// AI authoring. UNLIKE `forge`/`tui`/`web` (whose whole module is feature-gated
// below), `ai` is intentionally NOT gated: the trait, value types, FakeProvider,
// and interview/compose logic are always compiled — `ai_hook` (always on) and the
// `new --interview` path depend on them, and they're unit-testable with no `ai`
// feature and no network. Only the rig-backed adapter inside is gated (see
// `#[cfg(feature = "ai")] pub mod rig_provider;` in `ai/mod.rs`).
pub mod ai;
pub mod ai_hook;
pub mod cli;
pub mod config;
pub mod format;
// Forge adapters (HTTP) are gated behind the `forge` feature; the facade and the
// std-only git helpers are always compiled (the facade no-ops when forge is off).
#[cfg(feature = "forge")]
pub mod forge;
pub mod forge_hook;
pub mod frontmatter;
pub mod git;
pub mod history;
pub mod index;
pub mod links;
pub mod lint;
// Machine-readable CLI manifest for agents (issue 17); gated so a
// `--no-default-features` core drops `schemars`.
#[cfg(feature = "manifest")]
pub mod manifest;
pub mod naming;
pub mod publish;
pub mod query;
pub mod similar;
pub mod store;
pub mod template;
#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "web")]
pub mod serve;
pub mod view;
