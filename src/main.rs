use anyhow::Result;
use clap::Parser;

use adroit::cli::{Cli, Command};
use adroit::store::Store;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => {
            let store = Store::init(&cli.dir)?;
            println!("Initialized ADR directory at {}", store.root().display());
        }
        Some(Command::New { title }) => {
            let store = Store::open(&cli.dir)?;
            let mut adr = adroit::adr::Adr::new(&title)?;
            let path = store.write(&mut adr)?;
            println!("Created {}", path.display());
        }
        Some(Command::List) => {
            let store = Store::open(&cli.dir)?;
            for path in store.list_files()? {
                println!("{}", path.display());
            }
        }
        None => {
            adroit::tui::run()?;
        }
    }

    Ok(())
}
