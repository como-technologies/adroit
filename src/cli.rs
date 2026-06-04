use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

use crate::config::{DateSource, Layout, MarkdownTheme, RelinkScope};
use crate::format::Format;
use crate::naming::NamingScheme;

/// A snappy tool for managing Architecture Decision Records.
#[derive(Debug, Parser)]
// clap 4 can't group subcommands under headings, so the command list is
// hand-curated into workflow categories via help_template. The forge build adds
// a "Forge integration" section; the no-forge build omits it (those commands are
// `#[cfg]`-gated away). The `commands_are_all_grouped` test guards against drift
// in whichever build runs it.
#[command(name = "adroit", version, about)]
#[cfg_attr(
    feature = "forge",
    command(help_template = "\
{about-with-newline}
{usage-heading} {usage}

Authoring:
  new           Create a new ADR
  edit          Open an ADR in your editor ($EDITOR / $VISUAL)
  set-status    Change an ADR's status (moves the file in by_status)
  supersede     Mark an older ADR superseded by a newer one
  set-review    Set or clear an ADR's review deadline
  review        Generate a review-kickoff doc for an ADR
  link          Add or remove a typed link between two ADRs

Browse & inspect:
  list          List ADRs
  show          Show one ADR by its identifier
  status        Print an ADR's status (lowercase, scriptable)
  search        Search ADRs by title and body
  serve         Serve the read-only web dashboard

Repo health:
  check         Validate the repo (exits non-zero on problems)
  relink        Rewrite cross-ADR links to current locations
  renumber      Renumber an ADR to resolve a number collision
  migrate       Convert the repo to the configured layout/format

Publishing:
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
  help          Print help for a command

Options:
{options}{after-help}")
)]
#[cfg_attr(
    not(feature = "forge"),
    command(help_template = "\
{about-with-newline}
{usage-heading} {usage}

Authoring:
  new           Create a new ADR
  edit          Open an ADR in your editor ($EDITOR / $VISUAL)
  set-status    Change an ADR's status (moves the file in by_status)
  supersede     Mark an older ADR superseded by a newer one
  set-review    Set or clear an ADR's review deadline
  review        Generate a review-kickoff doc for an ADR
  link          Add or remove a typed link between two ADRs

Browse & inspect:
  list          List ADRs
  show          Show one ADR by its identifier
  status        Print an ADR's status (lowercase, scriptable)
  search        Search ADRs by title and body
  serve         Serve the read-only web dashboard

Repo health:
  check         Validate the repo (exits non-zero on problems)
  relink        Rewrite cross-ADR links to current locations
  renumber      Renumber an ADR to resolve a number collision
  migrate       Convert the repo to the configured layout/format

Publishing:
  index         Regenerate the ADR section of SUMMARY.md
  publish       Export the accepted ADR set to a directory

Configuration:
  config        Inspect or change configuration
  completions   Print a shell completion script (bash/zsh/fish/…)
  help          Print help for a command

Options:
{options}{after-help}")
)]
pub struct Cli {
    // --- Repo selection (global: inherited by every subcommand) -------------
    /// ADR directory (overrides config; default `~/.local/share/adroit/`).
    ///
    /// Also settable via the `ADROIT_DIR` environment variable (e.g. from a
    /// `.env` file), so you don't have to pass `--dir` on every command.
    #[arg(
        short,
        long,
        global = true,
        env = "ADROIT_DIR",
        help_heading = "Repo selection"
    )]
    pub dir: Option<PathBuf>,

    /// On-disk format: `markdown` or `frontmatter` (overrides config).
    ///
    /// Also settable via `ADROIT_FORMAT`.
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_FORMAT",
        help_heading = "Repo selection"
    )]
    pub format: Option<Format>,

    /// Directory layout: `by_status`, `by_category`, or `flat` (overrides config).
    ///
    /// Also settable via `ADROIT_LAYOUT`.
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_LAYOUT",
        help_heading = "Repo selection"
    )]
    pub layout: Option<Layout>,

    /// How ADR identifiers/filenames are formed (overrides config).
    ///
    /// `sequential` (NNNN, default), `date` (YYYYMMDD-title), `uuid`, or
    /// `per_category`. Also settable via `ADROIT_NAMING`.
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_NAMING",
        help_heading = "Repo selection"
    )]
    pub naming: Option<NamingScheme>,

    /// Where ADR dates/lifecycle come from (overrides config).
    ///
    /// `auto` (git when available, else filesystem), `git` (require git; warn if
    /// unavailable/shallow), or `filesystem` (never shell git). Also settable via
    /// `ADROIT_DATE_SOURCE`.
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_DATE_SOURCE",
        help_heading = "Repo selection"
    )]
    pub date_source: Option<DateSource>,

    /// How much a status-change move auto-relinks (overrides config).
    ///
    /// `all` (heal every inbound link, default), `self` (only the moved file's
    /// own links — defer the rest to a post-merge `adroit relink`), or `none`
    /// (move only). Also settable via `ADROIT_RELINK_SCOPE`.
    #[arg(
        long,
        value_enum,
        global = true,
        env = "ADROIT_RELINK_SCOPE",
        help_heading = "Repo selection"
    )]
    pub relink_scope: Option<RelinkScope>,

    // --- Command-specific defaults (top-level only; env binds everywhere) ----
    // These are NOT `global` — the env var still binds (clap reads env
    // regardless), but the flag stays off every subcommand's `--help` since only
    // a few commands use each. Set them in config / `.env`, or before the
    // subcommand (e.g. `adroit --theme gruvbox`).
    /// TUI markdown-preview color theme: `default` or `gruvbox` (overrides config).
    ///
    /// Only the TUI (bare `adroit`) and `serve` consult it. Also settable via
    /// `ADROIT_THEME`.
    #[arg(long, value_enum, env = "ADROIT_THEME")]
    pub theme: Option<MarkdownTheme>,

    /// Default template for `new`: a built-in (`madr`, `nygard`) or a path.
    ///
    /// Overrides config; `new --template` still wins per-invocation. Also
    /// settable via `ADROIT_TEMPLATE`.
    #[arg(long, env = "ADROIT_TEMPLATE")]
    pub default_template: Option<String>,

    /// Days after which a still-Proposed ADR with no `review_by` is flagged
    /// review-due — `0` disables (overrides config).
    ///
    /// Used by `list` / `stats` / `check` and the dashboard. Also settable via
    /// `ADROIT_REVIEW_OVERDUE_DAYS`.
    #[arg(long, env = "ADROIT_REVIEW_OVERDUE_DAYS")]
    pub review_overdue_days: Option<u32>,

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
    /// Save a forge token to the local credential store (used after env vars).
    ///
    /// A dependency-free `0600` file next to the config — no env-var copy-paste.
    /// (OAuth device-flow + OS keychain are future enhancements.)
    #[cfg(feature = "forge")]
    Auth {
        /// Which token to store: `github`, `gitlab`, or `jira`.
        provider: String,
        /// The token value (omit to be prompted, hidden).
        #[arg(long)]
        token: Option<String>,
        /// For `jira`: the account email saved alongside the token.
        #[arg(long)]
        email: Option<String>,
    },
    /// Set up forge integration by detecting the provider from the git remote.
    ///
    /// Writes `forge.provider` / `forge.repo` (+ `forge.host` for self-managed)
    /// to your config and reminds you which token env var to set. `--print` only
    /// shows what it detected.
    #[cfg(feature = "forge")]
    Init {
        /// Show the detected settings without writing them to config.
        #[arg(long)]
        print: bool,
    },
    /// Export the accepted ADR set to a directory (static-dir publisher).
    ///
    /// Sibling to `index`, but for the published-docs side. Copies every
    /// accepted ADR plus an `index.md` to `--out`. Idempotent.
    Publish {
        /// Output directory for the published ADRs.
        #[arg(short, long)]
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
        #[arg(short, long)]
        output: Option<PathBuf>,
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
