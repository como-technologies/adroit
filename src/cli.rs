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
