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
    /// `--forge` was passed (opt-in; the hook is otherwise a no-op).
    pub enabled: bool,
    /// `--dry-run`: print the plan, touch nothing remote or on disk.
    pub dry_run: bool,
    /// `--yes`: apply without the confirmation gate.
    pub yes: bool,
}

/// Fired after `adroit new` writes the ADR: create the tracker issue + draft
/// PR and record their URLs in `## References`. A no-op unless `--forge`
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

/// Forge actions after a `set-status` move: on an applied `accepted`, commit the
/// `proposed/ → accepted/` move + relink and push it to the base branch (#4's
/// "push the relink commit"). No-op unless `--forge` + the feature build.
#[cfg(feature = "forge")]
pub fn after_status_change(
    cfg: &Config,
    new_path: &Path,
    new_status: Status,
    flags: ForgeFlags,
) -> Result<()> {
    if !flags.enabled {
        return Ok(());
    }
    crate::forge::after_status_change(cfg, new_path, new_status, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn after_status_change(
    _cfg: &Config,
    _new_path: &Path,
    _new_status: Status,
    _flags: ForgeFlags,
) -> Result<()> {
    Ok(())
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

/// Enrich a single summary in place with live forge state (issue/PR links + PR
/// approvals/CI/merged) for the read-only `serve` dashboard panel. A no-op (the
/// summary's `forge_data` stays `None`) unless built with `forge` *and* a forge
/// provider is configured. Read-only: it never writes to the forge.
#[cfg(feature = "forge")]
pub fn enrich_one(
    forge: Option<&crate::config::ForgeConfig>,
    store: &crate::store::Store,
    summary: &mut crate::view::AdrSummary,
) -> Result<()> {
    let Some(fcfg) = forge else {
        return Ok(());
    };
    crate::forge::enrich_with(fcfg, store, std::slice::from_mut(summary))
}

#[cfg(not(feature = "forge"))]
pub fn enrich_one(
    _forge: Option<&crate::config::ForgeConfig>,
    _store: &crate::store::Store,
    _summary: &mut crate::view::AdrSummary,
) -> Result<()> {
    Ok(())
}

/// Dashboard forge tiles: `(proposed_without_pr, approved_unmerged)`, or `None`
/// when no provider is configured / built (so the dashboard hides the tiles).
#[cfg(feature = "forge")]
pub fn dashboard_summary(
    forge: Option<&crate::config::ForgeConfig>,
    store: &crate::store::Store,
    summaries: &[crate::view::AdrSummary],
    quorum: u32,
) -> Result<Option<(u32, u32)>> {
    let Some(fcfg) = forge else {
        return Ok(None);
    };
    crate::forge::dashboard_summary(fcfg, store, summaries, quorum)
}

#[cfg(not(feature = "forge"))]
pub fn dashboard_summary(
    _forge: Option<&crate::config::ForgeConfig>,
    _store: &crate::store::Store,
    _summaries: &[crate::view::AdrSummary],
    _quorum: u32,
) -> Result<Option<(u32, u32)>> {
    Ok(None)
}

/// `review --forge`: post the kickoff on the linked issue/PR with the reviewer
/// pool @-mentioned, and tag the PR with a `review-by:<deadline>` label.
#[cfg(feature = "forge")]
pub fn review_kickoff(
    cfg: &Config,
    path: &Path,
    body: &str,
    deadline: &str,
    flags: ForgeFlags,
) -> Result<()> {
    if !flags.enabled {
        return Ok(());
    }
    crate::forge::review_kickoff(cfg, path, body, deadline, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn review_kickoff(
    _cfg: &Config,
    _path: &Path,
    _body: &str,
    _deadline: &str,
    flags: ForgeFlags,
) -> Result<()> {
    if flags.enabled {
        warn_no_feature();
    }
    Ok(())
}

/// `set-review --forge`: comment the deadline on the linked issue/PR **and** set
/// the tracker's native due/target date (`date`, `None` clears).
#[cfg(feature = "forge")]
pub fn set_review_deadline(
    cfg: &Config,
    path: &Path,
    note: &str,
    date: Option<&str>,
    flags: ForgeFlags,
) -> Result<()> {
    if !flags.enabled {
        return Ok(());
    }
    crate::forge::set_review_deadline(cfg, path, note, date, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn set_review_deadline(
    _cfg: &Config,
    _path: &Path,
    _note: &str,
    _date: Option<&str>,
    flags: ForgeFlags,
) -> Result<()> {
    if flags.enabled {
        warn_no_feature();
    }
    Ok(())
}

/// Refresh an ADR's linked PR description from its content (sync / relink-patch).
#[cfg(feature = "forge")]
pub fn sync_pr(cfg: &Config, path: &Path, flags: ForgeFlags) -> Result<bool> {
    if !flags.enabled {
        return Ok(true);
    }
    crate::forge::sync_pr(cfg, path, flags.dry_run, flags.yes)
}

#[cfg(not(feature = "forge"))]
pub fn sync_pr(_cfg: &Config, _path: &Path, flags: ForgeFlags) -> Result<bool> {
    if flags.enabled {
        eprintln!(
            "adroit: refreshing a PR description needs the `forge` feature \
             (rebuild with `--features forge`)"
        );
    }
    Ok(true)
}

/// Post a chat notification to `webhook`. `dry_run` prints the message instead.
/// Returns `true` only when the message was actually delivered (so the caller
/// doesn't claim success on a dry run, a failed webhook, or a no-forge build).
// Only ever called by the forge-gated `notify` command, so it exists solely in
// forge builds (no no-op twin needed).
#[cfg(feature = "forge")]
pub fn notify(webhook: &str, text: &str, dry_run: bool) -> Result<bool> {
    if dry_run {
        println!("Would post to webhook:\n{text}");
        return Ok(false);
    }
    crate::forge::notify(webhook, text)
}

/// Shared "you asked for --forge but this build lacks it" notice.
#[cfg(not(feature = "forge"))]
fn warn_no_feature() {
    eprintln!(
        "adroit: --forge ignored — this build lacks the `forge` feature \
         (rebuild with `--features forge`)"
    );
}
