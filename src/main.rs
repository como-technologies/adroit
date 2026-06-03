use anyhow::{Context, Result};
use clap::Parser;

use adroit::adr::{Number, ReviewBy, Status};
use adroit::cli::{Cli, Command, ConfigAction};
use adroit::config::{self, Config, Layout};
use adroit::format::Format;
use adroit::naming::AdrRef;
use adroit::query::{self, Filter};
use adroit::store::{LinkKind, Store, StoreOptions};
use adroit::view::{AdrSummary, EdgeKind, Severity};

/// Parse a CLI ADR identifier into an [`AdrRef`] under the configured scheme.
fn resolve_ref(cfg: &Config, id: &str) -> Result<AdrRef> {
    cfg.naming.parse_ref(id).ok_or_else(|| {
        anyhow::anyhow!(
            "'{id}' is not a valid ADR identifier for the {} naming scheme",
            cfg.naming
        )
    })
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
    // `--relink-scope` / `ADROIT_RELINK_SCOPE` overrides how much a status-change
    // move auto-relinks (all/self/none).
    if let Some(scope) = cli.relink_scope {
        cfg.relink_scope = scope;
    }

    // `config` operates on configuration, not ADRs — handle it before resolving
    // a dir or opening a store, so it works even on a profile-mismatched repo.
    if let Some(Command::Config { action }) = &cli.command {
        return cmd_config(action.as_ref(), &cli);
    }
    // `auth` only writes the credential store — no ADR dir/store needed.
    if let Some(Command::Auth {
        provider,
        token,
        email,
    }) = &cli.command
    {
        return cmd_auth(provider, token.clone(), email.clone());
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

    // `open_or_create_with` creates the dir when it's missing. Capture whether
    // it existed first, so we can flag a freshly-created dir — a typo'd `--dir` /
    // `ADROIT_DIR` otherwise silently creates a stray empty repo and a read
    // returns nothing with no error.
    let dir_existed = dir.is_dir();
    let store = Store::open_or_create_with(&dir, opts)?;
    if !dir_existed {
        match &cli.command {
            // `new` legitimately scaffolds a brand-new repo — a neutral note.
            Some(Command::New { .. }) => {
                eprintln!("Created new ADR directory {}", dir.display());
            }
            // Any other command against a non-existent dir means we're pointed at
            // an empty/typo'd path; warn so an empty result isn't mistaken for an
            // empty repo.
            _ => {
                eprintln!(
                    "warning: ADR directory {} did not exist and was created empty \
                     — check your --dir / ADROIT_DIR if you expected existing ADRs",
                    dir.display()
                );
            }
        }
    }

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
            with_forge,
            dry_run,
        }) => {
            let path = cmd_new(
                &store,
                &cfg,
                &title,
                template.as_deref(),
                category.as_deref(),
            )?;
            println!("Created {}", path.display());
            // Opt-in forge hook (issue + draft PR + ## References). No-op unless
            // --with-forge and the `forge` feature is built in. Runs before the
            // editor so the populated References are visible.
            adroit::forge_hook::after_new(
                &cfg,
                &path,
                &title,
                adroit::forge_hook::ForgeFlags {
                    enabled: with_forge,
                    dry_run,
                    yes: false,
                },
            )?;
            if cfg.open_on_new && !no_edit {
                open_in_editor(editor, &path)?;
            }
        }
        Some(Command::List { status, forge }) => cmd_list(&store, &cfg, status.as_deref(), forge)?,
        Some(Command::Show { id }) => cmd_show(&store, &resolve_ref(&cfg, &id)?)?,
        Some(Command::Status { id }) => cmd_get_status(&store, &cfg, &id)?,
        Some(Command::SetStatus {
            id,
            status,
            with_forge,
            dry_run,
            yes,
        }) => cmd_set_status(
            &store,
            &cfg,
            &id,
            &status,
            adroit::forge_hook::ForgeFlags {
                enabled: with_forge,
                dry_run,
                yes,
            },
        )?,
        Some(Command::Supersede {
            new,
            old,
            with_forge,
            dry_run,
            yes,
        }) => {
            cmd_supersede(
                &store,
                &cfg,
                &resolve_ref(&cfg, &new)?,
                &resolve_ref(&cfg, &old)?,
                adroit::forge_hook::ForgeFlags {
                    enabled: with_forge,
                    dry_run,
                    yes,
                },
            )?;
        }
        Some(Command::SetReview {
            id,
            date,
            clear,
            with_forge,
            dry_run,
            yes,
        }) => {
            cmd_set_review(
                &store,
                &cfg,
                &resolve_ref(&cfg, &id)?,
                date.as_deref(),
                clear,
                adroit::forge_hook::ForgeFlags {
                    enabled: with_forge,
                    dry_run,
                    yes,
                },
            )?;
        }
        Some(Command::Link {
            id,
            relates_to,
            depends_on,
            refines,
            remove,
        }) => {
            cmd_link(&store, &cfg, &id, relates_to, depends_on, refines, remove)?;
        }
        Some(Command::Search { term }) => cmd_search(&store, &term)?,
        Some(Command::Check { forge }) => cmd_check(&store, &cfg, forge)?,
        Some(Command::Relink {
            dry_run,
            with_forge,
            yes,
        }) => cmd_relink(
            &store,
            &cfg,
            dry_run,
            adroit::forge_hook::ForgeFlags {
                enabled: with_forge,
                dry_run,
                yes,
            },
        )?,
        Some(Command::Sync { id, dry_run, yes }) => cmd_sync(&store, &cfg, &id, dry_run, yes)?,
        Some(Command::Renumber { old, new, file }) => {
            require_numeric_scheme(&cfg, "renumber")?;
            cmd_renumber(&store, Number::new(old), Number::new(new), file.as_deref())?;
        }
        Some(Command::Migrate { yes, dry_run }) => cmd_migrate(&store, yes, dry_run)?,
        Some(Command::Init { print }) => cmd_init(&store, print)?,
        Some(Command::Publish { out, dry_run }) => cmd_publish(&store, &out, dry_run)?,
        Some(Command::Notify { id, dry_run }) => cmd_notify(&store, &cfg, &id, dry_run)?,
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
            with_forge,
            dry_run,
            yes,
        }) => {
            require_numeric_scheme(&cfg, "review")?;
            cmd_review(
                &store,
                &cfg,
                Number::new(number),
                days,
                quorum,
                output.as_deref(),
                adroit::forge_hook::ForgeFlags {
                    enabled: with_forge,
                    dry_run,
                    yes,
                },
            )?;
        }
        Some(Command::Serve { host, port }) => serve(&cfg, &dir, &host, port)?,
        // `config` returns before the store is opened (see above).
        Some(Command::Config { .. }) => unreachable!("config handled before store open"),
        Some(Command::Auth { .. }) => unreachable!("auth handled before store open"),
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
        relink_scope: cfg.relink_scope,
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

fn cmd_list(store: &Store, cfg: &Config, status_filter: Option<&str>, forge: bool) -> Result<()> {
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
    let mut rows = query::summaries(store, &filter)?;
    if rows.is_empty() {
        return Ok(());
    }
    // Opt-in: attach live forge state (no-op unless --forge + feature + config).
    adroit::forge_hook::enrich(cfg, store, &mut rows, forge)?;
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
    // Typed relational links + plain body links (supersession already shown).
    for link in &detail.related {
        let label = match link.kind {
            EdgeKind::Supersedes => continue,
            EdgeKind::DependsOn => "Depends on",
            EdgeKind::Refines => "Refines",
            EdgeKind::RelatesTo => "Relates to",
            EdgeKind::Related => "Related",
        };
        println!("{label}: {}", link.reference);
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

/// `adroit status <ID>`: print just an ADR's current status word, **lowercase**,
/// so it's scriptable — it round-trips into `set-status` and matches the
/// by_status directory names (e.g. `[ "$(adroit status 7)" = accepted ]`). The
/// capitalized form is display-only (`show`, `list`).
fn cmd_get_status(store: &Store, cfg: &Config, id: &str) -> Result<()> {
    let r = resolve_ref(cfg, id)?;
    let path = store.find_path_by_ref(&r)?;
    let detail = query::detail_at(store, &path)?;
    println!("{}", detail.summary.status.to_string().to_lowercase());
    Ok(())
}

/// `adroit set-status <ID> <STATUS>`: set an ADR's status (moves the file in
/// by_status layout and rewrites links per `relink_scope`).
fn cmd_set_status(
    store: &Store,
    cfg: &Config,
    id: &str,
    status: &str,
    forge: adroit::forge_hook::ForgeFlags,
) -> Result<()> {
    let new_status: Status = status.parse().map_err(|_| {
        anyhow::anyhow!(
            "invalid status '{status}', expected: proposed, accepted, rejected, deprecated, superseded"
        )
    })?;
    let r = resolve_ref(cfg, id)?;
    // Opt-in forge pre-step (verify + merge/close before the local move). A
    // false return means "previewed only — don't move"; an error aborts.
    let path = store.find_path_by_ref(&r)?;
    if !adroit::forge_hook::before_status_change(cfg, &path, new_status, forge)? {
        return Ok(());
    }
    let path = store.set_status_ref(&r, new_status)?;
    println!(
        "Updated {} status to {new_status} ({})",
        cfg.naming.display(&r),
        path.display()
    );
    Ok(())
}

/// Trim an RFC 3339 timestamp to its `YYYY-MM-DD` date for terse display.
fn ymd(iso: &str) -> &str {
    iso.get(..10).unwrap_or(iso)
}

/// Add or remove a typed relational link from `id` to a target ADR.
fn cmd_link(
    store: &Store,
    cfg: &Config,
    id: &str,
    relates_to: Option<String>,
    depends_on: Option<String>,
    refines: Option<String>,
    remove: bool,
) -> Result<()> {
    let (kind, target) = match (relates_to, depends_on, refines) {
        (Some(t), None, None) => (LinkKind::RelatesTo, t),
        (None, Some(t), None) => (LinkKind::DependsOn, t),
        (None, None, Some(t)) => (LinkKind::Refines, t),
        _ => anyhow::bail!("exactly one of --relates-to / --depends-on / --refines is required"),
    };
    let source = resolve_ref(cfg, id)?;
    let target = resolve_ref(cfg, &target)?;
    store.set_links_ref(&source, kind, &target, remove)?;
    let verb = if remove { "Unlinked" } else { "Linked" };
    let rel = match kind {
        LinkKind::RelatesTo => "relates to",
        LinkKind::DependsOn => "depends on",
        LinkKind::Refines => "refines",
    };
    println!(
        "{verb} {} {rel} {}",
        cfg.naming.display(&source),
        cfg.naming.display(&target)
    );
    Ok(())
}

fn cmd_supersede(
    store: &Store,
    cfg: &Config,
    new: &AdrRef,
    old: &AdrRef,
    forge: adroit::forge_hook::ForgeFlags,
) -> Result<()> {
    // Opt-in forge pre-step: comment on + close the old ADR's issue/PR. A false
    // return means "previewed only — don't change locally".
    let old_path_pre = store.find_path_by_ref(old)?;
    let new_label = cfg.naming.display(new);
    if !adroit::forge_hook::on_supersede(cfg, &old_path_pre, &new_label, forge)? {
        return Ok(());
    }
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
    forge: adroit::forge_hook::ForgeFlags,
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
    // Opt-in: mirror the deadline to the linked issue/PR as a comment.
    let note = match review_by {
        Some(rb) => format!("Review deadline for {id} set to **{rb}** (via adroit)."),
        None => format!("Review deadline for {id} cleared (via adroit)."),
    };
    adroit::forge_hook::comment(cfg, &path, &note, "review deadline", forge)?;
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
    println!(
        "{:<id_w$}{:<12}{}{}",
        row_id(row),
        row.status,
        row.title,
        forge_suffix(row)
    );
}

/// A compact " · PR merged/2 approvals, ci ok" suffix for `list --forge` rows.
fn forge_suffix(row: &AdrSummary) -> String {
    let Some(f) = &row.forge_data else {
        return String::new();
    };
    let mut parts = Vec::new();
    if f.pr_url.is_some() {
        let state = if f.pr_merged == Some(true) {
            "merged".to_string()
        } else {
            match (f.pr_approvals, &f.pr_ci) {
                (Some(a), Some(ci)) => format!("{a} approvals, ci {ci}"),
                _ => "open".to_string(),
            }
        };
        parts.push(format!("PR {state}"));
    } else if f.issue_url.is_some() {
        parts.push("issue".to_string());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("  · {}", parts.join(", "))
    }
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
        "relink_scope" => cli.relink_scope.map(|s| s.to_string()),
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
        let source = config_source(cli, key, config::yaml_has_key(&raw, key));
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
fn cmd_relink(
    store: &Store,
    cfg: &Config,
    dry_run: bool,
    forge: adroit::forge_hook::ForgeFlags,
) -> Result<()> {
    let r = store.relink(!dry_run)?;
    // Opt-in: after fixing in-repo links, refresh each linked PR's description so
    // its ADR reference tracks the (possibly moved) file.
    if forge.enabled {
        for (path, _) in store.list_with_paths()? {
            adroit::forge_hook::sync_pr(cfg, &path, forge)?;
        }
    }
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

/// `adroit sync`: refresh an ADR's linked PR description from its content.
fn cmd_sync(store: &Store, cfg: &Config, id: &str, dry_run: bool, yes: bool) -> Result<()> {
    let r = resolve_ref(cfg, id)?;
    let path = store.find_path_by_ref(&r)?;
    adroit::forge_hook::sync_pr(
        cfg,
        &path,
        adroit::forge_hook::ForgeFlags {
            enabled: true, // `sync` is inherently a forge op
            dry_run,
            yes,
        },
    )?;
    Ok(())
}

/// `adroit check`: structural CI gate. Runs [`query::check`] — the shared
/// validation engine (also behind the web dashboard's repo-health panel) — and
/// renders its report. It bails (non-zero exit) only when an **error**-severity
/// problem exists (duplicate number, broken link, unparseable file, …); a report
/// with only **warnings** (e.g. a stale-but-resolvable link that a post-merge
/// `adroit relink` will heal) is printed but exits 0. This lets a status-change
/// PR branch — which transiently carries stale inbound links under
/// `relink_scope = self` — pass CI, while a genuine defect still fails it.
fn cmd_check(store: &Store, cfg: &Config, forge: bool) -> Result<()> {
    let mut report = query::check(store)?;
    // Opt-in forge-aware checks (issue/PR drift) appended to the same report;
    // they're Warning-severity so they report but don't fail the gate.
    if forge {
        let entries = store.list_with_paths()?;
        report
            .problems
            .extend(adroit::forge_hook::check_repo(cfg, &entries, forge)?);
    }
    // Print every problem (errors and warnings), sorted for stable output.
    let mut messages: Vec<&str> = report.problems.iter().map(|p| p.message.as_str()).collect();
    messages.sort_unstable();
    for message in &messages {
        eprintln!("{message}");
    }
    let errors = report
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    if errors > 0 {
        anyhow::bail!(
            "{} problem(s) found across {} ADR file(s)",
            report.problems.len(),
            report.checked
        );
    }
    if report.problems.is_empty() {
        println!("OK: {} ADRs, no problems", report.checked);
    } else {
        let warnings = report.problems.len();
        println!("OK: {} ADRs, {} warning(s)", report.checked, warnings);
    }
    Ok(())
}

/// `adroit auth`: save a forge token to the local credential store.
fn cmd_auth(provider: &str, token: Option<String>, email: Option<String>) -> Result<()> {
    if !matches!(provider, "github" | "gitlab" | "jira") {
        anyhow::bail!("provider must be one of: github, gitlab, jira");
    }
    let token = match token {
        Some(t) => t,
        None => dialoguer::Password::new()
            .with_prompt(format!("{provider} API token"))
            .interact()
            .context("reading token")?,
    };
    config::store_credential(provider, &token)?;
    if provider == "jira"
        && let Some(e) = email
    {
        config::store_credential("jira_email", &e)?;
    }
    let path = config::credentials_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!("Saved {provider} token to {path} (environment variables still take precedence).");
    Ok(())
}

/// `adroit init`: detect the forge from the git remote and write the config.
fn cmd_init(store: &Store, print_only: bool) -> Result<()> {
    let url = std::process::Command::new("git")
        .arg("-C")
        .arg(store.root())
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let Some((provider, repo, host)) = url.as_deref().and_then(config::parse_remote_url) else {
        println!("Couldn't detect a GitHub/GitLab `origin` remote. Configure it manually:");
        println!("  adroit config set forge.provider github   # or gitlab");
        println!("  adroit config set forge.repo owner/repo");
        return Ok(());
    };

    let token_env = match provider {
        config::Provider::Gitlab => "ADROIT_GITLAB_TOKEN",
        _ => "ADROIT_GITHUB_TOKEN",
    };
    let host_note = host
        .as_deref()
        .map(|h| format!(" @ {h}"))
        .unwrap_or_default();
    println!("Detected forge: {provider} {repo}{host_note}");

    if print_only {
        println!("\n(--print: nothing written). To apply:");
        println!("  adroit config set forge.provider {provider}");
        println!("  adroit config set forge.repo {repo}");
        return Ok(());
    }

    // Load the persisted config (preserving other keys), set the forge block, save.
    let mut cfg = config::Config::load()?;
    let f = cfg.forge.get_or_insert_with(Default::default);
    f.provider = provider;
    f.repo = Some(repo);
    f.host = host;
    cfg.save()?;
    println!("Wrote forge settings to your config.");
    println!("Next: export {token_env}=<token>   (tokens come from the environment, never config)");
    Ok(())
}

/// `adroit publish`: export accepted ADRs to a directory (static-dir publisher).
fn cmd_publish(store: &Store, out: &std::path::Path, dry_run: bool) -> Result<()> {
    let report = adroit::publish::publish(store, out, !dry_run)?;
    if dry_run {
        println!(
            "Would publish {} accepted ADR(s) to {}:",
            report.written,
            out.display()
        );
        for (title, file) in &report.files {
            println!("  {file}  {title}");
        }
        println!("\nDry run — re-run without --dry-run to write.");
    } else {
        println!(
            "Published {} accepted ADR(s) to {}",
            report.written,
            out.display()
        );
    }
    Ok(())
}

/// `adroit notify`: announce an ADR's state to a chat webhook.
fn cmd_notify(store: &Store, cfg: &Config, id: &str, dry_run: bool) -> Result<()> {
    let webhook = std::env::var("ADROIT_NOTIFY_WEBHOOK").map_err(|_| {
        anyhow::anyhow!("set ADROIT_NOTIFY_WEBHOOK to a Slack/Teams incoming-webhook URL")
    })?;
    let r = resolve_ref(cfg, id)?;
    let path = store.find_path_by_ref(&r)?;
    let detail = query::detail_at(store, &path)?;
    let s = &detail.summary;
    let text = format!("*{}: {}* — {}", s.reference, s.title, s.status);
    adroit::forge_hook::notify(&webhook, &text, dry_run)?;
    if !dry_run {
        println!("Notified ({})", s.reference);
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
    forge: adroit::forge_hook::ForgeFlags,
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
            std::fs::write(out, &doc)
                .with_context(|| format!("could not write {}", out.display()))?;
            println!("Created {}", out.display());
        }
        None => print!("{doc}"),
    }
    // Opt-in: post the kickoff as a comment on the ADR's linked issue/PR.
    adroit::forge_hook::comment(cfg, &path, &doc, "review kickoff", forge)?;
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
