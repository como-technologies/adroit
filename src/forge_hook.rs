//! Always-compiled facade over the (feature-gated) [`crate::forge`] integration.
//!
//! This is the single boundary — besides the `mod` line in `lib.rs` — where the
//! `forge` feature's `#[cfg]` lives. Verbs (`cmd_new`, …) call these functions
//! unconditionally; when the feature is off the bodies are no-ops, so the main
//! paths carry no `#[cfg]` and no `if forge_enabled`.

use std::path::Path;

use anyhow::Result;

use crate::adr::Status;
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
pub fn after_new(cfg: &Config, path: &Path, title: &str, flags: ForgeFlags) -> Result<()> {
    if !flags.enabled {
        return Ok(());
    }
    crate::forge::after_new(cfg, path, title, flags.dry_run)
}

/// No-op stub when built without the `forge` feature — warns if the user asked
/// for forge so the silence isn't mysterious.
#[cfg(not(feature = "forge"))]
pub fn after_new(_cfg: &Config, _path: &Path, _title: &str, flags: ForgeFlags) -> Result<()> {
    if flags.enabled {
        warn_no_feature();
    }
    Ok(())
}

/// Forge actions before a `set-status` move (verify + merge/close). Returns
/// `true` to proceed with the local move, `false` to stop (preview / not
/// `--yes`); `Err` aborts (e.g. an unapproved `accepted`).
#[cfg(feature = "forge")]
pub fn before_status_change(
    cfg: &Config,
    path: &Path,
    new_status: Status,
    flags: ForgeFlags,
) -> Result<bool> {
    if !flags.enabled {
        return Ok(true);
    }
    crate::forge::before_status_change(cfg, path, new_status, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn before_status_change(
    _cfg: &Config,
    _path: &Path,
    _new_status: Status,
    flags: ForgeFlags,
) -> Result<bool> {
    if flags.enabled {
        warn_no_feature();
    }
    Ok(true)
}

/// Forge actions before a `supersede` (comment + close the old ADR's issue/PR).
#[cfg(feature = "forge")]
pub fn on_supersede(
    cfg: &Config,
    old_path: &Path,
    new_label: &str,
    flags: ForgeFlags,
) -> Result<bool> {
    if !flags.enabled {
        return Ok(true);
    }
    crate::forge::on_supersede(cfg, old_path, new_label, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn on_supersede(
    _cfg: &Config,
    _old_path: &Path,
    _new_label: &str,
    flags: ForgeFlags,
) -> Result<bool> {
    if flags.enabled {
        warn_no_feature();
    }
    Ok(true)
}

/// Forge-aware `check` problems (issue/PR drift). Empty unless enabled + built.
#[cfg(feature = "forge")]
pub fn check_repo(
    cfg: &Config,
    entries: &[(std::path::PathBuf, crate::adr::Adr)],
    enabled: bool,
) -> Result<Vec<crate::view::Problem>> {
    if !enabled {
        return Ok(Vec::new());
    }
    crate::forge::check_repo(cfg, entries)
}

#[cfg(not(feature = "forge"))]
pub fn check_repo(
    _cfg: &Config,
    _entries: &[(std::path::PathBuf, crate::adr::Adr)],
    enabled: bool,
) -> Result<Vec<crate::view::Problem>> {
    if enabled {
        warn_no_feature();
    }
    Ok(Vec::new())
}

/// Attach live forge state to list/dashboard rows. No-op unless enabled + built.
#[cfg(feature = "forge")]
pub fn enrich(
    cfg: &Config,
    store: &crate::store::Store,
    summaries: &mut [crate::view::AdrSummary],
    enabled: bool,
) -> Result<()> {
    if !enabled {
        return Ok(());
    }
    crate::forge::enrich(cfg, store, summaries)
}

#[cfg(not(feature = "forge"))]
pub fn enrich(
    _cfg: &Config,
    _store: &crate::store::Store,
    _summaries: &mut [crate::view::AdrSummary],
    enabled: bool,
) -> Result<()> {
    if enabled {
        warn_no_feature();
    }
    Ok(())
}

/// Shared "you asked for --with-forge but this build lacks it" notice.
#[cfg(not(feature = "forge"))]
fn warn_no_feature() {
    eprintln!(
        "adroit: --with-forge ignored — this build lacks the `forge` feature \
         (rebuild with `--features forge`)"
    );
}
