use anyhow::Result;
use clap::Parser;

use adroit::cli::{Cli, Command};
use adroit::config;
use adroit::store::Store;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::Config::load()?;
    let dir = config::resolve_dir(cli.dir, &cfg);
    let store = Store::open_or_create(&dir)?;

    match cli.command {
        Some(Command::New { title }) => {
            let mut adr = adroit::adr::Adr::new(&title)?;
            let path = store.write(&mut adr)?;
            println!("Created {}", path.display());
        }
        Some(Command::List) => {
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
