pub mod adr;
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
pub mod naming;
pub mod query;
pub mod store;
pub mod template;
#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "web")]
pub mod serve;
pub mod view;
