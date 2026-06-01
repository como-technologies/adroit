use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::{Layout, MarkdownTheme};
use crate::format::Format;

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
    /// Regenerate the ADR section of SUMMARY.md (or print it to stdout).
    Index,
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
