use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A snappy TUI for managing Architecture Decision Records.
#[derive(Debug, Parser)]
#[command(name = "adroit", version, about)]
pub struct Cli {
    /// Path to the ADR directory. Overrides the config file and
    /// XDG data directory default (~/.local/share/adroit/).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new ADR.
    New {
        /// Title for the new decision record.
        title: String,
    },
    /// List existing ADRs.
    List,
    /// Show a single ADR by number.
    Show {
        /// ADR number to display.
        number: u32,
    },
    /// Update the status of an ADR.
    Status {
        /// ADR number to update.
        number: u32,
        /// New status (proposed, accepted, deprecated, superseded).
        status: String,
    },
    /// Open an ADR in your editor ($EDITOR or $VISUAL).
    Edit {
        /// ADR number to edit.
        number: u32,
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
