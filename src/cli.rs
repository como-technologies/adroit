use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

use crate::config::{DateSource, Layout, MarkdownTheme, RelinkScope};
use crate::format::Format;
use crate::naming::NamingScheme;

/// How a read verb prints its result.
///
/// `human` (default) is the formatted text rendering; `json` emits the
/// [`crate::view`] types verbatim — the same structured contract the web API
/// returns — for scripts and AI agents that drive adroit's CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

/// A snappy tool for managing Architecture Decision Records.
#[derive(Debug, Parser)]
// clap 4 can't group subcommands under headings, so the command list is
// hand-curated into workflow categories via help_template. The forge build adds
// a "Forge integration" section; the no-forge build omits it (those commands are
// `#[cfg]`-gated away). The `commands_are_all_grouped` test guards against drift
// in whichever build runs it.
// `-h` and `--help` show the SAME concise help (command list + the everyday
// options); `--help-all` shows every option in full. Implemented with the
// canonical clap recipe (disable the built-in flag, then HelpShort/HelpLong
// custom flags); the config-override options carry `hide_short_help` so they
// appear only under `--help-all`. `max_term_width` caps wrapping on wide
// terminals (from `cli: tighten --help output`).
#[command(
    name = "adroit",
    version,
    about,
    max_term_width = 100,
    disable_help_flag = true,
    after_help = "Run `adroit --help-all` to see every option, or `adroit <command> --help` for one command."
)]
#[cfg_attr(
    feature = "forge",
    command(help_template = "\
{about-with-newline}
{usage-heading} {usage}

Author a decision:
  new           Create a new ADR (--interview for an AI Q&A draft)
  draft         Fill in an existing ADR via the AI interview
  compose       Revise an ADR's body from a free-form instruction (AI)
  plan          Draft an AI implementation plan for an ADR
  edit          Open an ADR in your editor ($EDITOR / $VISUAL)
  lint          Check one ADR's authoring quality (--ai for a model review)
  dedupe        Find existing ADRs that overlap a new one
  related       Find similar ADRs to link (mechanical)
  link          Add or remove a typed link between two ADRs

Review & decide:
  set-review    Set or clear an ADR's review deadline
  review        Generate a review-kickoff doc for an ADR
  summarize     One-paragraph AI TL;DR of an ADR
  set-status    Change an ADR's status (moves the file in by_status)
  supersede     Mark an older ADR superseded by a newer one

Explore the corpus:
  list          List ADRs
  show          Show one ADR by its identifier
  status        Print an ADR's status (lowercase, scriptable)
  search        Search ADRs by title and body
  stats         Show repo statistics (status counts, ages, growth)
  graph         Print the ADR relationship graph
  ask           Ask the corpus a question (AI answer + citations)
  serve         Serve the read-only web dashboard

Maintain the repo:
  check         Validate the repo (exits non-zero on problems)
  relink        Rewrite cross-ADR links to current locations
  renumber      Renumber an ADR to resolve a number collision
  migrate       Convert the repo to the configured layout/format
  index         Regenerate the ADR section of SUMMARY.md
  publish       Export the accepted ADR set to a directory

Forge integration:
  init          Detect the forge from the git remote and configure it
  auth          Store a forge token in the local credential store
  sync          Refresh a linked PR/MR description from the ADR
  reconcile     Sync local status with the forge after out-of-band changes
  notify        Post an ADR's state to a chat webhook

Configuration:
  config        Inspect or change configuration
  completions   Print a shell completion script (bash/zsh/fish/…)
  manifest      Print a machine-readable CLI manifest (JSON, for agents)
  help          Print help for a command

Options:
{options}{after-help}")
)]
#[cfg_attr(
    not(feature = "forge"),
    command(help_template = "\
{about-with-newline}
{usage-heading} {usage}

Author a decision:
  new           Create a new ADR (--interview for an AI Q&A draft)
  draft         Fill in an existing ADR via the AI interview
  compose       Revise an ADR's body from a free-form instruction (AI)
  plan          Draft an AI implementation plan for an ADR
  edit          Open an ADR in your editor ($EDITOR / $VISUAL)
  lint          Check one ADR's authoring quality (--ai for a model review)
  dedupe        Find existing ADRs that overlap a new one
  related       Find similar ADRs to link (mechanical)
  link          Add or remove a typed link between two ADRs

Review & decide:
  set-review    Set or clear an ADR's review deadline
  review        Generate a review-kickoff doc for an ADR
  summarize     One-paragraph AI TL;DR of an ADR
  set-status    Change an ADR's status (moves the file in by_status)
  supersede     Mark an older ADR superseded by a newer one

Explore the corpus:
  list          List ADRs
  show          Show one ADR by its identifier
  status        Print an ADR's status (lowercase, scriptable)
  search        Search ADRs by title and body
  stats         Show repo statistics (status counts, ages, growth)
  graph         Print the ADR relationship graph
  ask           Ask the corpus a question (AI answer + citations)
  serve         Serve the read-only web dashboard

Maintain the repo:
  check         Validate the repo (exits non-zero on problems)
  relink        Rewrite cross-ADR links to current locations
  renumber      Renumber an ADR to resolve a number collision
  migrate       Convert the repo to the configured layout/format
  index         Regenerate the ADR section of SUMMARY.md
  publish       Export the accepted ADR set to a directory

Configuration:
  config        Inspect or change configuration
  completions   Print a shell completion script (bash/zsh/fish/…)
  manifest      Print a machine-readable CLI manifest (JSON, for agents)
  help          Print help for a command

Options:
{options}{after-help}")
)]
pub struct Cli {
    // --- Config / repo-selection flags --------------------------------------
    // `--dir` is the everyday global, shown in the concise help. The on-disk
    // shape + behavior flags below stay `global` (so they bind on any command,
    // per `cli: tighten --help output`) but carry `hide_short_help`, so they
    // surface only under `--help-all` rather than being fully hidden — set them
    // once via `adroit config` / the `ADROIT_*` env vars.
    /// ADR directory (overrides config; default `~/.local/share/adroit/`). [env: ADROIT_DIR]
    #[arg(short, long, global = true, env = "ADROIT_DIR")]
    pub dir: Option<PathBuf>,

    /// On-disk format: `markdown` or `frontmatter` (overrides config) [env: ADROIT_FORMAT]
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_FORMAT",
        hide_short_help = true
    )]
    pub format: Option<Format>,

    /// Directory layout: `by_status`, `by_category`, or `flat` (overrides config) [env: ADROIT_LAYOUT]
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_LAYOUT",
        hide_short_help = true
    )]
    pub layout: Option<Layout>,

    /// ADR naming scheme: `sequential`, `date`, `uuid`, `per_category` (overrides config) [env: ADROIT_NAMING]
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_NAMING",
        hide_short_help = true
    )]
    pub naming: Option<NamingScheme>,

    /// Date/lifecycle source: `auto`, `git`, `filesystem` (overrides config) [env: ADROIT_DATE_SOURCE]
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_DATE_SOURCE",
        hide_short_help = true
    )]
    pub date_source: Option<DateSource>,

    /// Link-rewrite scope on status moves: `all`, `self`, `none` (overrides config) [env: ADROIT_RELINK_SCOPE]
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_RELINK_SCOPE",
        hide_short_help = true
    )]
    pub relink_scope: Option<RelinkScope>,

    // These are NOT `global` (only a few commands use each), but the env var
    // still binds and they show under `--help-all`.
    /// TUI color theme: `gruvbox` (default), `warm`, or `default` (overrides config) [env: ADROIT_THEME]
    #[arg(long, value_enum, env = "ADROIT_THEME", hide_short_help = true)]
    pub theme: Option<MarkdownTheme>,

    /// Default template for `new` — a built-in name or path (overrides config) [env: ADROIT_TEMPLATE]
    #[arg(long, env = "ADROIT_TEMPLATE", hide_short_help = true)]
    pub default_template: Option<String>,

    /// Days before a Proposed ADR with no deadline is flagged review-due; `0` disables (overrides config) [env: ADROIT_REVIEW_OVERDUE_DAYS]
    #[arg(long, env = "ADROIT_REVIEW_OVERDUE_DAYS", hide_short_help = true)]
    pub review_overdue_days: Option<u32>,

    /// Output format for read verbs: `human` (default) or `json`.
    ///
    /// `json` emits the structured `view` types — the same contract the web API
    /// returns — for scripts and AI agents. Honored by `list` / `show` /
    /// `search` / `stats` / `graph` / `check`; other verbs ignore it.
    #[arg(
        short = 'o',
        long,
        value_enum,
        global = true,
        default_value_t = OutputFormat::Human,
        help_heading = "Output"
    )]
    pub output: OutputFormat,

    /// Print help — the same concise view for `-h` and `--help`.
    #[arg(
        short = 'h',
        long = "help",
        action = clap::ArgAction::HelpShort,
        global = true
    )]
    pub help: Option<bool>,

    /// Print help including every option in full detail.
    #[arg(long = "help-all", action = clap::ArgAction::HelpLong, global = true)]
    pub help_all: Option<bool>,

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
        /// Category subdirectory for the new ADR (required by the by_category
        /// layout; rejected by the others).
        #[arg(short, long)]
        category: Option<String>,
        /// Create even if an ADR with this exact title already exists (skip the
        /// duplicate guard). `new` always allocates a fresh number regardless.
        #[arg(long)]
        force: bool,
        /// Run a short Socratic interview and have the configured AI provider
        /// draft the ADR body from your answers + the existing corpus. The draft
        /// is marked `<!-- adroit:ai-suggested -->` for you to review/edit before
        /// commit. Opt-in; needs `ai.enabled` (build with `--features ai`) or the
        /// `ADROIT_AI_FAKE` test seam.
        #[arg(long)]
        interview: bool,
        /// Also create the linked tracker issue + a draft PR and record their
        /// URLs in `## References` (opt-in; requires a configured `forge`).
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, preview the forge actions without performing them.
        #[cfg(feature = "forge")]
        #[arg(long)]
        dry_run: bool,
    },
    /// List existing ADRs.
    List {
        /// Only show ADRs with this status.
        #[arg(short, long)]
        status: Option<String>,
        /// Enrich each row with live forge state (PR approvals/CI). Reads the
        /// forge; requires a configured `forge` + the feature build.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
    },
    /// Show a single ADR by its identifier.
    Show {
        /// ADR identifier: a number (`9`/`ADR-0009`) under the sequential scheme,
        /// or the slug / uuid prefix under the date / uuid schemes.
        id: String,
    },
    /// Summarize an ADR in one paragraph via AI (read-only).
    ///
    /// A plain-language TL;DR for a PR description, a chat notification, or a
    /// decision-log entry. Prints to stdout unless `--out <PATH>`. Needs an AI
    /// provider (`ai.enabled` with `--features ai`, or the `ADROIT_AI_FAKE` seam).
    Summarize {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Write the summary to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Print an ADR's current status — lowercase and scriptable, so it feeds
    /// straight into `set-status` or a shell test. Use `show` for the full record.
    Status {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
    },
    /// Set an ADR's status (moves the file in by_status layout, rewrites links).
    SetStatus {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// New status (proposed, accepted, rejected, deprecated, superseded).
        status: String,
        /// Also drive the forge: on `accepted` verify approvals/CI then merge the
        /// PR + close the issue; on `rejected`/`deprecated` close them. Opt-in;
        /// requires a configured `forge`.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, preview the forge actions and make no changes.
        #[cfg(feature = "forge")]
        #[arg(long)]
        dry_run: bool,
        /// With `--forge`, apply (e.g. merge the PR). Without it, preview.
        #[cfg(feature = "forge")]
        #[arg(long)]
        yes: bool,
    },
    /// Mark an older ADR as superseded by a newer one.
    Supersede {
        /// The new (superseding) ADR identifier (number, slug, or uuid prefix).
        new: String,
        /// The old (superseded) ADR identifier (number, slug, or uuid prefix).
        old: String,
        /// Also comment on + close the superseded ADR's forge issue/PR. Opt-in.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, preview the forge actions and make no changes.
        #[cfg(feature = "forge")]
        #[arg(long)]
        dry_run: bool,
        /// With `--forge`, apply. Without it, preview.
        #[cfg(feature = "forge")]
        #[arg(long)]
        yes: bool,
    },
    /// Add (or remove with `--remove`) a typed relational link between two ADRs.
    ///
    /// Records the link in `<id>`'s frontmatter so it shows in `show` and the
    /// dashboard's relations graph. Exactly one of `--relates-to` / `--depends-on`
    /// / `--refines` names the target. Requires the frontmatter format.
    Link {
        /// The source ADR identifier (number, slug, or uuid prefix).
        id: String,
        /// Link to a related ADR (non-directional).
        #[arg(long, group = "kind", value_name = "TARGET")]
        relates_to: Option<String>,
        /// Link to an ADR this one depends on.
        #[arg(long, group = "kind", value_name = "TARGET")]
        depends_on: Option<String>,
        /// Link to an ADR this one refines / elaborates.
        #[arg(long, group = "kind", value_name = "TARGET")]
        refines: Option<String>,
        /// Remove the link instead of adding it.
        #[arg(long)]
        remove: bool,
    },
    /// Set (or clear) an ADR's review deadline (ISO-8601 `YYYY-MM-DD`).
    ///
    /// A still-`Proposed` ADR whose deadline has passed is flagged review-due
    /// in stats and the dashboard. Pass `--clear` to remove the deadline.
    SetReview {
        /// ADR identifier to set the review deadline on (number, slug, or uuid
        /// prefix — see `show`).
        id: String,
        /// Review deadline as `YYYY-MM-DD`. Omit together with `--clear`.
        #[arg(required_unless_present = "clear")]
        date: Option<String>,
        /// Remove the review deadline instead of setting one.
        #[arg(long, conflicts_with = "date")]
        clear: bool,
        /// Also mirror the deadline as a comment on the ADR's linked issue/PR.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, preview the comment without posting.
        #[cfg(feature = "forge")]
        #[arg(long)]
        dry_run: bool,
        /// With `--forge`, post the comment (without it, preview).
        #[cfg(feature = "forge")]
        #[arg(long)]
        yes: bool,
    },
    /// Search ADRs by title and body (case-insensitive).
    Search {
        /// Term to search for.
        term: String,
    },
    /// Validate the ADR repo and exit non-zero if any problem is found.
    ///
    /// A structural CI gate: checks for status/directory mismatches,
    /// duplicate numbers, unparseable files, broken supersession links, and
    /// broken/stale cross-ADR relative links.
    Check {
        /// Also run forge-aware checks (issue/PR drift). Reads the forge over
        /// the network; requires a configured `forge` + the feature build.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
    },
    /// Lint an ADR draft for authoring quality (read-only).
    ///
    /// Mechanical checks by default — sections still left as their `_…_` prompt,
    /// no honest negative consequences, only one option considered. `--ai` adds a model
    /// review against ADR best practices + house style. Exits non-zero on
    /// mechanical findings; distinct from `check` (structural repo validity).
    Lint {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Also run an AI review (needs a configured AI provider).
        #[arg(long)]
        ai: bool,
    },
    /// Print repo statistics: per-status counts, proposed-age rows (review-due
    /// flagged), and a created-per-period histogram.
    ///
    /// `-o json` emits `view::Stats`; the human view is a compact summary.
    Stats,
    /// Print the ADR relationship graph — supersession + typed-link edges.
    ///
    /// Most useful with `-o json` (`view::Graph`, nodes + edges); the human view
    /// summarizes counts.
    Graph,
    /// Find ADRs textually similar to this one that it isn't already linked to.
    ///
    /// Mechanical (TF-IDF over titles + bodies) — no AI. Surfaces ADRs you may
    /// want to `link`. `-o json` emits the ranked matches.
    Related {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
    },
    /// Surface existing ADRs that overlap a (new) one — "did we already decide this?"
    ///
    /// Like `related`, but includes already-linked ADRs and is framed for
    /// catching duplicates before a decision is re-litigated. Mechanical; no AI.
    Dedupe {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
    },
    /// Ask a question of the ADR corpus and get an AI answer with citations.
    ///
    /// Retrieval is mechanical (TF-IDF over your question); the configured AI
    /// provider synthesizes the answer, citing the ADRs it used. Read-only;
    /// `-o json` emits `{answer, sources}`. Needs an AI provider.
    Ask {
        /// The natural-language question (quote it).
        question: String,
    },
    /// Rewrite cross-ADR relative links to each ADR's current location.
    ///
    /// Fixes links left stale by status-change file moves (run by hand or in
    /// CI). Status changes already relink automatically; this repairs a repo
    /// edited outside adroit. Idempotent.
    Relink {
        /// Show which files/links would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Also refresh each linked PR's description to the current ADR (patches
        /// forge-side links after status moves). Requires a configured `forge`.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, apply the PR updates (without it, preview).
        #[cfg(feature = "forge")]
        #[arg(long)]
        yes: bool,
    },
    /// Refresh an ADR's linked PR description from its content (MR-desc sync).
    ///
    /// Writes the ADR (with a `<!-- adroit:adr=… -->` marker) into the PR body so
    /// reviewers always see the latest text. Requires a configured `forge`.
    #[cfg(feature = "forge")]
    Sync {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Preview without changing the PR.
        #[arg(long)]
        dry_run: bool,
        /// Apply the change (without it, preview).
        #[arg(long)]
        yes: bool,
    },
    /// Reconcile local ADR status with the forge after out-of-band changes.
    ///
    /// Scans linked PRs/issues and reports drift (an MR merged or a tracker
    /// issue closed *outside* adroit). With `--yes`, fixes the clear case — a
    /// merged PR whose ADR isn't accepted — by moving it to `accepted/`.
    /// Read-only on the forge.
    #[cfg(feature = "forge")]
    Reconcile {
        /// Apply the fixable drift (default: report only).
        #[arg(long)]
        yes: bool,
    },
    /// Renumber an ADR (sequential scheme) to resolve a collision.
    ///
    /// Renames the file (slug preserved), rewrites its `# ADR-NNNN:` heading and
    /// every inbound reference (label + link), then relinks. `--file`
    /// disambiguates when two files share the old number.
    Renumber {
        /// Current ADR number.
        old: u32,
        /// New ADR number (must be unused).
        new: u32,
        /// The file to renumber, when two ADRs share `old`.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Convert the repo on disk to the configured layout/format.
    ///
    /// The source profile is auto-detected; files are moved between
    /// flat/by-status dirs and/or re-serialized markdown<->frontmatter, then
    /// cross-ADR links are fixed. Prints a preview by default — pass `--yes` to
    /// apply. Set the target via --layout / --format (or config / .env).
    Migrate {
        /// Apply the migration (default: preview only).
        #[arg(long)]
        yes: bool,
        /// Show what would change without writing (overrides `--yes`).
        #[arg(long)]
        dry_run: bool,
    },
    /// Save a forge token to the credential store (env vars still take precedence).
    ///
    /// Stored in the **OS keychain** when available (macOS Keychain / Windows
    /// Credential Manager / Linux keyutils), else a `0600` file next to the config.
    /// `ADROIT_CREDENTIAL_STORE=file|keychain` forces a specific backend.
    #[cfg(feature = "forge")]
    Auth {
        /// Which token to store: `github`, `gitlab`, `jira`, or `anthropic`
        /// (the AI key — same keychain/file store).
        provider: String,
        /// The token value (omit to be prompted, hidden).
        #[arg(long)]
        token: Option<String>,
        /// For `jira`: the account email saved alongside the token.
        #[arg(long)]
        email: Option<String>,
    },
    /// Interactive wizard to set up forge integration.
    ///
    /// Detects the provider/repo from the git remote (confirm or override),
    /// asks for the issue tracker, writes `forge.*` to config, and optionally
    /// writes `./.env` (ADROIT_DIR), drops a repo-local `adr-template.md`, and
    /// installs a pre-commit hook running `adroit check`. `--print` previews
    /// without writing; `--yes` does the full setup non-interactively.
    #[cfg(feature = "forge")]
    Init {
        /// Show the detected settings + planned steps without writing anything.
        #[arg(long)]
        print: bool,
        /// Non-interactive: use the detected forge + defaults and do the full setup.
        #[arg(long)]
        yes: bool,
    },
    /// Export the accepted ADR set to a directory (static-dir publisher).
    ///
    /// Sibling to `index`, but for the published-docs side. Copies every
    /// accepted ADR plus an `index.md` to `--out`. Idempotent.
    Publish {
        /// Output directory for the published ADRs.
        ///
        /// Long-only (`--out`); the short `-o` is the global `--output`
        /// (human/json) selector.
        #[arg(long)]
        out: PathBuf,
        /// Preview what would be written without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Post an ADR's current state to a chat webhook (Slack/Teams-compatible).
    ///
    /// Reads the incoming-webhook URL from `ADROIT_NOTIFY_WEBHOOK`. Requires the
    /// `forge` feature build (it uses the bundled HTTP client).
    #[cfg(feature = "forge")]
    Notify {
        /// ADR identifier to announce (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Preview the message without posting.
        #[arg(long)]
        dry_run: bool,
    },
    /// Regenerate the ADR section of SUMMARY.md (or print it to stdout).
    Index {
        /// Don't write — just verify SUMMARY.md is up to date (CI gate).
        ///
        /// Exits non-zero if SUMMARY.md differs from what `index` would write.
        #[arg(long)]
        check: bool,
    },
    /// Open an ADR in your editor ($EDITOR or $VISUAL).
    Edit {
        /// ADR identifier to edit (number, slug, or uuid prefix — see `show`).
        id: String,
    },
    /// Generate a review-kickoff doc for an ADR (prints to stdout by default).
    /// Generate an AI implementation plan for an (accepted) ADR.
    ///
    /// Reads the ADR + corpus and asks the configured AI provider for an ordered,
    /// actionable implementation checklist. Read-only — it never changes the ADR.
    /// Prints to stdout unless `--out <PATH>` is given. Needs an AI provider
    /// (`ai.enabled` with a `--features ai` build, or the `ADROIT_AI_FAKE` seam).
    Plan {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Write the plan to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run the AI interview on an existing ADR — the after-the-fact `new
    /// --interview`, for an ADR you created with a plain `new` (template) and want
    /// filled in before review.
    ///
    /// Asks the same Socratic questions, drafts the body, and splices it over the
    /// prose (identity / status / heading stay mechanical, marked
    /// `<!-- adroit:ai-suggested -->`), then opens your editor. Needs an AI provider.
    Draft {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// Do not open the editor after drafting.
        #[arg(long)]
        no_edit: bool,
    },
    /// Revise an existing ADR's body from a free-form instruction (AI).
    ///
    /// Unlike `draft` (which re-runs the fixed Socratic interview and redrafts the
    /// body wholesale), `compose` takes a free-form instruction (e.g. "expand the
    /// negative consequences", "add a rejected option about X") plus the ADR's
    /// *current* body, and returns a revised body — targeted, iterative editing.
    /// Prose only (identity / status / heading stay mechanical, marked
    /// `<!-- adroit:ai-suggested -->`), then opens your editor. Needs an AI provider.
    Compose {
        /// ADR identifier (number, slug, or uuid prefix — see `show`).
        id: String,
        /// What to change, in plain words (e.g. "tighten the context section").
        instruction: String,
        /// Do not open the editor after composing.
        #[arg(long)]
        no_edit: bool,
    },
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
        ///
        /// Long-only (`--out`); the short `-o` / `--output` is the global
        /// human/json result-format selector.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also post the kickoff as a comment on the ADR's linked issue/PR.
        #[cfg(feature = "forge")]
        #[arg(long)]
        forge: bool,
        /// With `--forge`, preview the comment without posting.
        #[cfg(feature = "forge")]
        #[arg(long)]
        dry_run: bool,
        /// With `--forge`, post the comment (without it, preview).
        #[cfg(feature = "forge")]
        #[arg(long)]
        yes: bool,
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
    /// Inspect or change configuration.
    ///
    /// `adroit config` (or `config show`) lists every setting with its resolved
    /// value and where it came from. `config get <key>` prints one value;
    /// `config set <key> <value>` persists to `config.yaml` (or the project
    /// `.env`, with `--local`).
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Print a shell completion script to stdout (bash, zsh, fish, …).
    ///
    /// Generated from the command tree, so it always matches this build (a
    /// no-forge build omits the forge commands). Load it with e.g.
    /// `. <(adroit completions bash)` or save it onto your shell's fpath.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print a machine-readable manifest of the CLI surface (JSON) for agents.
    ///
    /// Lists every command (only the ones compiled into this build), its args /
    /// flags / enums, whether it reads or writes, the `-o json` output shape, any
    /// runtime requirement (e.g. `ai.enabled`), and the JSON Schemas of the
    /// `view` types — so an agent can discover and drive adroit without scraping
    /// `--help`.
    #[cfg(feature = "manifest")]
    Manifest,
}

/// Subcommands for `adroit config`.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigAction {
    /// List every setting with its resolved value and source (the default).
    Show,
    /// Print one setting's resolved value.
    Get {
        /// Config key (e.g. `layout`, `format`, `date_source`).
        key: String,
    },
    /// Set a value — writes `config.yaml`, or the project `.env` with `--local`.
    Set {
        /// Config key (e.g. `layout`, `format`, `date_source`).
        key: String,
        /// New value (validated against the key's type).
        value: String,
        /// Write `KEY=value` to the project `.env` instead of `config.yaml`.
        #[arg(long)]
        local: bool,
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

    /// The top-level command list is hand-curated into workflow categories in
    /// `Cli`'s `help_template` (clap 4 can't group subcommands). This guards
    /// against drift: every real subcommand must appear in that grouped help, so
    /// adding a command without categorizing it fails the build.
    #[test]
    fn completions_generate_for_every_shell() {
        use clap::ValueEnum;
        for &shell in Shell::value_variants() {
            let mut cmd = Cli::command();
            let mut out = Vec::new();
            clap_complete::generate(shell, &mut cmd, "adroit", &mut out);
            assert!(!out.is_empty(), "{shell} completion script was empty");
        }
    }

    #[test]
    fn commands_are_all_grouped() {
        let help = Cli::command().render_help().to_string();
        for sub in Cli::command().get_subcommands() {
            let name = sub.get_name();
            // Match the command at the start of its listing line ("  <name>  …"),
            // so `status` isn't satisfied by `set-status`, nor `link` by `relink`.
            let listed = help
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("{name} ")));
            assert!(
                listed,
                "subcommand `{name}` is missing from Cli's grouped help_template — add it to a category"
            );
        }
    }
}
