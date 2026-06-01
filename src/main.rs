use anyhow::{Context, Result};
use clap::Parser;

use adroit::adr::{Number, ReviewBy, Status};
use adroit::cli::{Cli, Command, ConfigAction};
use adroit::config::{self, Config, Layout};
use adroit::format::Format;
use adroit::naming::AdrRef;
use adroit::query::{self, Filter};
use adroit::store::{Store, StoreOptions};
use adroit::view::AdrSummary;

/// Parse a CLI ADR identifier into an [`AdrRef`] under the configured scheme.
fn resolve_ref(cfg: &Config, id: &str) -> Result<AdrRef> {
    cfg.naming.parse_ref(id).ok_or_else(|| {
        anyhow::anyhow!(
            "'{id}' is not a valid ADR identifier for the {} naming scheme",
            cfg.naming
        )
    })
}

/// The duplicate-detection / existence key for an ADR identity, so `cmd_check`
/// groups and looks ADRs up uniformly across schemes.
fn ident_key(r: &AdrRef) -> String {
    match r {
        AdrRef::Number(n) => format!("n:{n}"),
        AdrRef::Slug(s) => format!("s:{s}"),
    }
}

/// Bail when a numeric-only command (`renumber`/`review`, whose artifacts are
/// number-shaped) runs under a non-numeric naming scheme.
fn require_numeric_scheme(cfg: &Config, command: &str) -> Result<()> {
    if cfg.naming.is_numeric() {
        Ok(())
    } else {
        anyhow::bail!(
            "`{command}` requires a numeric naming scheme; the configured scheme is `{}`",
            cfg.naming
        )
    }
}

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
    // `--review-overdue-days` / `ADROIT_REVIEW_OVERDUE_DAYS` overrides the
    // config's review-due staleness threshold (applies to every surface, since
    // each builds its store options from this resolved `cfg`).
    if let Some(days) = cli.review_overdue_days {
        cfg.review_overdue_days = days;
    }
    // `--default-template` / `ADROIT_TEMPLATE` overrides the config's default
    // template for `new` (a per-invocation `new --template` still wins).
    if let Some(template) = &cli.default_template {
        cfg.default_template = template.clone();
    }
    // `--date-source` / `ADROIT_DATE_SOURCE` overrides where dates come from.
    if let Some(source) = cli.date_source {
        cfg.date_source = source;
    }
    // `--naming` / `ADROIT_NAMING` overrides the identifier/filename scheme.
    if let Some(naming) = cli.naming {
        cfg.naming = naming;
    }

    // `config` operates on configuration, not ADRs — handle it before resolving
    // a dir or opening a store, so it works even on a profile-mismatched repo.
    if let Some(Command::Config { action }) = &cli.command {
        return cmd_config(action.as_ref(), &cli);
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

    // Refuse to operate on a repo whose on-disk layout/format doesn't match the
    // configured one — it would silently hide ADRs or corrupt numbering.
    // `migrate` is the conversion path, so it's exempt.
    if !matches!(cli.command, Some(Command::Migrate { .. }))
        && let Some(msg) = store.profile_mismatch()
    {
        anyhow::bail!("{msg}");
    }

    match cli.command {
        Some(Command::New {
            title,
            template,
            no_edit,
            category,
        }) => {
            let path = cmd_new(
                &store,
                &cfg,
                &title,
                template.as_deref(),
                category.as_deref(),
            )?;
            println!("Created {}", path.display());
            if cfg.open_on_new && !no_edit {
                open_in_editor(editor, &path)?;
            }
        }
        Some(Command::List { status }) => cmd_list(&store, status.as_deref())?,
        Some(Command::Show { id }) => cmd_show(&store, &resolve_ref(&cfg, &id)?)?,
        Some(Command::Status { id, status }) => {
            let new_status: Status = status.parse().map_err(|_| {
                anyhow::anyhow!(
                    "invalid status '{status}', expected: proposed, accepted, rejected, deprecated, superseded"
                )
            })?;
            let r = resolve_ref(&cfg, &id)?;
            let path = store.set_status_ref(&r, new_status)?;
            println!(
                "Updated {} status to {new_status} ({})",
                cfg.naming.display(&r),
                path.display()
            );
        }
        Some(Command::Supersede { new, old }) => {
            cmd_supersede(
                &store,
                &cfg,
                &resolve_ref(&cfg, &new)?,
                &resolve_ref(&cfg, &old)?,
            )?;
        }
        Some(Command::SetReview { id, date, clear }) => {
            cmd_set_review(
                &store,
                &cfg,
                &resolve_ref(&cfg, &id)?,
                date.as_deref(),
                clear,
            )?;
        }
        Some(Command::Search { term }) => cmd_search(&store, &term)?,
        Some(Command::Check) => cmd_check(&store)?,
        Some(Command::Relink { dry_run }) => cmd_relink(&store, dry_run)?,
        Some(Command::Renumber { old, new, file }) => {
            require_numeric_scheme(&cfg, "renumber")?;
            cmd_renumber(&store, Number::new(old), Number::new(new), file.as_deref())?;
        }
        Some(Command::Migrate { yes, dry_run }) => cmd_migrate(&store, yes, dry_run)?,
        Some(Command::Index { check }) => cmd_index(&store, &cfg, check)?,
        Some(Command::Edit { id }) => {
            let path = store.find_path_by_ref(&resolve_ref(&cfg, &id)?)?;
            open_in_editor(editor, &path)?;
        }
        Some(Command::Review {
            number,
            days,
            quorum,
            output,
        }) => {
            require_numeric_scheme(&cfg, "review")?;
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
        // `config` returns before the store is opened (see above).
        Some(Command::Config { .. }) => unreachable!("config handled before store open"),
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
        review_overdue_days: (cfg.review_overdue_days > 0).then_some(cfg.review_overdue_days),
        date_source: cfg.date_source,
        naming: cfg.naming,
    }
}

fn cmd_new(
    store: &Store,
    cfg: &Config,
    title: &str,
    template: Option<&str>,
    category: Option<&str>,
) -> Result<std::path::PathBuf> {
    let mut adr = adroit::adr::Adr::new(title)?;
    adr.status = cfg.default_status;
    // Assign the identity up front (so the heading renders correctly), via the
    // configured naming scheme; `store.write` then reuses it. Under by_category
    // the directory is the area, so a `--category` is required and the number is
    // local to it.
    let r = if store.options().layout == Layout::ByCategory {
        let category = category.ok_or_else(|| {
            anyhow::anyhow!("the by_category layout requires `--category <name>`")
        })?;
        adr.category = Some(category.to_string());
        store.next_ref_in_category(category)
    } else {
        if category.is_some() {
            anyhow::bail!("`--category` only applies to the by_category layout");
        }
        store.next_ref(title, adr.id.uuid())?
    };
    adroit::store::apply_ref_pub(&mut adr, &r);

    if store.options().format == Format::Markdown {
        let name = template.unwrap_or(&cfg.default_template);
        let text = adroit::template::resolve(name, cfg.templates_dir.as_deref(), store.root())
            .with_context(|| format!("could not resolve template '{name}'"))?;
        let date = adr.created.to_string();
        let date = date.get(..10).unwrap_or(&date);
        adr.body = adroit::template::render(&text, cfg.naming, &r, title, cfg.default_status, date);
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
    let id_w = id_col_width(&rows);
    println!("{:<id_w$}{:<12}Title", "#", "Status");
    for row in &rows {
        print_summary_row(row, id_w);
    }
    Ok(())
}

fn cmd_show(store: &Store, r: &AdrRef) -> Result<()> {
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    let s = &detail.summary;
    println!("{}: {}", s.reference, s.title);
    println!("Status:  {}", s.status);
    if let Some(c) = &s.created {
        println!("Created: {}", ymd(c));
    }
    if let Some(lm) = &detail.last_modified {
        println!("Updated: {}", ymd(lm));
    }
    for r in &s.supersedes {
        println!("Supersedes: {r}");
    }
    if let Some(r) = &s.superseded_by {
        println!("Superseded by: {r}");
    }
    println!("Path:    {}", path.display());
    // Git-derived lifecycle (proposed → accepted/rejected/…). Empty outside git.
    if !detail.history.is_empty() {
        println!();
        println!("History:");
        for e in &detail.history {
            println!("  {}  {:<10}  {}", ymd(&e.date), e.label, e.subject);
        }
    }
    if !detail.body.is_empty() {
        println!();
        println!("{}", detail.body);
    }
    Ok(())
}

/// Trim an RFC 3339 timestamp to its `YYYY-MM-DD` date for terse display.
fn ymd(iso: &str) -> &str {
    iso.get(..10).unwrap_or(iso)
}

fn cmd_supersede(store: &Store, cfg: &Config, new: &AdrRef, old: &AdrRef) -> Result<()> {
    let old_path = store.supersede(new, old)?;
    // Add a reciprocal note to the new ADR referencing the old one.
    add_supersedes_note(store, cfg, new, old)?;
    println!(
        "{} superseded by {} (moved to {})",
        cfg.naming.display(old),
        cfg.naming.display(new),
        old_path.display()
    );
    Ok(())
}

/// Set or clear an ADR's `review_by` deadline (format-preserving).
fn cmd_set_review(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    date: Option<&str>,
    clear: bool,
) -> Result<()> {
    let review_by = if clear {
        None
    } else {
        let raw = date.expect("clap requires a date unless --clear");
        Some(
            raw.parse::<ReviewBy>()
                .map_err(|e| anyhow::anyhow!("{e}"))?,
        )
    };
    let path = store.set_review_by_ref(r, review_by)?;
    let id = cfg.naming.display(r);
    match review_by {
        Some(rb) => println!("Set {id} review deadline to {rb} ({})", path.display()),
        None => println!("Cleared {id} review deadline ({})", path.display()),
    }
    Ok(())
}

/// Append a "Supersedes [<old>](...)" note to the new ADR's body if not present.
fn add_supersedes_note(store: &Store, cfg: &Config, new: &AdrRef, old: &AdrRef) -> Result<()> {
    let path = store.find_path_by_ref(new)?;
    let content = std::fs::read_to_string(&path)?;
    let old_label = cfg.naming.link_label(old);
    let marker = format!("Supersedes [{old_label}]");
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
    let old_path = store.find_path_by_ref(old)?;
    let link = relative_link(&path, &old_path);
    updated.push_str(&format!("> Supersedes [{old_label}]({link})"));
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
    let id_w = id_col_width(&rows);
    for row in &rows {
        print_summary_row(row, id_w);
    }
    if rows.is_empty() {
        eprintln!("No ADRs matched '{term}'");
    }
    Ok(())
}

/// The identifier shown in a `list`/`search` row: the zero-padded number for
/// numeric schemes (unchanged), the scheme's reference for date/uuid.
fn row_id(row: &AdrSummary) -> &str {
    if row.number.is_some() {
        &row.number_display
    } else {
        &row.reference
    }
}

/// Width of the identifier column: at least 8 (so sequential output is
/// byte-identical — its ids are ≤4 chars), else the longest id + a 2-space gap
/// so a long slug/uuid never abuts the Status column.
fn id_col_width(rows: &[AdrSummary]) -> usize {
    let longest = rows.iter().map(|r| row_id(r).len()).max().unwrap_or(0);
    (longest + 2).max(8)
}

/// Render one `list` / `search` row. Shared so the two read commands stay
/// byte-identical. `id_w` is the (dynamic) identifier column width.
fn print_summary_row(row: &AdrSummary, id_w: usize) {
    println!("{:<id_w$}{:<12}{}", row_id(row), row.status, row.title);
}

fn cmd_index(store: &Store, cfg: &Config, check: bool) -> Result<()> {
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

    if check {
        // CI gate: never write. Compare what `regenerate` WOULD produce against
        // the on-disk SUMMARY.md and exit non-zero if they differ.
        return cmd_index_check(store, summary.as_deref(), &link_prefix);
    }

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

/// `index --check`: verify SUMMARY.md is up to date without writing.
///
/// When no SUMMARY.md is found we print a note and exit 0 — not every repo
/// publishes one, and failing CI for its absence would be surprising.
fn cmd_index_check(
    store: &Store,
    summary: Option<&std::path::Path>,
    link_prefix: &str,
) -> Result<()> {
    let Some(path) = summary else {
        println!("No SUMMARY.md found — nothing to check.");
        return Ok(());
    };
    let existing = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let block = adroit::index::render_block(store, link_prefix)?;
    let expected = adroit::index::splice(&existing, &block);
    if expected == existing {
        println!("SUMMARY.md is up to date ({})", path.display());
        Ok(())
    } else {
        anyhow::bail!(
            "SUMMARY.md is out of date — run `adroit index` ({})",
            path.display()
        );
    }
}

/// `adroit config`: show / get / set configuration.
fn cmd_config(action: Option<&ConfigAction>, cli: &Cli) -> Result<()> {
    match action {
        None | Some(ConfigAction::Show) => config_show(cli),
        Some(ConfigAction::Get { key }) => config_get(cli, key),
        Some(ConfigAction::Set { key, value, local }) => config_set(key, value, *local),
    }
}

/// The flag/env value clap captured for a config `key` (if that key has one).
fn config_cli_value(cli: &Cli, key: &str) -> Option<String> {
    match key {
        "dir" => cli.dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "format" => cli.format.map(|f| f.to_string()),
        "layout" => cli.layout.map(|l| l.to_string()),
        "tui_theme" => cli.theme.map(|t| t.to_string()),
        "default_template" => cli.default_template.clone(),
        "review_overdue_days" => cli.review_overdue_days.map(|n| n.to_string()),
        "date_source" => cli.date_source.map(|d| d.to_string()),
        _ => None,
    }
}

/// Effective value of `key`: a flag/env override wins, then the config file /
/// built-in default (with `dir` resolved to its computed default).
fn config_effective(cli: &Cli, cfg: &Config, key: &str) -> String {
    if let Some(v) = config_cli_value(cli, key) {
        return v;
    }
    if key == "dir" {
        return config::resolve_dir(None, cfg)
            .to_string_lossy()
            .into_owned();
    }
    cfg.get_str(key).unwrap_or_else(|| "(unset)".to_string())
}

/// Where `key`'s effective value came from, by precedence. A flag and an env
/// var can both be set (a flag wins); we tell them apart by comparing the env
/// var's value to the value clap actually resolved.
fn config_source(cli: &Cli, key: &str, in_file: bool) -> &'static str {
    match config_cli_value(cli, key) {
        Some(v) => match config::env_var_for(key).and_then(|e| std::env::var(e).ok()) {
            Some(env_val) if env_val == v => "env",
            _ => "flag",
        },
        None if in_file => "config",
        None => "default",
    }
}

fn config_show(cli: &Cli) -> Result<()> {
    let cfg = Config::load()?;
    // Parse the raw YAML to tell a file-set key from a defaulted one.
    let raw: serde_yaml_ng::Value = config::config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_yaml_ng::from_str(&s).ok())
        .unwrap_or(serde_yaml_ng::Value::Null);

    println!("{:<21}{:<30} SOURCE", "KEY", "VALUE");
    for &key in config::CONFIG_KEYS {
        let value = config_effective(cli, &cfg, key);
        let source = config_source(cli, key, raw.get(key).is_some());
        // The literal space guarantees a gap even when `value` exceeds the pad.
        println!("{key:<21}{value:<30} {source}");
    }
    if let Some(p) = config::config_path() {
        println!("\nconfig file: {}", p.display());
    }
    Ok(())
}

fn config_get(cli: &Cli, key: &str) -> Result<()> {
    if !config::CONFIG_KEYS.contains(&key) {
        anyhow::bail!("unknown config key `{key}` — run `adroit config` to list keys");
    }
    let cfg = Config::load()?;
    println!("{}", config_effective(cli, &cfg, key));
    Ok(())
}

fn config_set(key: &str, value: &str, local: bool) -> Result<()> {
    if !config::CONFIG_KEYS.contains(&key) {
        anyhow::bail!("unknown config key `{key}` — run `adroit config` to list keys");
    }
    // Validate the value against the key's type before writing anything.
    Config::default()
        .set_str(key, value)
        .map_err(|e| anyhow::anyhow!(e))?;

    if local {
        let env = config::env_var_for(key).ok_or_else(|| {
            anyhow::anyhow!(
                "`{key}` has no environment variable — omit --local to write config.yaml"
            )
        })?;
        let path = std::path::PathBuf::from(".env");
        config::upsert_env_file(&path, env, value)?;
        println!("Set {env}={value} in {}", path.display());
    } else {
        let mut cfg = Config::load()?;
        cfg.set_str(key, value).map_err(|e| anyhow::anyhow!(e))?;
        cfg.save()?;
        let path = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("Set {key} = {value} in {path}");
    }
    Ok(())
}

/// `adroit migrate`: convert the repo to the configured layout/format. Prints a
/// preview by default (or with `--dry-run`); `--yes` applies it.
fn cmd_migrate(store: &Store, yes: bool, dry_run: bool) -> Result<()> {
    // `--dry-run` always wins over `--yes`, so a preview is never destructive.
    let apply = yes && !dry_run;
    let plan = store.migrate(false)?;
    if plan.is_noop() {
        println!("Already in the configured layout/format — nothing to migrate.");
        return Ok(());
    }
    if let Some((from, to)) = plan.layout_change {
        println!("Layout: {from} -> {to}");
    }
    if let Some((from, to)) = plan.format_change {
        println!("Format: {from} -> {to}");
    }
    println!("{} ADR file(s) affected.", plan.files);
    for (from, to) in &plan.moves {
        println!("  {} -> {}", from.display(), to.display());
    }
    if !apply {
        let label = if dry_run { "Dry run" } else { "Preview only" };
        println!("\n{label} — re-run with `--yes` to apply.");
        return Ok(());
    }
    let done = store.migrate(true)?;
    println!("\nMigrated {} file(s).", done.files);
    if done.links_rewritten > 0 {
        println!("Relinked {} cross-ADR link(s).", done.links_rewritten);
    }
    Ok(())
}

/// `adroit renumber`: renumber a sequential ADR (resolves a collision).
fn cmd_renumber(
    store: &Store,
    old: Number,
    new: Number,
    file: Option<&std::path::Path>,
) -> Result<()> {
    let r = store.renumber(old, new, file)?;
    println!(
        "Renumbered ADR-{:04} -> ADR-{:04} ({} file(s) updated).",
        r.from, r.to, r.files_updated
    );
    Ok(())
}

/// `adroit relink`: rewrite cross-ADR relative links to each ADR's current
/// location. Repairs links left stale by file moves; idempotent. `--dry-run`
/// reports what would change without writing.
fn cmd_relink(store: &Store, dry_run: bool) -> Result<()> {
    let r = store.relink(!dry_run)?;
    if r.files_changed == 0 {
        println!("Links already canonical — nothing to relink.");
        return Ok(());
    }
    let verb = if dry_run { "Would relink" } else { "Relinked" };
    let links = if r.links_rewritten == 1 {
        "link"
    } else {
        "links"
    };
    let files = if r.files_changed == 1 {
        "file"
    } else {
        "files"
    };
    println!(
        "{verb} {} {links} across {} {files}:",
        r.links_rewritten, r.files_changed
    );
    for f in &r.changed_files {
        println!("  {}", f.display());
    }
    if dry_run {
        println!("\nDry run — no files written. Re-run without `--dry-run` to apply.");
    }
    Ok(())
}

/// `adroit check`: structural CI gate. Collects every problem found across the
/// store and bails (non-zero exit) with a summary if any exist; otherwise prints
/// an "OK" line and exits 0.
///
/// Checks performed (directory-status checks are skipped in flat/frontmatter
/// where no directory implies a status):
/// 1. Status ↔ directory mismatch (by_status only).
/// 2. Duplicate ADR numbers.
/// 3. Unparseable / missing-H1 ADR files.
/// 4. Broken supersession links (referenced ADR number doesn't exist).
/// 5. Broken / stale cross-ADR relative links.
fn cmd_check(store: &Store) -> Result<()> {
    use std::collections::BTreeMap;

    let files = store.list_files()?;
    let mut problems: Vec<String> = Vec::new();

    // Track which ADR numbers exist (for the numeric supersession-link checks)
    // and group paths by the scheme's identity (to flag duplicates — works for
    // every naming scheme, not just the numeric ones).
    let mut by_number: BTreeMap<u32, Vec<std::path::PathBuf>> = BTreeMap::new();
    let mut by_ident: BTreeMap<String, Vec<std::path::PathBuf>> = BTreeMap::new();
    let scheme = store.options().naming;
    let markdown = store.options().format == Format::Markdown;

    for path in &files {
        let rel = path
            .strip_prefix(store.root())
            .unwrap_or(path)
            .display()
            .to_string();

        // (3) Unparseable / missing H1.
        let adr = match store.read(path) {
            Ok(adr) => adr,
            Err(e) => {
                problems.push(format!("{rel}: failed to parse ({e})"));
                continue;
            }
        };
        if let Some(number) = adr.number {
            by_number
                .entry(number.get())
                .or_default()
                .push(path.clone());
        }
        // Group by the scheme's identity for duplicate detection. A numeric ADR
        // with no number, or a file with no parseable identity, is skipped (same
        // as before) so stray notes don't register as collisions.
        let r = adr.reference();
        let track = matches!(r, AdrRef::Slug(_)) || adr.number.is_some();
        if track {
            by_ident
                .entry(ident_key(&r))
                .or_default()
                .push(path.clone());
        }

        // Markdown-specific checks need the file's raw text and section status.
        if markdown {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("could not read {}", path.display()))?;

            // (1) Status ↔ directory mismatch (by_status only). A section with
            // no explicit status word is allowed (directory is source of truth).
            if let Some(dir_status) = store.dir_status(path)
                && let Some(section_status) =
                    adroit::format::parse_markdown_section_status(&content)
                && dir_status != section_status
            {
                let num = adr.number.map(|n| format!("ADR-{n} ")).unwrap_or_default();
                problems.push(format!(
                    "{num}({rel}): directory says {dir_status} but ## Status says {section_status}"
                ));
            }
        }
    }

    // (4) Broken supersession links. Resolved through the naming seam and
    // checked against the full identity set, so forward/backward references in
    // any order — and slug schemes — all work.
    if markdown {
        for path in &files {
            let rel = path
                .strip_prefix(store.root())
                .unwrap_or(path)
                .display()
                .to_string();
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let (supersedes, superseded_by) =
                adroit::format::parse_markdown_section_supersession(&content, scheme);
            for (kind, r) in [("Supersedes", supersedes), ("Superseded by", superseded_by)] {
                if let Some(r) = r
                    && !by_ident.contains_key(&ident_key(&r))
                {
                    problems.push(format!(
                        "{rel}: ## Status says {kind} {} but no such ADR exists",
                        scheme.display(&r)
                    ));
                }
            }
        }
    }

    // (5) Cross-ADR relative links: each must resolve to an existing file, and
    // an ADR-numbered link should point at where that ADR currently lives.
    let by_number_path: BTreeMap<u32, std::path::PathBuf> = by_number
        .iter()
        .filter(|(_, paths)| paths.len() == 1)
        .map(|(n, paths)| (*n, paths[0].clone()))
        .collect();
    for path in &files {
        let rel = path
            .strip_prefix(store.root())
            .unwrap_or(path)
            .display()
            .to_string();
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        for target in adroit::links::relative_md_targets(&content) {
            let pathpart = target.split('#').next().unwrap_or(target);
            let resolved = dir.join(pathpart);
            if !resolved.exists() {
                problems.push(format!(
                    "{rel}: broken link [{target}] — target file not found"
                ));
                continue;
            }
            // Stale: resolves, but not to the current home of its ADR number.
            if let Some(num) = adroit::links::number_in_target(target)
                && let Some(canon) = by_number_path.get(&num)
                && let (Ok(rp), Ok(cp)) = (
                    std::fs::canonicalize(&resolved),
                    std::fs::canonicalize(canon),
                )
                && rp != cp
            {
                let want = adroit::links::rel_link(dir, canon);
                problems.push(format!(
                    "{rel}: stale link [{target}] — ADR-{num} is now [{want}] (run `adroit relink`)"
                ));
            }
        }
    }

    // (2) Duplicate identifiers (scheme-aware). The wording stays "number" for
    // numeric schemes (byte-identical message) and "identifier" otherwise.
    let noun = if scheme.is_numeric() {
        "number"
    } else {
        "identifier"
    };
    for (key, paths) in &by_ident {
        if paths.len() > 1 {
            // Numeric identity → `ADR-NNNN` (from the key, so the message is
            // byte-identical); slug identity → the scheme's display string.
            let disp = if let Some(num) = key.strip_prefix("n:") {
                format!("ADR-{:04}", num.parse::<u32>().unwrap_or(0))
            } else {
                scheme
                    .parse(&paths[0], "")
                    .map(|r| scheme.display(&r))
                    .unwrap_or_else(|| key.trim_start_matches("s:").to_string())
            };
            let list = paths
                .iter()
                .map(|p| {
                    p.strip_prefix(store.root())
                        .unwrap_or(p)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", ");
            problems.push(format!("{disp}: duplicate {noun} used by {list}"));
        }
    }

    if problems.is_empty() {
        println!("OK: {} ADRs, no problems", files.len());
        Ok(())
    } else {
        problems.sort();
        for problem in &problems {
            eprintln!("{problem}");
        }
        anyhow::bail!(
            "{} problem(s) found across {} ADR file(s)",
            problems.len(),
            files.len()
        );
    }
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
