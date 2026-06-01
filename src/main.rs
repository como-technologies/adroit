use anyhow::{Context, Result};
use clap::Parser;

use adroit::adr::{Number, ReviewBy, Status};
use adroit::cli::{Cli, Command};
use adroit::config::{self, Config, Layout};
use adroit::format::Format;
use adroit::query::{self, Filter};
use adroit::store::{Store, StoreOptions};
use adroit::view::AdrSummary;

fn main() -> Result<()> {
    // Load a `.env` file (CWD or a parent) before parsing so `ADROIT_DIR` and
    // friends can be sourced from a file instead of passed on every command.
    // Real environment variables already set take precedence over the file.
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let mut cfg = config::Config::load()?;
    config::bootstrap(&mut cfg);
    // `--theme` / `ADROIT_THEME` overrides the config's TUI preview theme.
    if let Some(theme) = cli.theme {
        cfg.tui_theme = theme;
    }
    let dir = config::resolve_dir(cli.dir, &cfg);

    let opts = store_options(&cfg, cli.format, cli.layout);

    // Resolve editor before any I/O so we fail fast on misconfiguration.
    // Needed for `edit`, and for `new` unless --no-edit / open_on_new=false.
    let needs_editor = match &cli.command {
        Some(Command::Edit { .. }) => true,
        Some(Command::New { no_edit, .. }) => !no_edit && cfg.open_on_new,
        _ => false,
    };
    let editor = if needs_editor {
        Some(config::resolve_editor(&mut cfg)?)
    } else {
        None
    };

    let store = Store::open_or_create_with(&dir, opts)?;

    match cli.command {
        Some(Command::New {
            title,
            template,
            no_edit,
        }) => {
            let path = cmd_new(&store, &cfg, &title, template.as_deref())?;
            println!("Created {}", path.display());
            if cfg.open_on_new && !no_edit {
                open_in_editor(editor, &path)?;
            }
        }
        Some(Command::List { status }) => cmd_list(&store, status.as_deref())?,
        Some(Command::Show { number }) => cmd_show(&store, Number::new(number))?,
        Some(Command::Status { number, status }) => {
            let new_status: Status = status.parse().map_err(|_| {
                anyhow::anyhow!(
                    "invalid status '{status}', expected: proposed, accepted, rejected, deprecated, superseded"
                )
            })?;
            let number = Number::new(number);
            let path = store.set_status(number, new_status)?;
            println!(
                "Updated ADR {number} status to {new_status} ({})",
                path.display()
            );
        }
        Some(Command::Supersede { new, old }) => {
            cmd_supersede(&store, Number::new(new), Number::new(old))?;
        }
        Some(Command::SetReview {
            number,
            date,
            clear,
        }) => {
            cmd_set_review(&store, Number::new(number), date.as_deref(), clear)?;
        }
        Some(Command::Search { term }) => cmd_search(&store, &term)?,
        Some(Command::Index) => cmd_index(&store, &cfg)?,
        Some(Command::Edit { number }) => {
            let number = Number::new(number);
            let path = store.find_path_by_number(number)?;
            open_in_editor(editor, &path)?;
        }
        Some(Command::Review {
            number,
            days,
            quorum,
            output,
        }) => {
            cmd_review(
                &store,
                &cfg,
                Number::new(number),
                days,
                quorum,
                output.as_deref(),
            )?;
        }
        Some(Command::Serve { host, port }) => serve(&cfg, &dir, &host, port)?,
        None => run_tui(&cfg, &dir)?,
    }

    Ok(())
}

/// Launch the read-only web dashboard against the resolved ADR `dir` (honoring
/// `--dir`/config/`--format`/`--layout`). When built without the `web` feature,
/// print a rebuild hint instead (mirrors `run_tui`).
#[cfg(feature = "web")]
fn serve(cfg: &Config, dir: &std::path::Path, host: &str, port: u16) -> Result<()> {
    adroit::serve::run(cfg, dir, host, port)
}

#[cfg(not(feature = "web"))]
fn serve(_cfg: &Config, _dir: &std::path::Path, _host: &str, _port: u16) -> Result<()> {
    anyhow::bail!(
        "adroit was built without the `web` feature. \
         Rebuild with `--features web` (e.g. `cargo run --features web -- serve`)."
    );
}

/// Launch the interactive TUI (no subcommand) against the resolved ADR `dir`
/// (honoring `--dir`/config/`--format`/`--layout`), mirroring how `serve` is
/// threaded the resolved dir. When built without the `tui` feature, print a hint
/// instead so the binary still works.
#[cfg(feature = "tui")]
fn run_tui(cfg: &Config, dir: &std::path::Path) -> Result<()> {
    adroit::tui::run(cfg, dir)
}

#[cfg(not(feature = "tui"))]
fn run_tui(_cfg: &Config, _dir: &std::path::Path) -> Result<()> {
    println!(
        "adroit was built without the `tui` feature. \
         Rebuild with `--features tui`, or use the CLI subcommands (try `adroit --help`)."
    );
    Ok(())
}

/// Build store options from config, applying CLI overrides.
fn store_options(cfg: &Config, format: Option<Format>, layout: Option<Layout>) -> StoreOptions {
    let mut status_dir = std::collections::BTreeMap::new();
    for status in Status::ALL {
        status_dir.insert(status, cfg.dir_for(status));
    }
    StoreOptions {
        format: format.unwrap_or(cfg.format),
        layout: layout.unwrap_or(cfg.layout),
        status_dir,
    }
}

fn cmd_new(
    store: &Store,
    cfg: &Config,
    title: &str,
    template: Option<&str>,
) -> Result<std::path::PathBuf> {
    let mut adr = adroit::adr::Adr::new(title)?;
    adr.status = cfg.default_status;
    let number = store.next_number()?;
    adr.number = Some(number);

    if store.options().format == Format::Markdown {
        let name = template.unwrap_or(&cfg.default_template);
        let text = adroit::template::resolve(name, cfg.templates_dir.as_deref(), store.root())
            .with_context(|| format!("could not resolve template '{name}'"))?;
        let date = adr.created.to_string();
        let date = date.get(..10).unwrap_or(&date);
        adr.body = adroit::template::render(&text, number, title, cfg.default_status, date);
    }

    Ok(store.write(&mut adr)?)
}

fn cmd_list(store: &Store, status_filter: Option<&str>) -> Result<()> {
    let status: Option<Status> = match status_filter {
        Some(s) => Some(
            s.parse()
                .map_err(|_| anyhow::anyhow!("invalid status '{s}'"))?,
        ),
        None => None,
    };
    let filter = Filter {
        status,
        ..Default::default()
    };
    let rows = query::summaries(store, &filter)?;
    if rows.is_empty() {
        return Ok(());
    }
    println!("{:<8}{:<12}Title", "#", "Status");
    for row in &rows {
        print_summary_row(row);
    }
    Ok(())
}

fn cmd_show(store: &Store, number: Number) -> Result<()> {
    let path = store.find_path_by_number(number)?;
    let detail = query::detail(store, number.get())?;
    let s = &detail.summary;
    println!("ADR {number}: {}", s.title);
    println!("Status:  {}", s.status);
    for n in &s.supersedes {
        println!("Supersedes: ADR-{n:04}");
    }
    if let Some(n) = s.superseded_by {
        println!("Superseded by: ADR-{n:04}");
    }
    println!("Path:    {}", path.display());
    if !detail.body.is_empty() {
        println!();
        println!("{}", detail.body);
    }
    Ok(())
}

fn cmd_supersede(store: &Store, new: Number, old: Number) -> Result<()> {
    let old_path = store.supersede(new, old)?;
    // Add a reciprocal note to the new ADR referencing the old one.
    add_supersedes_note(store, new, old)?;
    println!(
        "ADR {old} superseded by ADR {new} (moved to {})",
        old_path.display()
    );
    Ok(())
}

/// Set or clear an ADR's `review_by` deadline (format-preserving).
fn cmd_set_review(store: &Store, number: Number, date: Option<&str>, clear: bool) -> Result<()> {
    let review_by = if clear {
        None
    } else {
        let raw = date.expect("clap requires a date unless --clear");
        Some(
            raw.parse::<ReviewBy>()
                .map_err(|e| anyhow::anyhow!("{e}"))?,
        )
    };
    let path = store.set_review_by(number, review_by)?;
    match review_by {
        Some(rb) => println!(
            "Set ADR {number} review deadline to {rb} ({})",
            path.display()
        ),
        None => println!("Cleared ADR {number} review deadline ({})", path.display()),
    }
    Ok(())
}

/// Append a "Supersedes ADR-<old>" note to the new ADR's body if not present.
fn add_supersedes_note(store: &Store, new: Number, old: Number) -> Result<()> {
    let path = store.find_path_by_number(new)?;
    let content = std::fs::read_to_string(&path)?;
    let marker = format!("Supersedes [ADR-{old}]");
    if content.contains(&marker) {
        return Ok(());
    }
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut updated = content.trim_end_matches(['\n', '\r']).to_string();
    updated.push_str(newline);
    updated.push_str(newline);
    // Relative link from new's dir to old's (now in superseded/).
    let old_path = store.find_path_by_number(old)?;
    let link = relative_link(&path, &old_path);
    updated.push_str(&format!("> Supersedes [ADR-{old}]({link})"));
    updated.push_str(newline);
    std::fs::write(&path, updated)?;
    Ok(())
}

/// Relative markdown link from one file to another (sibling-style).
fn relative_link(from_file: &std::path::Path, to_file: &std::path::Path) -> String {
    let from_dir = from_file.parent().unwrap_or(std::path::Path::new(""));
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = to_file.components().collect();
    let mut i = 0;
    while i < from.len() && i < to.len() && from[i] == to[i] {
        i += 1;
    }
    let ups = from.len() - i;
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    for c in &to[i..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

fn cmd_search(store: &Store, term: &str) -> Result<()> {
    let rows = query::search(store, term)?;
    for row in &rows {
        print_summary_row(row);
    }
    if rows.is_empty() {
        eprintln!("No ADRs matched '{term}'");
    }
    Ok(())
}

/// Render one `list` / `search` row. Shared so the two read commands stay
/// byte-identical. Uses the summary's zero-padded number display.
fn print_summary_row(row: &AdrSummary) {
    let num = row.number.map(|n| format!("{n:04}")).unwrap_or_default();
    println!("{:<8}{:<12}{}", num, row.status, row.title);
}

fn cmd_index(store: &Store, cfg: &Config) -> Result<()> {
    // Determine the SUMMARY.md path: config override, else discover next to the
    // ADR root (../SUMMARY.md is the usual mdBook layout).
    let summary = cfg
        .summary_path
        .clone()
        .or_else(|| discover_summary(store.root()));

    // Link prefix: how the ADR root is referenced from the SUMMARY file.
    let link_prefix = summary
        .as_deref()
        .and_then(|s| link_prefix_for(s, store.root()))
        .unwrap_or_else(|| "./adrs".to_string());

    match summary {
        Some(path) => {
            let updated = adroit::index::regenerate(store, &path, &link_prefix)?;
            println!("Updated {}", path.display());
            let _ = updated;
        }
        None => {
            let block = adroit::index::render_block(store, &link_prefix)?;
            println!("{block}");
        }
    }
    Ok(())
}

/// Generate a review-kickoff doc for an ADR. Pure generation — no git ops.
fn cmd_review(
    store: &Store,
    cfg: &Config,
    number: Number,
    days: Option<u32>,
    quorum: Option<u32>,
    output: Option<&std::path::Path>,
) -> Result<()> {
    use adroit::template;

    // Resolve the ADR via the store so it works in by_status mode and errors
    // cleanly when the number isn't found.
    let path = store.find_path_by_number(number)?;
    let adr = store.read(&path)?;

    let days = days.unwrap_or(cfg.review_days);
    let quorum = quorum.unwrap_or(cfg.review_quorum);

    // Today (local) as the review start date.
    let start = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date();
    let window = template::review_window(start, days);
    let date_iso = start
        .format(&time::format_description::well_known::Iso8601::DATE)
        .unwrap_or_else(|_| start.to_string());

    // Relative link to the ADR from the kickoff doc's expected location.
    // The kickoff lives alongside the ADR (same status dir), so a sibling link
    // to the file name is correct for the by_status layout.
    let adr_link = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let params = template::KickoffParams {
        number,
        title: &adr.title,
        date: &date_iso,
        adr_path: &adr_link,
        window,
        quorum,
    };
    let doc = template::render_kickoff(template::REVIEW_KICKOFF, &params);

    match output {
        Some(out) => {
            if let Some(parent) = out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not create directory {}", parent.display()))?;
            }
            std::fs::write(out, doc)
                .with_context(|| format!("could not write {}", out.display()))?;
            println!("Created {}", out.display());
        }
        None => print!("{doc}"),
    }
    Ok(())
}

/// Look for a SUMMARY.md alongside or one level above the ADR root.
fn discover_summary(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let candidates = [
        root.join("SUMMARY.md"),
        root.parent().map(|p| p.join("SUMMARY.md"))?,
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Compute the link prefix referencing `root` from the `summary` file's dir.
fn link_prefix_for(summary: &std::path::Path, root: &std::path::Path) -> Option<String> {
    let base = summary.parent()?;
    let rel = relative_link(&base.join("SUMMARY.md"), &root.join("x"));
    // rel ends with "/x"; strip it.
    let rel = rel.strip_suffix("/x").unwrap_or(&rel);
    if rel.is_empty() {
        Some(".".to_string())
    } else if rel.starts_with("..") {
        Some(rel.to_string())
    } else {
        Some(format!("./{rel}"))
    }
}

fn open_in_editor(editor: Option<Option<String>>, path: &std::path::Path) -> Result<()> {
    match editor.expect("editor resolved above") {
        Some(cmd) => spawn_editor(&cmd, path),
        None => edit::edit_file(path).context("editor failed"),
    }
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
