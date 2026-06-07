use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;

use adroit::adr::{Number, ReviewBy, Status};
use adroit::cli::{Cli, Command, ConfigAction, OutputFormat};
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
    // Disable ANSI color when stdout isn't a terminal (pipes, `-o json`, CI);
    // when it is, `colored` still honors NO_COLOR / CLICOLOR.
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        colored::control::set_override(false);
    }
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
    // `completions` just prints a script generated from the command tree — no
    // ADR dir/store needed, so it works anywhere (and reflects this build).
    if let Some(Command::Completions { shell }) = &cli.command {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let bin = cmd.get_name().to_string();
        clap_complete::generate(*shell, &mut cmd, bin, &mut std::io::stdout());
        return Ok(());
    }
    // `auth` only writes the credential store — no ADR dir/store needed.
    // Forge-only: the command exists solely in `--features forge` builds.
    #[cfg(feature = "forge")]
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

    // The `frontmatter` format is numeric-only — its YAML persists a `number:`
    // field and can't represent the slug-based identity schemes (date / uuid /
    // per_category). Refuse the combination up front with a clear message instead
    // of failing deep in the write path with a cryptic "number must be assigned
    // before serializing" error. (`config`/`completions` already returned above.)
    if opts.format == Format::Frontmatter && !opts.naming.is_numeric() {
        anyhow::bail!(
            "the `frontmatter` format supports only the `sequential` naming scheme \
             (it persists a numeric `number:`); `{}` requires `--format markdown`",
            opts.naming
        );
    }

    // Resolve editor before any I/O so we fail fast on misconfiguration.
    // Needed for `edit`, and for `new` unless --no-edit / open_on_new=false.
    let needs_editor = match &cli.command {
        Some(Command::Edit { .. }) => true,
        Some(Command::New { no_edit, .. }) => !no_edit && cfg.open_on_new,
        Some(Command::Draft { no_edit, .. }) => !no_edit,
        Some(Command::Compose { no_edit, .. }) => !no_edit,
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

    // `-o/--output` is honored by the read verbs (Copy, so reading it here
    // doesn't conflict with the `match cli.command` move below).
    let output = cli.output;

    match cli.command {
        Some(Command::New {
            title,
            template,
            no_edit,
            category,
            force,
            interview,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            dry_run,
        }) => {
            // Duplicate guard (`new` stays non-idempotent — this only catches the
            // accidental re-run). Aborts before allocating a number.
            if !dup_guard(&store, &cfg, &title, force)? {
                return Ok(());
            }
            let (path, r) = cmd_new(
                &store,
                &cfg,
                &title,
                template.as_deref(),
                category.as_deref(),
            )?;
            println!("Created {}", path.display());
            // Opt-in AI draft (writes over the template body before the forge
            // hook commits it / the editor opens for review). `degraded` = the
            // interview was asked for but no provider was available.
            let degraded = interview && !run_interview(&store, &cfg, &title, &r)?;
            // Opt-in forge hook (issue + draft PR + ## References). The forge
            // flags only exist in `--features forge` builds; otherwise the hook
            // gets disabled flags and no-ops. Runs before the editor so the
            // populated References are visible.
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes: false,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            adroit::forge_hook::after_new(&cfg, &path, &title, forge_flags)?;
            // Don't bury a degraded-interview warning under the editor: when
            // `--interview` was requested but couldn't run, skip the auto-open so
            // the message stays on screen (the file is still there to `edit`).
            if cfg.open_on_new && !no_edit && !degraded {
                open_in_editor(editor, &path)?;
            }
        }
        Some(Command::List {
            status,
            #[cfg(feature = "forge")]
            forge,
        }) => {
            #[cfg(feature = "forge")]
            let forge_on = forge;
            #[cfg(not(feature = "forge"))]
            let forge_on = false;
            cmd_list(&store, &cfg, status.as_deref(), forge_on, output)?;
        }
        Some(Command::Show { id }) => cmd_show(&store, &resolve_ref(&cfg, &id)?, output)?,
        Some(Command::Summarize { id, out }) => {
            cmd_summarize(&store, &cfg, &resolve_ref(&cfg, &id)?, out.as_deref())?
        }
        Some(Command::Status { id }) => cmd_get_status(&store, &cfg, &id)?,
        Some(Command::SetStatus {
            id,
            status,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            dry_run,
            #[cfg(feature = "forge")]
            yes,
        }) => {
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            cmd_set_status(&store, &cfg, &id, &status, forge_flags)?;
        }
        Some(Command::Supersede {
            new,
            old,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            dry_run,
            #[cfg(feature = "forge")]
            yes,
        }) => {
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            cmd_supersede(
                &store,
                &cfg,
                &resolve_ref(&cfg, &new)?,
                &resolve_ref(&cfg, &old)?,
                forge_flags,
            )?;
        }
        Some(Command::SetReview {
            id,
            date,
            clear,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            dry_run,
            #[cfg(feature = "forge")]
            yes,
        }) => {
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            cmd_set_review(
                &store,
                &cfg,
                &resolve_ref(&cfg, &id)?,
                date.as_deref(),
                clear,
                forge_flags,
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
        Some(Command::Search { term }) => cmd_search(&store, &term, output)?,
        Some(Command::Check {
            #[cfg(feature = "forge")]
            forge,
        }) => {
            #[cfg(feature = "forge")]
            let forge_on = forge;
            #[cfg(not(feature = "forge"))]
            let forge_on = false;
            cmd_check(&store, &cfg, forge_on, output)?;
        }
        Some(Command::Lint { id, ai }) => {
            cmd_lint(&store, &cfg, &resolve_ref(&cfg, &id)?, ai, output)?
        }
        Some(Command::Stats) => cmd_stats(&store, output)?,
        Some(Command::Graph) => cmd_graph(&store, output)?,
        Some(Command::Related { id }) => {
            cmd_related(&store, &cfg, &resolve_ref(&cfg, &id)?, false, output)?
        }
        Some(Command::Dedupe { id }) => {
            cmd_related(&store, &cfg, &resolve_ref(&cfg, &id)?, true, output)?
        }
        Some(Command::Ask { question }) => cmd_ask(&store, &cfg, &question, output)?,
        Some(Command::Plan { id, out }) => {
            cmd_plan(&store, &cfg, &resolve_ref(&cfg, &id)?, out.as_deref())?
        }
        Some(Command::Draft { id, no_edit }) => {
            cmd_draft(&store, &cfg, &resolve_ref(&cfg, &id)?, no_edit, editor)?
        }
        Some(Command::Compose {
            id,
            instruction,
            no_edit,
        }) => cmd_compose(
            &store,
            &cfg,
            &resolve_ref(&cfg, &id)?,
            &instruction,
            no_edit,
            editor,
        )?,
        Some(Command::Relink {
            dry_run,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            yes,
        }) => {
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            cmd_relink(&store, &cfg, dry_run, forge_flags)?;
        }
        #[cfg(feature = "forge")]
        Some(Command::Sync { id, dry_run, yes }) => cmd_sync(&store, &cfg, &id, dry_run, yes)?,
        #[cfg(feature = "forge")]
        Some(Command::Reconcile { yes }) => cmd_reconcile(&store, &cfg, yes)?,
        Some(Command::Renumber { old, new, file }) => {
            require_numeric_scheme(&cfg, "renumber")?;
            cmd_renumber(&store, Number::new(old), Number::new(new), file.as_deref())?;
        }
        Some(Command::Migrate { yes, dry_run }) => cmd_migrate(&store, yes, dry_run)?,
        #[cfg(feature = "forge")]
        Some(Command::Init { print, yes }) => cmd_init(&store, print, yes)?,
        Some(Command::Publish { out, dry_run }) => cmd_publish(&store, &out, dry_run)?,
        #[cfg(feature = "forge")]
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
            out,
            #[cfg(feature = "forge")]
            forge,
            #[cfg(feature = "forge")]
            dry_run,
            #[cfg(feature = "forge")]
            yes,
        }) => {
            require_numeric_scheme(&cfg, "review")?;
            #[cfg(feature = "forge")]
            let forge_flags = adroit::forge_hook::ForgeFlags {
                enabled: forge,
                dry_run,
                yes,
            };
            #[cfg(not(feature = "forge"))]
            let forge_flags = adroit::forge_hook::ForgeFlags::default();
            cmd_review(
                &store,
                &cfg,
                Number::new(number),
                days,
                quorum,
                out.as_deref(),
                forge_flags,
            )?;
        }
        Some(Command::Serve { host, port }) => serve(&cfg, &dir, &host, port)?,
        // `config` / `completions` return before the store is opened (see above).
        Some(Command::Config { .. }) => unreachable!("config handled before store open"),
        Some(Command::Completions { .. }) => {
            unreachable!("completions handled before store open")
        }
        #[cfg(feature = "forge")]
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

/// Build store options from config, applying CLI `--format`/`--layout` overrides.
fn store_options(cfg: &Config, format: Option<Format>, layout: Option<Layout>) -> StoreOptions {
    let mut options = StoreOptions::from_config(cfg);
    if let Some(format) = format {
        options.format = format;
    }
    if let Some(layout) = layout {
        options.layout = layout;
    }
    options
}

/// Guard against accidentally creating a duplicate ADR. On an exact
/// (case-insensitive) title match it warns and lists the matches plus the top
/// similar ADRs; on a terminal it then prompts to confirm. Returns `false` to
/// abort (no ADR created). `--force` and non-interactive contexts proceed — `new`
/// stays non-idempotent by design, so this only catches the accidental re-run.
fn dup_guard(store: &Store, cfg: &Config, title: &str, force: bool) -> Result<bool> {
    use std::io::{IsTerminal, Write};
    if force {
        return Ok(true);
    }
    let summaries = query::summaries(store, &Filter::default())?;
    let exact: Vec<&AdrSummary> = summaries
        .iter()
        .filter(|s| s.title.trim().eq_ignore_ascii_case(title.trim()))
        .collect();
    if exact.is_empty() {
        return Ok(true);
    }

    eprintln!(
        "{}",
        format!(
            "warning: {} existing ADR(s) already use this title:",
            exact.len()
        )
        .yellow()
    );
    for s in &exact {
        eprintln!(
            "  {} {} [{}]",
            s.reference.bold(),
            s.title,
            status_color(s.status)
        );
    }

    // Top similar existing ADRs (excluding the exact matches), via the same
    // TF-IDF engine as `dedupe` — the question text is the new title.
    let exact_refs: std::collections::HashSet<&str> =
        exact.iter().map(|s| s.reference.as_str()).collect();
    let mut docs = corpus_docs(store, cfg)?;
    docs.push(adroit::similar::Doc {
        id: "__new__".to_string(),
        reference: String::new(),
        title: title.to_string(),
        text: title.to_string(),
    });
    let similar: Vec<_> = adroit::similar::rank(&docs, "__new__")
        .into_iter()
        .filter(|m| !exact_refs.contains(m.reference.as_str()))
        .take(3)
        .collect();
    if !similar.is_empty() {
        eprintln!("similar existing ADRs:");
        for m in &similar {
            eprintln!(
                "  {:.2}  {} {}",
                m.score,
                m.reference.bold(),
                m.title.dimmed()
            );
        }
    }

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprint!("Create another ADR with this title anyway? [y/N] ");
        std::io::stderr().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            eprintln!("Aborted — no ADR created. (use --force to skip this check)");
            return Ok(false);
        }
    } else {
        eprintln!("(non-interactive: proceeding — pass --force to silence, or use a new title)");
    }
    Ok(true)
}

fn cmd_new(
    store: &Store,
    cfg: &Config,
    title: &str,
    template: Option<&str>,
    category: Option<&str>,
) -> Result<(std::path::PathBuf, AdrRef)> {
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

    let path = store.write(&mut adr)?;
    Ok((path, r))
}

/// `new --interview`: resolve a provider then run the shared interview. Degrades
/// to a note (keeping the plain template) when no provider is available, so the
/// ADR is always created. Returns `true` when the draft actually ran (so the
/// caller can skip auto-opening the editor on a degrade).
fn run_interview(store: &Store, cfg: &Config, title: &str, r: &AdrRef) -> Result<bool> {
    let Some(provider) = adroit::ai_hook::open_provider(cfg) else {
        if cfg!(feature = "ai") {
            eprintln!(
                "{}",
                "--interview could not run: enable AI (`ai.enabled` / `ADROIT_AI_ENABLED=true`) \
                 and set `ADROIT_ANTHROPIC_KEY`. The ADR was created from the plain template — \
                 fill it in later with `adroit draft <id>`."
                    .yellow()
            );
        } else {
            eprintln!(
                "{}",
                "--interview could not run: this binary lacks the AI feature. Rebuild with \
                 `just build-ai` (then enable AI). The ADR was created from the plain template — \
                 fill it in later with `adroit draft <id>`."
                    .yellow()
            );
        }
        return Ok(false);
    };
    interview_and_draft(store, title, r, provider.as_ref())
}

/// The shared Socratic interview → AI draft → splice, used by both `new
/// --interview` (at creation) and `draft <ID>` (on an existing ADR). Asks the
/// fixed [`INTERVIEW_QUESTIONS`] over stdin (prompts to stderr), drafts the body
/// from the answers + corpus, and splices it over the prose — identity / status /
/// heading stay mechanical, marked `<!-- adroit:ai-suggested -->`. Returns `true`
/// on success; `false` if the AI call fails — the existing body is kept and a
/// warning printed (never an error), so the ADR is never lost.
fn interview_and_draft(
    store: &Store,
    title: &str,
    r: &AdrRef,
    provider: &dyn adroit::ai::AiProvider,
) -> Result<bool> {
    use adroit::ai::{self, INTERVIEW_QUESTIONS, Interview};
    use std::io::{BufRead, Write};

    // Plain stdin prompts (robust on a non-TTY, e.g. piped test input). Prompts
    // go to stderr so stdout stays clean.
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut answers: Vec<String> = Vec::with_capacity(INTERVIEW_QUESTIONS.len());
    for q in INTERVIEW_QUESTIONS {
        eprintln!("\n{q}");
        eprint!("> ");
        std::io::stderr().flush().ok();
        let a = lines.next().transpose()?.unwrap_or_default();
        answers.push(a.trim().to_string());
    }

    let iv = Interview {
        title: title.to_string(),
        context: answers[0].clone(),
        drivers: answers[1].clone(),
        options: answers[2].clone(),
        risks: answers[3].clone(),
    };
    let corpus: Vec<String> = query::summaries(store, &Filter::default())?
        .iter()
        .map(|s| format!("{} — {}", s.reference, s.title))
        .collect();

    let req = ai::build_request(&iv, &corpus);
    announce_estimate(provider, &req, "\nDrafting the ADR body");
    let draft = match ai::draft_body(provider, &iv, &corpus) {
        Ok(d) => d,
        // A provider failure (credits, network, …) shouldn't error: keep the
        // existing body, warn, and let the caller skip the editor.
        Err(e) => {
            eprintln!(
                "{}",
                format!(
                    "warning: AI draft failed ({e}). Kept the existing body (your answers \
                     weren't saved) — fix the provider and re-run, or edit by hand."
                )
                .yellow()
            );
            return Ok(false);
        }
    };
    // Journal the raw draft before splicing, so it survives a failed write/edit.
    let journal = journal_draft(store, r, &draft);
    splice_ai_draft(store, r, &draft)?;
    eprintln!(
        "AI-drafted the body (marked `{}`). Review and edit before committing.",
        ai::AI_MARKER
    );
    if let Some(p) = journal {
        eprintln!(
            "(Raw draft journaled to {} — delete when you're done.)",
            p.display()
        );
    }
    Ok(true)
}

/// Splice a marker-wrapped AI `draft` into the ADR at `r`: keep the mechanical
/// header (every line before the first `## Context…` prose section) and replace
/// the prose. In the markdown profile `adr.body` is the whole document, so this
/// is what preserves the H1 + `## Status`; in frontmatter it's the prose after the
/// YAML. AI never touches identity/status. Shared by `new --interview` + `draft`.
fn splice_ai_draft(store: &Store, r: &AdrRef, draft: &str) -> Result<()> {
    let path = store
        .find_path_by_ref(r)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let adr = store.read(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let prefix: String = adr
        .body
        .lines()
        .take_while(|l| !l.trim_start().starts_with("## Context"))
        .collect::<Vec<_>>()
        .join("\n");
    let new_body = if prefix.trim().is_empty() {
        format!("{}\n", draft.trim())
    } else {
        format!("{}\n\n{}\n", prefix.trim_end(), draft.trim())
    };
    store
        .set_body_ref(r, &new_body)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

/// One-line pre-call cost notice (RFC issue #5): the action, the provider, and a
/// rough token estimate, to stderr — so a large call never happens silently.
fn announce_estimate(
    provider: &dyn adroit::ai::AiProvider,
    req: &adroit::ai::CompletionRequest,
    action: &str,
) {
    eprintln!(
        "{action} via {} (~{} input tokens, up to {} generated — estimate) …",
        provider.id(),
        req.estimate_input_tokens(),
        req.max_tokens,
    );
}

/// Journal the raw AI `draft` to a git-ignored `<adr>.md.draft` sidecar before it
/// is spliced in, so the model's output survives a failed write or a botched edit
/// (resume or discard). Best-effort — a journaling failure is non-fatal. The
/// sidecar's extension isn't `.md`, so the store never treats it as an ADR.
fn journal_draft(store: &Store, r: &AdrRef, draft: &str) -> Option<std::path::PathBuf> {
    let path = store.find_path_by_ref(r).ok()?;
    let sidecar = std::path::PathBuf::from(format!("{}.draft", path.display()));
    std::fs::write(&sidecar, draft).ok()?;
    Some(sidecar)
}

/// `adroit draft <ID>`: run the **same interview** as `new --interview`, but on an
/// ADR that already exists — for when you created it with a plain `new` (template)
/// and want to fill it in later, before review. Drafts the body, then opens the
/// editor. Unlike `new --interview` it requires a provider (the ADR already
/// exists, so there's no template-fallback to degrade to).
fn cmd_draft(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    no_edit: bool,
    editor: Option<Option<String>>,
) -> Result<()> {
    let provider = require_provider(cfg, "draft")?;
    let path = store.find_path_by_ref(r)?;
    let adr = store.read(&path)?;
    let drafted = interview_and_draft(store, &adr.title, r, provider.as_ref())?;
    if drafted && !no_edit {
        open_in_editor(editor, &path)?;
    }
    Ok(())
}

/// `adroit compose <ID> "<instruction>"`: instruction-driven revision of an
/// existing ADR's body — the targeted, iterative cousin of `draft` (which re-runs
/// the fixed interview and redrafts wholesale). Reads the current body + corpus,
/// asks the provider for a revised body, splices it over the prose (heading /
/// status stay mechanical, marked `AI_MARKER`), then opens the editor. Same
/// engine as the TUI's "AI: draft / revise body" assist. Requires a provider.
fn cmd_compose(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    instruction: &str,
    no_edit: bool,
    editor: Option<Option<String>>,
) -> Result<()> {
    if instruction.trim().is_empty() {
        anyhow::bail!(
            "compose needs an instruction, e.g. adroit compose 9 \"expand the consequences\""
        );
    }
    let provider = require_provider(cfg, "compose")?;
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    let corpus: Vec<String> = query::summaries(store, &Filter::default())?
        .iter()
        .map(|s| format!("{} — {}", s.reference, s.title))
        .collect();
    let req = adroit::ai::build_compose_request(
        &detail.summary.title,
        instruction,
        &detail.body,
        &corpus,
    );
    announce_estimate(
        provider.as_ref(),
        &req,
        &format!("Composing {}", detail.summary.reference),
    );
    let drafted = adroit::ai::draft_compose(
        provider.as_ref(),
        &detail.summary.title,
        instruction,
        &detail.body,
        &corpus,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    splice_ai_draft(store, r, &drafted)?;
    println!(
        "Revised {} (AI-suggested; review before committing).",
        detail.summary.reference
    );
    if !no_edit {
        open_in_editor(editor, &path)?;
    }
    Ok(())
}

/// Print any `view` type as pretty JSON — the `-o json` path for the read verbs.
fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn cmd_list(
    store: &Store,
    cfg: &Config,
    status_filter: Option<&str>,
    forge: bool,
    output: OutputFormat,
) -> Result<()> {
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
    // Opt-in: attach live forge state (no-op unless --forge + feature + config).
    adroit::forge_hook::enrich(cfg, store, &mut rows, forge)?;
    if output == OutputFormat::Json {
        return print_json(&rows);
    }
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

fn cmd_show(store: &Store, r: &AdrRef, output: OutputFormat) -> Result<()> {
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    if output == OutputFormat::Json {
        return print_json(&detail);
    }
    let s = &detail.summary;
    println!("{}: {}", s.reference.bold(), s.title);
    println!("Status:  {}", status_color(s.status));
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
    // On an applied `accepted --forge`, commit the move + relink onto the base
    // branch and push it (no-op otherwise). Runs after the local move.
    adroit::forge_hook::after_status_change(cfg, &path, new_status, forge)?;
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
    // Relative link from new's dir to old's (now in superseded/). Use the same
    // canonical engine `relink` uses (`links::rel_link`) so the note's link is
    // born canonical (e.g. `./0002-x.md` for a same-dir target). Computing it any
    // other way leaves the repo non-canonical — a follow-up `relink` would then
    // rewrite this note, breaking the "relink is a no-op after a status op"
    // invariant. (`relative_link` here would omit the `./` for a same-dir target.)
    let old_path = store.find_path_by_ref(old)?;
    let new_dir = path.parent().unwrap_or(std::path::Path::new(""));
    let link = adroit::links::rel_link(new_dir, &old_path);
    updated.push_str(&format!("> Supersedes [{old_label}]({link})"));
    updated.push_str(newline);
    std::fs::write(&path, updated)?;
    Ok(())
}

/// Sibling-style relative markdown link from one file to another. A thin adapter
/// over the canonical [`adroit::links::rel_link`] that drops its leading `./` —
/// the sole caller, `link_prefix_for`, manages the `./`/`..`/`.` prefix itself.
fn relative_link(from_file: &std::path::Path, to_file: &std::path::Path) -> String {
    let from_dir = from_file.parent().unwrap_or(std::path::Path::new(""));
    let link = adroit::links::rel_link(from_dir, to_file);
    link.strip_prefix("./").unwrap_or(&link).to_string()
}

fn cmd_search(store: &Store, term: &str, output: OutputFormat) -> Result<()> {
    let rows = query::search(store, term)?;
    if output == OutputFormat::Json {
        return print_json(&rows);
    }
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
/// Color an ADR status for human output (a no-op when color is disabled).
fn status_color(s: Status) -> colored::ColoredString {
    let label = s.to_string();
    match s {
        Status::Proposed => label.yellow(),
        Status::Accepted => label.green(),
        Status::Rejected => label.red(),
        Status::Superseded => label.magenta(),
        Status::Deprecated => label.dimmed(),
    }
}

/// The bar color matching a status (for the `stats` bar chart).
fn status_bar_color(s: Status) -> colored::Color {
    match s {
        Status::Proposed => colored::Color::Yellow,
        Status::Accepted => colored::Color::Green,
        Status::Rejected => colored::Color::Red,
        Status::Superseded => colored::Color::Magenta,
        Status::Deprecated => colored::Color::BrightBlack,
    }
}

/// Render `(label, value, bar_color)` rows as horizontal bars (rnought/talaria
/// style: `label ██████░░ value`), scaled to the largest value.
fn print_bars(rows: &[(String, usize, colored::Color)], width: usize) {
    let max = rows.iter().map(|(_, v, _)| *v).max().unwrap_or(0).max(1);
    let label_w = rows.iter().map(|(l, _, _)| l.len()).max().unwrap_or(0);
    for (label, value, color) in rows {
        let filled = if *value == 0 {
            0
        } else {
            (((*value as f64 / max as f64) * width as f64).round() as usize).clamp(1, width)
        };
        let bar = "█".repeat(filled).color(*color);
        let empty = "░".repeat(width - filled).dimmed();
        println!("  {label:<label_w$}  {bar}{empty} {value}");
    }
}

/// A short, colored label for a graph edge kind.
fn edge_label(k: EdgeKind) -> colored::ColoredString {
    match k {
        EdgeKind::Supersedes => "supersedes".red(),
        EdgeKind::DependsOn => "depends on".magenta(),
        EdgeKind::Refines => "refines".blue(),
        EdgeKind::RelatesTo => "relates to".cyan(),
        EdgeKind::Related => "links".dimmed(),
    }
}

fn print_summary_row(row: &AdrSummary, id_w: usize) {
    // Pad the *plain* status to width before coloring, so ANSI codes don't break
    // column alignment.
    let pad = " ".repeat(12usize.saturating_sub(row.status.to_string().len()));
    println!(
        "{:<id_w$}{}{pad}{}{}",
        row_id(row),
        status_color(row.status),
        row.title,
        forge_suffix(row),
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
        // `naming` has a `--naming` / `ADROIT_NAMING` override too; without this
        // arm `config show`/`get naming` ignored it and reported the file/default
        // value (the actual ADR operations still honored it — only the diagnostic
        // lied).
        "naming" => cli.naming.map(|n| n.to_string()),
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

/// `adroit reconcile`: sync local ADR status with the forge after out-of-band changes.
#[cfg(feature = "forge")]
fn cmd_reconcile(store: &Store, cfg: &Config, apply: bool) -> Result<()> {
    let summaries = query::summaries(store, &Filter::default())?;
    adroit::forge::reconcile(cfg, store, &summaries, apply)?;
    Ok(())
}

/// `adroit sync`: refresh an ADR's linked PR description from its content.
#[cfg(feature = "forge")]
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
fn cmd_check(store: &Store, cfg: &Config, forge: bool, output: OutputFormat) -> Result<()> {
    let mut report = query::check(store)?;
    // Opt-in forge-aware checks (issue/PR drift) appended to the same report;
    // they're Warning-severity so they report but don't fail the gate.
    if forge {
        let entries = store.list_with_paths()?;
        report
            .problems
            .extend(adroit::forge_hook::check_repo(cfg, &entries, forge)?);
    }
    let errors = report
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    // `-o json` emits the structured report on stdout; the CI gate still holds —
    // a non-zero exit on any Error-severity problem.
    if output == OutputFormat::Json {
        print_json(&report)?;
        if errors > 0 {
            anyhow::bail!(
                "{} problem(s) found across {} ADR file(s)",
                report.problems.len(),
                report.checked
            );
        }
        return Ok(());
    }
    // Print every problem (errors and warnings), sorted for stable output.
    let mut messages: Vec<&str> = report.problems.iter().map(|p| p.message.as_str()).collect();
    messages.sort_unstable();
    for message in &messages {
        eprintln!("{message}");
    }
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

/// `adroit stats`: repo statistics (status counts, proposed ages, growth).
/// `-o json` emits `view::Stats`; the human view is a compact summary.
fn cmd_stats(store: &Store, output: OutputFormat) -> Result<()> {
    let stats = query::stats(store)?;
    if output == OutputFormat::Json {
        return print_json(&stats);
    }
    println!("Total ADRs: {}", stats.total.to_string().bold());
    println!("{}", "By status:".bold());
    let status_rows: Vec<(String, usize, colored::Color)> = stats
        .by_status
        .iter()
        .map(|sc| (sc.status.to_string(), sc.count, status_bar_color(sc.status)))
        .collect();
    print_bars(&status_rows, 24);
    if !stats.review_due.is_empty() {
        println!(
            "Review due: {}",
            stats.review_due.len().to_string().yellow()
        );
    }
    if !stats.proposed_age.is_empty() {
        println!("{}", "Oldest proposed:".bold());
        for p in stats.proposed_age.iter().take(5) {
            let age = p
                .age_days
                .map(|d| format!("{d}d"))
                .unwrap_or_else(|| "?".into());
            let flag = if p.review_due {
                " (review due)".yellow()
            } else {
                "".normal()
            };
            println!("  {:<8} {:<5} {}{}", p.reference, age, p.title, flag);
        }
    }
    if !stats.created_over_time.is_empty() {
        println!("{}", "Created over time:".bold());
        let month_rows: Vec<(String, usize, colored::Color)> = stats
            .created_over_time
            .iter()
            .map(|b| (b.month.clone(), b.count, colored::Color::Cyan))
            .collect();
        print_bars(&month_rows, 24);
    }
    Ok(())
}

/// `adroit graph`: the ADR relationship graph (supersession + typed links).
/// `-o json` emits `view::Graph` (nodes + edges); the human view is a **tree** —
/// each ADR with outgoing edges, its relations indented under it.
fn cmd_graph(store: &Store, output: OutputFormat) -> Result<()> {
    use std::collections::{BTreeMap, HashMap, HashSet};

    let graph = query::graph(store)?;
    if output == OutputFormat::Json {
        return print_json(&graph);
    }
    println!(
        "{}",
        format!(
            "ADR relationship graph · {} ADRs · {} edges",
            graph.nodes.len(),
            graph.edges.len()
        )
        .bold()
    );

    // Look up a node's status/title by its reference.
    let node: HashMap<&str, &adroit::view::GraphNode> = graph
        .nodes
        .iter()
        .map(|n| (n.reference.as_str(), n))
        .collect();
    // Group outgoing edges by source — BTreeMap keeps the output sorted/stable.
    let mut outgoing: BTreeMap<&str, Vec<&adroit::view::GraphEdge>> = BTreeMap::new();
    for e in &graph.edges {
        outgoing.entry(e.from.as_str()).or_default().push(e);
    }
    // Every ref that participates in at least one edge.
    let connected: HashSet<&str> = outgoing
        .keys()
        .copied()
        .chain(graph.edges.iter().map(|e| e.to.as_str()))
        .collect();

    for (from, edges) in &outgoing {
        match node.get(from) {
            Some(n) => println!(
                "\n{} {} [{}]",
                from.bold(),
                n.title.dimmed(),
                status_color(n.status)
            ),
            None => println!("\n{}", from.bold()),
        }
        for (i, e) in edges.iter().enumerate() {
            let connector = if i + 1 == edges.len() {
                "└─"
            } else {
                "├─"
            };
            let to_title = node
                .get(e.to.as_str())
                .map(|n| n.title.as_str())
                .unwrap_or("");
            println!(
                "  {} {} {} {} {}",
                connector.dimmed(),
                edge_label(e.kind),
                "→".dimmed(),
                e.to.bold(),
                to_title.dimmed()
            );
        }
    }

    // Isolated ADRs (no relationships) — a dim footnote.
    let isolated: Vec<&str> = graph
        .nodes
        .iter()
        .map(|n| n.reference.as_str())
        .filter(|r| !connected.contains(r))
        .collect();
    if !isolated.is_empty() {
        println!(
            "\n{} {}",
            "unconnected:".dimmed(),
            isolated.join(", ").dimmed()
        );
    }
    Ok(())
}

/// `adroit related <ID>` / `dedupe <ID>`: mechanical TF-IDF similarity over the
/// corpus (no AI). `related` excludes ADRs already linked to the target; `dedupe`
/// shows all overlaps (framed for catching duplicates). Read-only; `-o json`
/// emits the ranked matches.
/// Assemble the corpus as similarity docs: each ADR's address (id) + reference +
/// title + (title+body) text. Shared by `related` / `dedupe` / `ask`.
fn corpus_docs(store: &Store, cfg: &Config) -> Result<Vec<adroit::similar::Doc>> {
    let summaries = query::summaries(store, &Filter::default())?;
    let mut docs = Vec::with_capacity(summaries.len());
    for s in &summaries {
        let body = resolve_ref(cfg, &s.address)
            .ok()
            .and_then(|rr| store.find_path_by_ref(&rr).ok())
            .and_then(|p| store.read(&p).ok())
            .map(|a| a.body)
            .unwrap_or_default();
        docs.push(adroit::similar::Doc {
            id: s.address.clone(),
            reference: s.reference.clone(),
            title: s.title.clone(),
            text: format!("{} {}", s.title, body),
        });
    }
    Ok(docs)
}

/// `adroit ask "<question>"`: answer a question grounded in the ADR corpus.
/// Retrieval is mechanical (TF-IDF over the question); the configured AI provider
/// synthesizes the answer with citations. Read-only; `-o json` emits
/// `{answer, sources}`.
fn cmd_ask(store: &Store, cfg: &Config, question: &str, output: OutputFormat) -> Result<()> {
    let provider = require_provider(cfg, "ask")?;
    let mut docs = corpus_docs(store, cfg)?;
    if docs.is_empty() {
        anyhow::bail!("no ADRs to answer from");
    }
    // Rank the corpus against the question (added as a transient target doc).
    docs.push(adroit::similar::Doc {
        id: "__query__".to_string(),
        reference: String::new(),
        title: String::new(),
        text: question.to_string(),
    });
    let top: Vec<adroit::similar::Match> = adroit::similar::rank(&docs, "__query__")
        .into_iter()
        .take(5)
        .collect();

    let mut context = String::new();
    for m in &top {
        if let Some(d) = docs.iter().find(|d| d.id == m.id) {
            let excerpt: String = d.text.chars().take(800).collect();
            context.push_str(&format!("### {} — {}\n{excerpt}\n\n", d.reference, d.title));
        }
    }
    if context.is_empty() {
        context.push_str("(no closely matching ADRs)");
    }

    let req = adroit::ai::build_ask_request(question, &context);
    announce_estimate(
        provider.as_ref(),
        &req,
        &format!("Asking over {} ADR(s)", top.len()),
    );
    let answer = adroit::ai::draft_ask(provider.as_ref(), question, &context)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let answer = answer.trim();

    if output == OutputFormat::Json {
        let sources: Vec<&str> = top.iter().map(|m| m.reference.as_str()).collect();
        return print_json(&serde_json::json!({ "answer": answer, "sources": sources }));
    }
    println!("{answer}");
    if !top.is_empty() {
        let refs: Vec<&str> = top.iter().map(|m| m.reference.as_str()).collect();
        eprintln!("\n(sources: {})", refs.join(", "));
    }
    Ok(())
}

fn cmd_related(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    dedupe: bool,
    output: OutputFormat,
) -> Result<()> {
    use std::collections::HashSet;

    let target_path = store.find_path_by_ref(r)?;
    let target = query::detail_at(store, &target_path)?;
    let linked: HashSet<&str> = target.related.iter().map(|l| l.address.as_str()).collect();

    let docs = corpus_docs(store, cfg)?;

    let shown: Vec<adroit::similar::Match> = adroit::similar::rank(&docs, &target.summary.address)
        .into_iter()
        .filter(|m| dedupe || !linked.contains(m.id.as_str()))
        .take(3)
        .collect();

    if output == OutputFormat::Json {
        return print_json(&shown);
    }
    if shown.is_empty() {
        let what = if dedupe {
            "overlapping"
        } else {
            "unlinked related"
        };
        println!("No {what} ADRs found for {}", target.summary.reference);
        return Ok(());
    }
    let header = if dedupe {
        "Possible overlaps (already decided?)"
    } else {
        "Related — consider linking"
    };
    println!("{} for {}:", header.bold(), target.summary.reference.bold());
    for m in &shown {
        // Color the similarity score by strength.
        let score = format!("{:.2}", m.score);
        let score = if m.score >= 0.5 {
            score.green()
        } else if m.score >= 0.25 {
            score.yellow()
        } else {
            score.dimmed()
        };
        println!("  {score}  {} {}", m.reference.bold(), m.title.dimmed());
    }
    Ok(())
}

/// Resolve an AI provider for `verb`, or a clear error that distinguishes "this
/// binary wasn't built with the `ai` feature" from "AI isn't enabled / no key".
fn require_provider(cfg: &Config, verb: &str) -> Result<Box<dyn adroit::ai::AiProvider>> {
    if let Some(p) = adroit::ai_hook::open_provider(cfg) {
        return Ok(p);
    }
    if cfg!(feature = "ai") {
        anyhow::bail!(
            "`{verb}` needs an AI provider: set `ai.enabled` (or `ADROIT_AI_ENABLED=true`) and \
             `ADROIT_ANTHROPIC_KEY` in config / `.env`"
        )
    }
    anyhow::bail!(
        "`{verb}` needs the AI feature, which this binary was not built with — rebuild with \
         `just build-ai` (`cargo build --features ai`), then enable it via `ai.enabled` / \
         `ADROIT_AI_ENABLED`"
    )
}

/// `adroit summarize <ID>`: a one-paragraph AI TL;DR of an ADR (read-only).
/// Prints to stdout unless `--out`. Needs a provider.
fn cmd_summarize(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let provider = require_provider(cfg, "summarize")?;
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    let req = adroit::ai::build_summary_request(&detail.summary.title, &detail.body);
    announce_estimate(
        provider.as_ref(),
        &req,
        &format!("Summarizing {}", detail.summary.reference),
    );
    let summary = adroit::ai::draft_summary(provider.as_ref(), &detail.summary.title, &detail.body)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let summary = summary.trim();
    match out {
        Some(p) => {
            std::fs::write(p, format!("{summary}\n"))?;
            println!("Wrote summary to {}", p.display());
        }
        None => println!("{summary}"),
    }
    Ok(())
}

/// `adroit plan <ID>`: an AI implementation plan for an (accepted) ADR. Reads the
/// ADR + corpus, asks the provider for an ordered checklist, and prints it (or
/// writes `--out`). Read-only — the ADR is never modified.
fn cmd_plan(store: &Store, cfg: &Config, r: &AdrRef, out: Option<&std::path::Path>) -> Result<()> {
    let provider = require_provider(cfg, "plan")?;
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    // Corpus context: the other ADRs' `reference — title` lines (exclude self).
    let corpus: Vec<String> = query::summaries(store, &Filter::default())?
        .iter()
        .filter(|s| s.address != detail.summary.address)
        .map(|s| format!("{} — {}", s.reference, s.title))
        .collect();
    let req = adroit::ai::build_plan_request(&detail.summary.title, &detail.body, &corpus);
    announce_estimate(
        provider.as_ref(),
        &req,
        &format!("Planning {}", detail.summary.reference),
    );
    let plan = adroit::ai::draft_plan(
        provider.as_ref(),
        &detail.summary.title,
        &detail.body,
        &corpus,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    match out {
        Some(p) => {
            std::fs::write(p, &plan)?;
            println!("Wrote implementation plan to {}", p.display());
        }
        None => println!("{plan}"),
    }
    Ok(())
}

/// `adroit lint <ID>`: authoring-quality checks on one ADR (read-only).
/// Mechanical findings by default; `--ai` adds a model review. Exits non-zero on
/// mechanical findings (distinct from `check`'s structural gate). `-o json` emits
/// the findings.
fn cmd_lint(
    store: &Store,
    cfg: &Config,
    r: &AdrRef,
    ai_review: bool,
    output: OutputFormat,
) -> Result<()> {
    let path = store.find_path_by_ref(r)?;
    let detail = query::detail_at(store, &path)?;
    let mut findings = adroit::lint::lint(&detail.body);
    let mechanical = findings.len();

    if ai_review {
        let provider = require_provider(cfg, "lint --ai")?;
        let corpus: Vec<String> = query::summaries(store, &Filter::default())?
            .iter()
            .filter(|s| s.address != detail.summary.address)
            .map(|s| format!("{} — {}", s.reference, s.title))
            .collect();
        let req = adroit::ai::build_lint_request(&detail.summary.title, &detail.body, &corpus);
        announce_estimate(
            provider.as_ref(),
            &req,
            &format!("Reviewing {}", detail.summary.reference),
        );
        let review = adroit::ai::draft_lint(
            provider.as_ref(),
            &detail.summary.title,
            &detail.body,
            &corpus,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        findings.push(adroit::lint::LintFinding {
            source: adroit::lint::LintSource::Ai,
            message: review.trim().to_string(),
        });
    }

    if output == OutputFormat::Json {
        print_json(&findings)?;
    } else if findings.is_empty() {
        println!("OK: no lint findings for {}", detail.summary.reference);
    } else {
        for f in &findings {
            println!("[{}] {}", f.source, f.message);
        }
    }

    // Exit non-zero on mechanical findings (the AI review is advisory).
    if mechanical > 0 {
        anyhow::bail!("{mechanical} authoring finding(s)");
    }
    Ok(())
}

/// `adroit auth`: save a forge token to the local credential store.
#[cfg(feature = "forge")]
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
    // `store_credential` reports where it landed (OS keychain or the file store);
    // the token value itself is never echoed.
    let where_stored = config::store_credential(provider, &token)?;
    if provider == "jira"
        && let Some(e) = email
    {
        config::store_credential("jira_email", &e)?;
    }
    println!(
        "Saved {provider} token to the {where_stored} (environment variables still take precedence)."
    );
    Ok(())
}

/// `adroit init`: detect the forge from the git remote and write the config.
#[cfg(feature = "forge")]
fn cmd_init(store: &Store, print_only: bool, yes: bool) -> Result<()> {
    use dialoguer::{Confirm, Input, Select};

    let root = store.root();
    let detected = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .as_deref()
        .and_then(config::parse_remote_url);

    // `--print`: report the detection + the steps, write nothing, ask nothing.
    if print_only {
        match &detected {
            Some((p, repo, host)) => {
                let h = host
                    .as_deref()
                    .map(|h| format!(" @ {h}"))
                    .unwrap_or_default();
                println!("Detected forge: {p} {repo}{h}");
            }
            None => println!("No GitHub/GitLab `origin` remote detected — the wizard will prompt."),
        }
        println!("\n(--print: nothing written.) `adroit init` walks through:");
        println!("  • confirm provider/repo + choose the issue tracker → write forge.* to config");
        println!("  • optionally write ./.env (ADROIT_DIR; the token stays in your shell env)");
        println!("  • optionally drop a repo-local adr-template.md (MADR) to customize");
        println!("  • optionally install a pre-commit hook running `adroit check`");
        println!(
            "\n`adroit init --yes` does the full setup non-interactively (detected forge + native tracker)."
        );
        return Ok(());
    }

    // 1. Provider / repo / host — confirm the detected one, else prompt.
    let (provider, repo, host) = match detected {
        Some(d)
            if yes
                || Confirm::new()
                    .with_prompt(format!("Use detected forge: {} {}?", d.0, d.1))
                    .default(true)
                    .interact()? =>
        {
            d
        }
        Some(_) | None if yes => {
            anyhow::bail!(
                "no GitHub/GitLab `origin` remote detected — run `adroit init` (without --yes) to enter it"
            );
        }
        _ => {
            let choices = ["github", "gitlab"];
            let idx = Select::new()
                .with_prompt("Forge provider")
                .items(choices)
                .default(0)
                .interact()?;
            let provider = if idx == 1 {
                config::Provider::Gitlab
            } else {
                config::Provider::Github
            };
            let repo: String = Input::new()
                .with_prompt("Repo slug (owner/repo or group/project)")
                .interact_text()?;
            let host: String = Input::new()
                .with_prompt("API host (blank = provider default)")
                .allow_empty(true)
                .interact_text()?;
            let host = host.trim();
            (provider, repo, (!host.is_empty()).then(|| host.to_string()))
        }
    };

    // 2. Issue tracker.
    let (tracker, tracker_project, tracker_host) = if yes {
        (config::TrackerProvider::Native, None, None)
    } else {
        let choices = ["native (the forge's own issues)", "jira"];
        let idx = Select::new()
            .with_prompt("Issue tracker")
            .items(choices)
            .default(0)
            .interact()?;
        if idx == 1 {
            let key: String = Input::new()
                .with_prompt("Jira project key (e.g. OPS)")
                .interact_text()?;
            let h: String = Input::new()
                .with_prompt("Jira host (site.atlassian.net, or a self-hosted host)")
                .interact_text()?;
            (config::TrackerProvider::Jira, Some(key), Some(h))
        } else {
            (config::TrackerProvider::Native, None, None)
        }
    };

    // 3. Write forge.* to config (preserving other keys). Match every provider
    // explicitly so a future one can't silently inherit GitHub's token env var.
    let token_env = match provider {
        config::Provider::Gitlab => "ADROIT_GITLAB_TOKEN",
        config::Provider::Github | config::Provider::None => "ADROIT_GITHUB_TOKEN",
    };
    let mut cfg = config::Config::load()?;
    let f = cfg.forge.get_or_insert_with(Default::default);
    f.provider = provider;
    f.repo = Some(repo);
    f.host = host;
    f.tracker = tracker;
    f.tracker_project = tracker_project;
    f.tracker_host = tracker_host;
    cfg.save()?;
    let cfg_path = config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!("✓ wrote forge.* to {cfg_path}");

    // 4. ./.env — ADROIT_DIR only; the token stays in the shell env (never written).
    if yes
        || Confirm::new()
            .with_prompt("Write ./.env with ADROIT_DIR (token stays in your shell)?")
            .default(true)
            .interact()?
    {
        let env_path = std::path::Path::new(".env");
        config::upsert_env_file(env_path, "ADROIT_DIR", &root.display().to_string())?;
        println!("✓ wrote .env (ADROIT_DIR)");
    }

    // 5. Repo-local adr-template.md (MADR) for the team to customize.
    if yes
        || Confirm::new()
            .with_prompt("Drop a repo-local adr-template.md (MADR) to customize?")
            .default(false)
            .interact()?
    {
        let tpath = root.join("adr-template.md");
        if tpath.exists() {
            println!("  adr-template.md already exists — left as-is.");
        } else {
            std::fs::write(&tpath, adroit::template::MADR)?;
            println!("✓ wrote {}", tpath.display());
        }
    }

    // 6. Pre-commit hook running `adroit check`.
    if yes
        || Confirm::new()
            .with_prompt("Install a git pre-commit hook running `adroit check`?")
            .default(false)
            .interact()?
    {
        install_precommit_hook(root)?;
    }

    println!(
        "\nDone. Set your token in the environment: export {token_env}=<token>  (never commit it)."
    );
    Ok(())
}

/// The `adroit check` pre-commit hook body.
#[cfg(feature = "forge")]
fn precommit_hook_script() -> &'static str {
    "#!/bin/sh\n# Installed by `adroit init`: validate the ADR repo before each commit.\nadroit check\n"
}

/// Install (without overwriting) a pre-commit hook that runs `adroit check`,
/// resolving the hooks dir from git so it works with worktrees / custom git dirs.
#[cfg(feature = "forge")]
fn install_precommit_hook(adr_dir: &std::path::Path) -> Result<()> {
    let git_dir = std::process::Command::new("git")
        .arg("-C")
        .arg(adr_dir)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string()));
    let Some(git_dir) = git_dir else {
        eprintln!("  not a git work tree — skipped the pre-commit hook.");
        return Ok(());
    };
    let hook = git_dir.join("hooks").join("pre-commit");
    if hook.exists() {
        eprintln!(
            "  a pre-commit hook already exists at {} — left as-is.",
            hook.display()
        );
        return Ok(());
    }
    if let Some(parent) = hook.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&hook, precommit_hook_script())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("✓ installed pre-commit hook at {}", hook.display());
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
#[cfg(feature = "forge")]
fn cmd_notify(store: &Store, cfg: &Config, id: &str, dry_run: bool) -> Result<()> {
    let webhook = std::env::var("ADROIT_NOTIFY_WEBHOOK").map_err(|_| {
        anyhow::anyhow!("set ADROIT_NOTIFY_WEBHOOK to a Slack/Teams incoming-webhook URL")
    })?;
    let r = resolve_ref(cfg, id)?;
    let path = store.find_path_by_ref(&r)?;
    let detail = query::detail_at(store, &path)?;
    let s = &detail.summary;
    let text = format!("*{}: {}* — {}", s.reference, s.title, s.status);
    // Print success only when it was actually posted — not on a dry run, a
    // failed/unreachable webhook, or a build without the `forge` feature.
    if adroit::forge_hook::notify(&webhook, &text, dry_run)? {
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
