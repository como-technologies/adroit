pub mod adr;
// AI authoring: the trait, value types, FakeProvider, and interview logic are
// always compiled; the rig-backed adapter is gated behind the `ai` feature (the
// facade falls back to None/the ADROIT_AI_FAKE seam when ai is off).
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
pub mod naming;
pub mod publish;
pub mod query;
pub mod store;
pub mod template;
#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "web")]
pub mod serve;
pub mod view;
