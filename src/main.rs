use anyhow::{Context, Result};
use clap::Parser;

use adroit::adr::{Number, Status};
use adroit::cli::{Cli, Command};
use adroit::config;
use adroit::store::Store;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = config::Config::load()?;
    config::bootstrap(&mut cfg);
    let dir = config::resolve_dir(cli.dir, &cfg);

    // Resolve editor before any I/O so we fail fast on misconfiguration.
    let editor = if matches!(cli.command, Some(Command::Edit { .. })) {
        Some(config::resolve_editor(&mut cfg)?)
    } else {
        None
    };

    let store = Store::open_or_create(&dir)?;

    match cli.command {
        Some(Command::New { title }) => {
            let mut adr = adroit::adr::Adr::new(&title)?;
            let path = store.write(&mut adr)?;
            println!("Created {}", path.display());
        }
        Some(Command::List) => {
            let adrs = store.list()?;
            if !adrs.is_empty() {
                println!("{:<6}{:<12}{:<12}Title", "#", "Status", "Created");
                for adr in adrs {
                    let num = adr.number.map(|n| n.to_string()).unwrap_or_default();
                    let created = adr.created.to_string();
                    let date = &created[..10];
                    println!("{:<6}{:<12}{:<12}{}", num, adr.status, date, adr.title);
                }
            }
        }
        Some(Command::Show { number }) => {
            let number = Number::new(number);
            let path = store.find_path_by_number(number)?;
            let adr = store.read(&path)?;
            println!("ADR {}: {}", number, adr.title);
            println!("Status:  {}", adr.status);
            println!("Created: {}", adr.created);
            println!("ID:      {}", adr.id);
            if !adr.body.is_empty() {
                println!();
                println!("{}", adr.body);
            }
        }
        Some(Command::Status { number, status }) => {
            let new_status: Status = status.parse().map_err(|_| {
                anyhow::anyhow!(
                    "invalid status '{status}', expected: proposed, accepted, deprecated, superseded"
                )
            })?;
            let number = Number::new(number);
            let path = store.find_path_by_number(number)?;
            let mut adr = store.read(&path)?;
            adr.status = new_status;
            store.write(&mut adr)?;
            println!("Updated ADR {} status to {}", number, new_status);
        }
        Some(Command::Edit { number }) => {
            let number = Number::new(number);
            let path = store.find_path_by_number(number)?;
            match editor.expect("resolved above") {
                Some(cmd) => spawn_editor(&cmd, &path)?,
                None => edit::edit_file(&path).context("editor failed")?,
            }
        }
        None => {
            adroit::tui::run()?;
        }
    }

    Ok(())
}

/// Spawn an explicit editor command (may include flags, e.g. `"code --wait"`).
fn spawn_editor(cmd: &str, path: &std::path::Path) -> Result<()> {
    let mut parts = cmd.split_whitespace();
    let bin = parts.next().expect("editor command is non-empty");
    let exit = std::process::Command::new(bin)
        .args(parts)
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch editor '{cmd}'"))?;
    if !exit.success() {
        anyhow::bail!("editor exited with {exit}");
    }
    Ok(())
}
