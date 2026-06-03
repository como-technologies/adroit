//! Always-compiled facade over the (feature-gated) [`crate::forge`] integration.
//!
//! This is the single boundary — besides the `mod` line in `lib.rs` — where the
//! `forge` feature's `#[cfg]` lives. Verbs (`cmd_new`, …) call these functions
//! unconditionally; when the feature is off the bodies are no-ops, so the main
//! paths carry no `#[cfg]` and no `if forge_enabled`.

use std::path::Path;

use anyhow::Result;

use crate::config::Config;

/// Flags from `adroit new` that drive the forge hook (mirrors `migrate`'s
/// dry-run/apply UX).
#[derive(Debug, Clone, Copy, Default)]
pub struct ForgeFlags {
    /// `--with-forge` was passed (opt-in; the hook is otherwise a no-op).
    pub enabled: bool,
    /// `--dry-run`: print the plan, touch nothing remote or on disk.
    pub dry_run: bool,
    /// `--yes`: apply without the confirmation gate.
    pub yes: bool,
}

/// Fired after `adroit new` writes the ADR: create the tracker issue + draft
/// PR and record their URLs in `## References`. A no-op unless `--with-forge`
/// is set *and* the binary was built with the `forge` feature.
#[cfg(feature = "forge")]
pub fn after_new(cfg: &Config, path: &Path, flags: ForgeFlags) -> Result<()> {
    if !flags.enabled {
        return Ok(());
    }
    crate::forge::after_new(cfg, path, flags.dry_run, flags.yes)
}

/// No-op stub when built without the `forge` feature — warns if the user asked
/// for forge so the silence isn't mysterious.
#[cfg(not(feature = "forge"))]
pub fn after_new(_cfg: &Config, _path: &Path, flags: ForgeFlags) -> Result<()> {
    if flags.enabled {
        eprintln!(
            "adroit: --with-forge ignored — this build lacks the `forge` feature \
             (rebuild with `--features forge`)"
        );
    }
    Ok(())
}
