use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::{DateSource, Layout, MarkdownTheme};
use crate::format::Format;
use crate::naming::NamingScheme;

/// A snappy tool for managing Architecture Decision Records.
#[derive(Debug, Parser)]
#[command(name = "adroit", version, about)]
pub struct Cli {
    /// Path to the ADR directory. Overrides the config file and
    /// XDG data directory default (~/.local/share/adroit/).
    ///
    /// Also settable via the `ADROIT_DIR` environment variable (e.g. from a
    /// `.env` file), so you don't have to pass `--dir` on every command.
    #[arg(short, long, global = true, env = "ADROIT_DIR")]
    pub dir: Option<PathBuf>,

    /// On-disk format profile (overrides config): markdown or frontmatter.
    ///
    /// Also settable via `ADROIT_FORMAT`.
    #[arg(long, value_enum, global = true, env = "ADROIT_FORMAT")]
    pub format: Option<Format>,

    /// Directory layout (overrides config): by_status or flat.
    ///
    /// Also settable via `ADROIT_LAYOUT`.
    #[arg(long, value_enum, global = true, env = "ADROIT_LAYOUT")]
    pub layout: Option<Layout>,

    /// TUI markdown-preview color theme: default or gruvbox.
    ///
    /// Also settable via `ADROIT_THEME`.
    #[arg(long, value_enum, global = true, env = "ADROIT_THEME")]
    pub theme: Option<MarkdownTheme>,

    /// Days after which a still-Proposed ADR with no explicit `review_by` is
    /// flagged review-due (overrides config; `0` disables age-based flagging).
    ///
    /// Also settable via `ADROIT_REVIEW_OVERDUE_DAYS`.
    #[arg(long, global = true, env = "ADROIT_REVIEW_OVERDUE_DAYS")]
    pub review_overdue_days: Option<u32>,

    /// Default template for `new` — a built-in name (`madr`, `nygard`) or a path
    /// (overrides config). `new --template` still wins per-invocation.
    ///
    /// Also settable via `ADROIT_TEMPLATE`.
    #[arg(long, global = true, env = "ADROIT_TEMPLATE")]
    pub default_template: Option<String>,

    /// Where ADR dates/lifecycle come from: auto (git when available, else
    /// filesystem), git (require git; warn if unavailable/shallow), or
    /// filesystem (never shell git). Overrides config.
    ///
    /// Also settable via `ADROIT_DATE_SOURCE`.
    #[arg(long, value_enum, global = true, env = "ADROIT_DATE_SOURCE")]
    pub date_source: Option<DateSource>,

    /// How ADR identifiers/filenames are formed: sequential (NNNN, default),
    /// date (YYYYMMDD-title), uuid, or per_category. Overrides config.
    ///
    /// Also settable via `ADROIT_NAMING`.
    #[arg(long, value_enum, global = true, env = "ADROIT_NAMING")]
    pub naming: Option<NamingScheme>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new ADR.
    New {
        /// Title for the new decision record.
        title: String,
        /// Template name (madr, nygard) or a path to a custom template.
        #[arg(short, long)]
        template: Option<String>,
        /// Do not open the editor after creating the ADR.
        #[arg(long)]
        no_edit: bool,
    },
    /// List existing ADRs.
    List {
        /// Only show ADRs with this status.
        #[arg(short, long)]
        status: Option<String>,
    },
    /// Show a single ADR by number.
    Show {
        /// ADR number to display.
        number: u32,
    },
    /// Update the status of an ADR (moves the file in by_status layout).
    Status {
        /// ADR number to update.
        number: u32,
        /// New status (proposed, accepted, rejected, deprecated, superseded).
        status: String,
    },
    /// Mark an older ADR as superseded by a newer one.
    Supersede {
        /// The new (superseding) ADR number.
        new: u32,
        /// The old (superseded) ADR number.
        old: u32,
    },
    /// Set (or clear) an ADR's review deadline (ISO-8601 `YYYY-MM-DD`).
    ///
    /// A still-`Proposed` ADR whose deadline has passed is flagged review-due
    /// in stats and the dashboard. Pass `--clear` to remove the deadline.
    SetReview {
        /// ADR number to set the review deadline on.
        number: u32,
        /// Review deadline as `YYYY-MM-DD`. Omit together with `--clear`.
        #[arg(required_unless_present = "clear")]
        date: Option<String>,
        /// Remove the review deadline instead of setting one.
        #[arg(long, conflicts_with = "date")]
        clear: bool,
    },
    /// Search ADRs by title and body (case-insensitive).
    Search {
        /// Term to search for.
        term: String,
    },
    /// Validate the ADR repo and exit non-zero if any problem is found.
    ///
    /// A structural CI gate: checks for status/directory mismatches,
    /// duplicate numbers, unparseable files, broken supersession links, and
    /// broken/stale cross-ADR relative links.
    Check,
    /// Rewrite cross-ADR relative links to each ADR's current location.
    ///
    /// Fixes links left stale by status-change file moves (run by hand or in
    /// CI). Status changes already relink automatically; this repairs a repo
    /// edited outside adroit. Idempotent.
    Relink {
        /// Show which files/links would change without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Renumber an ADR (sequential scheme) to resolve a collision.
    ///
    /// Renames the file (slug preserved), rewrites its `# ADR-NNNN:` heading and
    /// every inbound reference (label + link), then relinks. `--file`
    /// disambiguates when two files share the old number.
    Renumber {
        /// Current ADR number.
        old: u32,
        /// New ADR number (must be unused).
        new: u32,
        /// The file to renumber, when two ADRs share `old`.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Convert the repo on disk to the configured layout/format.
    ///
    /// The source profile is auto-detected; files are moved between
    /// flat/by-status dirs and/or re-serialized markdown<->frontmatter, then
    /// cross-ADR links are fixed. Prints a preview by default — pass `--yes` to
    /// apply. Set the target via --layout / --format (or config / .env).
    Migrate {
        /// Apply the migration (default: preview only).
        #[arg(long)]
        yes: bool,
        /// Show what would change without writing (overrides `--yes`).
        #[arg(long)]
        dry_run: bool,
    },
    /// Regenerate the ADR section of SUMMARY.md (or print it to stdout).
    Index {
        /// Don't write — just verify SUMMARY.md is up to date (CI gate).
        ///
        /// Exits non-zero if SUMMARY.md differs from what `index` would write.
        #[arg(long)]
        check: bool,
    },
    /// Open an ADR in your editor ($EDITOR or $VISUAL).
    Edit {
        /// ADR number to edit.
        number: u32,
    },
    /// Generate a review-kickoff doc for an ADR (prints to stdout by default).
    Review {
        /// ADR number to generate a review kickoff for.
        number: u32,
        /// Review period length in business days (default: config `review_days`, 3).
        #[arg(long)]
        days: Option<u32>,
        /// Number of approvals required (default: config `review_quorum`, 3).
        #[arg(long)]
        quorum: Option<u32>,
        /// Write the generated doc to this path instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Serve the read-only web dashboard (requires the `web` feature).
    ///
    /// The subcommand always exists; without the `web` feature it prints a
    /// rebuild hint instead of starting a server (mirrors the TUI's handling).
    Serve {
        /// Host/address to bind (env: `ADROIT_HOST`).
        #[arg(long, default_value = "127.0.0.1", env = "ADROIT_HOST")]
        host: String,
        /// Port to bind (env: `ADROIT_PORT`).
        #[arg(long, default_value_t = 8080, env = "ADROIT_PORT")]
        port: u16,
    },
    /// Inspect or change configuration.
    ///
    /// `adroit config` (or `config show`) lists every setting with its resolved
    /// value and where it came from. `config get <key>` prints one value;
    /// `config set <key> <value>` persists to `config.yaml` (or the project
    /// `.env`, with `--local`).
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

/// Subcommands for `adroit config`.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigAction {
    /// List every setting with its resolved value and source (the default).
    Show,
    /// Print one setting's resolved value.
    Get {
        /// Config key (e.g. `layout`, `format`, `date_source`).
        key: String,
    },
    /// Set a value — writes `config.yaml`, or the project `.env` with `--local`.
    Set {
        /// Config key (e.g. `layout`, `format`, `date_source`).
        key: String,
        /// New value (validated against the key's type).
        value: String,
        /// Write `KEY=value` to the project `.env` instead of `config.yaml`.
        #[arg(long)]
        local: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_without_errors() {
        Cli::command().debug_assert();
    }
}
