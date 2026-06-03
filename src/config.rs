use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::adr::Status;
use crate::format::Format;
use crate::naming::NamingScheme;

/// On-disk directory layout for a store.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum Layout {
    /// ADRs grouped into per-status subdirectories (status by directory). Default.
    #[default]
    ByStatus,
    /// ADRs grouped into per-**category** subdirectories (the directory is the
    /// area, not the status). Status lives in the `## Status` section / banner,
    /// and numbering is per-category (pairs with the `per_category` naming
    /// scheme). Used for MADR-style category folders.
    ByCategory,
    /// All ADRs in one flat directory (the original adroit layout).
    Flat,
}

/// Color theme for the TUI markdown preview.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum MarkdownTheme {
    /// 16-color ANSI palette — respects the user's terminal colors. Default.
    #[default]
    Default,
    /// Gruvbox (true-color), matching the house mdBook/doxygen theme.
    Gruvbox,
}

/// Where adroit reads ADR creation / lifecycle dates from.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum DateSource {
    /// Git history when the ADR dir is a git work tree, else the filesystem.
    /// Adaptive and silent — the default.
    #[default]
    Auto,
    /// Require git history: warn once (then fall back) when it's unavailable or
    /// the clone is shallow, so a CI misconfiguration is loud, not silent.
    Git,
    /// Filesystem only — never shell `git` (mtime / authored dates, no
    /// reconstructed lifecycle timeline). Fast and dependency-free.
    Filesystem,
}

/// How much a status-change *move* auto-relinks cross-ADR links.
///
/// In `by_status`, a status change moves the ADR between directories, which
/// strands relative links to/from it. The default heals every link immediately.
/// Concurrent-PR teams instead defer the repo-wide heal to a single
/// `adroit relink` on `main` (so a status-change PR touches only its own ADR and
/// two decision PRs never collide on shared neighbors) — see the "heal-on-main"
/// workflow in the docs. The explicit `adroit relink` command is always
/// full-scope regardless of this setting.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum RelinkScope {
    /// Heal every inbound link on a move (today's behavior). Best for a single
    /// author / no concurrent PRs. Default.
    #[default]
    All,
    /// Fix only the *moved* file's own outbound links — leave neighbors for a
    /// post-merge `adroit relink`. The moved ADR stays internally valid, and a
    /// status-change PR touches only that one file. Recommended for branching
    /// teams. Serialized as `self`.
    #[strum(serialize = "self")]
    #[serde(rename = "self")]
    #[value(name = "self")]
    SelfOnly,
    /// Move only — defer all link fixing to a post-merge `adroit relink`.
    None,
}

/// The code-review host (and, by default, its native issue tracker) that the
/// opt-in forge integration drives. `none` disables it.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Forge integration off (the default).
    #[default]
    None,
    /// GitHub (Pull Requests + GitHub Issues).
    Github,
    /// GitLab (Merge Requests + GitLab Issues).
    Gitlab,
}

/// Which issue tracker the forge integration files to. `native` = the forge's
/// own issues (GitHub/GitLab Issues); the others split the tracker off the forge
/// (e.g. GitLab MRs + Jira issues).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
pub enum TrackerProvider {
    /// The forge's own issue tracker (default).
    #[default]
    Native,
    Jira,
    Linear,
    GhIssues,
    GlIssues,
}

/// Opt-in forge/tracker integration config. Tokens are **never** stored here —
/// they come from the environment (`ADROIT_GITHUB_TOKEN` / `ADROIT_GITLAB_TOKEN`
/// / `ADROIT_JIRA_TOKEN`), read at construction time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ForgeConfig {
    /// Code-review host (`none` disables the integration).
    pub provider: Provider,
    /// Provider slug — GitHub `owner/repo`, GitLab `group/project` (or numeric
    /// project id). Defaults to the git remote when unset (future `init`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// API host for self-managed / enterprise instances (defaults per provider:
    /// `api.github.com` / `gitlab.com`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Branch name prefix for `new`'s generated branch (default `adr/`).
    pub branch_prefix: String,
    /// Base branch PRs/MRs target (default `main`).
    pub base_branch: String,
    /// Issue tracker (default `native` = the forge's own issues).
    pub tracker: TrackerProvider,
    /// Project key/id for a **split** tracker (e.g. the Jira project `OPS`).
    /// Unused when `tracker = native`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker_project: Option<String>,
    /// API host for a split tracker (e.g. `your-site.atlassian.net`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker_host: Option<String>,
    /// API token — env-only, never persisted (`#[serde(skip)]`). Populated at
    /// construction from `ADROIT_*_TOKEN`.
    #[serde(skip)]
    pub token: Option<String>,
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            repo: None,
            host: None,
            branch_prefix: "adr/".to_string(),
            base_branch: "main".to_string(),
            tracker: TrackerProvider::default(),
            tracker_project: None,
            tracker_host: None,
            token: None,
        }
    }
}

/// Application configuration, persisted as YAML.
///
/// New keys all carry serde defaults so older config files keep loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// ADR directory path. Supports `~` and `$ENV_VAR` expansion.
    /// Relative paths resolve from the XDG data directory
    /// (typically `~/.local/share/adroit/`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,

    /// Preferred editor command (e.g. `"vim"`, `"code --wait"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,

    /// On-disk serialization profile (default: markdown).
    pub format: Format,

    /// On-disk directory layout (default: by_status).
    pub layout: Layout,

    /// Map from status to the directory name used in `by_status` layout.
    /// Defaults to lowercase status names.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub status_dirs: BTreeMap<Status, String>,

    /// Template used when scaffolding new ADRs (default: `madr`).
    pub default_template: String,

    /// Directory of custom named templates (`<name>.md`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templates_dir: Option<PathBuf>,

    /// Status assigned to newly created ADRs (default: Proposed).
    pub default_status: Status,

    /// Open `$EDITOR` automatically after `new` (default: true).
    pub open_on_new: bool,

    /// Path to a SUMMARY.md to regenerate on `index` (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_path: Option<PathBuf>,

    /// Default review period length in business days for `review` (default: 3).
    pub review_days: u32,

    /// Default quorum (approvals required) for `review` (default: 3).
    pub review_quorum: u32,

    /// A still-`Proposed` ADR older than this many days (by its creation date)
    /// is flagged **review-due** even without an explicit `review_by` deadline,
    /// so an aging backlog surfaces on its own. `0` disables age-based flagging
    /// (deadline-only). Default: 30.
    pub review_overdue_days: u32,

    /// Color theme for the TUI markdown preview (default: `default`/ANSI).
    pub tui_theme: MarkdownTheme,

    /// Where ADR creation / lifecycle dates come from: `auto` (git when
    /// available, else filesystem), `git` (require git; warn if unavailable or
    /// shallow), or `filesystem` (never shell git). Default: `auto`.
    pub date_source: DateSource,

    /// How ADR identifiers / filenames are formed: `sequential` (NNNN, default),
    /// `date` (YYYYMMDD-title), `uuid`, or `per_category` (per-directory NNNN).
    pub naming: NamingScheme,

    /// How much a status-change move auto-relinks: `all` (heal every inbound
    /// link, default), `self` (only the moved file's own links — defer the rest
    /// to a post-merge `adroit relink`), or `none` (move only). Default: `all`.
    pub relink_scope: RelinkScope,

    /// Opt-in forge/tracker integration (issue + PR/MR creation, etc.).
    /// Absent by default — bare `adroit` never touches a forge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge: Option<ForgeConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dir: None,
            editor: None,
            format: Format::default(),
            layout: Layout::default(),
            status_dirs: BTreeMap::new(),
            default_template: "madr".to_string(),
            default_status: Status::default(),
            open_on_new: true,
            templates_dir: None,
            summary_path: None,
            review_days: 3,
            review_quorum: 3,
            review_overdue_days: 30,
            tui_theme: MarkdownTheme::default(),
            date_source: DateSource::default(),
            naming: NamingScheme::default(),
            relink_scope: RelinkScope::default(),
            forge: None,
        }
    }
}

impl Config {
    /// Return the directory name for a status, honoring `status_dirs`
    /// overrides and falling back to the lowercase status name.
    pub fn dir_for(&self, status: Status) -> String {
        self.status_dirs
            .get(&status)
            .cloned()
            .unwrap_or_else(|| status.to_string().to_lowercase())
    }

    /// Current value of a scalar config `key` as a string (for `adroit config`),
    /// or `None` for an unset optional (`dir`/`editor`) or an unknown key.
    pub fn get_str(&self, key: &str) -> Option<String> {
        Some(match key {
            "dir" => self.dir.as_ref()?.to_string_lossy().into_owned(),
            "editor" => self.editor.clone()?,
            "format" => self.format.to_string(),
            "layout" => self.layout.to_string(),
            "default_template" => self.default_template.clone(),
            "default_status" => self.default_status.to_string(),
            "open_on_new" => self.open_on_new.to_string(),
            "review_days" => self.review_days.to_string(),
            "review_quorum" => self.review_quorum.to_string(),
            "review_overdue_days" => self.review_overdue_days.to_string(),
            "tui_theme" => self.tui_theme.to_string(),
            "date_source" => self.date_source.to_string(),
            "naming" => self.naming.to_string(),
            "relink_scope" => self.relink_scope.to_string(),
            // Forge sub-keys read through the (optional) `forge` block, falling
            // back to ForgeConfig defaults when it's unset. `repo`/`host` are
            // genuinely optional → `None` when unset.
            "forge.provider" => self.forge.as_ref().map_or_else(
                || Provider::default().to_string(),
                |f| f.provider.to_string(),
            ),
            "forge.repo" => self.forge.as_ref().and_then(|f| f.repo.clone())?,
            "forge.host" => self.forge.as_ref().and_then(|f| f.host.clone())?,
            "forge.branch_prefix" => self.forge.as_ref().map_or_else(
                || ForgeConfig::default().branch_prefix,
                |f| f.branch_prefix.clone(),
            ),
            "forge.base_branch" => self.forge.as_ref().map_or_else(
                || ForgeConfig::default().base_branch,
                |f| f.base_branch.clone(),
            ),
            "forge.tracker" => self.forge.as_ref().map_or_else(
                || TrackerProvider::default().to_string(),
                |f| f.tracker.to_string(),
            ),
            "forge.tracker_project" => self
                .forge
                .as_ref()
                .and_then(|f| f.tracker_project.clone())?,
            "forge.tracker_host" => self.forge.as_ref().and_then(|f| f.tracker_host.clone())?,
            _ => return None,
        })
    }

    /// Set a scalar config `key` from a string, validating the value. Returns a
    /// human error for an unknown key or an invalid value.
    pub fn set_str(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bad = |what: &str| format!("invalid {what} `{value}`");
        match key {
            "dir" => self.dir = Some(PathBuf::from(value)),
            "editor" => self.editor = Some(value.to_string()),
            "format" => {
                self.format = value
                    .parse()
                    .map_err(|_| bad("format (markdown|frontmatter)"))?
            }
            "layout" => {
                self.layout = value
                    .parse()
                    .map_err(|_| bad("layout (by_status|by_category|flat)"))?
            }
            "default_template" => self.default_template = value.to_string(),
            "default_status" => self.default_status = value.parse().map_err(|_| bad("status"))?,
            "open_on_new" => self.open_on_new = value.parse().map_err(|_| bad("boolean"))?,
            "review_days" => self.review_days = value.parse().map_err(|_| bad("number"))?,
            "review_quorum" => self.review_quorum = value.parse().map_err(|_| bad("number"))?,
            "review_overdue_days" => {
                self.review_overdue_days = value.parse().map_err(|_| bad("number"))?
            }
            "tui_theme" => {
                self.tui_theme = value.parse().map_err(|_| bad("theme (default|gruvbox)"))?
            }
            "date_source" => {
                self.date_source = value
                    .parse()
                    .map_err(|_| bad("date source (auto|git|filesystem)"))?
            }
            "naming" => {
                self.naming = value
                    .parse()
                    .map_err(|_| bad("naming scheme (sequential|date|uuid|per_category)"))?
            }
            "relink_scope" => {
                self.relink_scope = value
                    .parse()
                    .map_err(|_| bad("relink scope (all|self|none)"))?
            }
            // Forge sub-keys lazily create the `forge` block, then set one field.
            "forge.provider" => {
                self.forge.get_or_insert_with(ForgeConfig::default).provider = value
                    .parse()
                    .map_err(|_| bad("forge provider (none|github|gitlab)"))?
            }
            "forge.repo" => {
                self.forge.get_or_insert_with(ForgeConfig::default).repo = Some(value.to_string())
            }
            "forge.host" => {
                self.forge.get_or_insert_with(ForgeConfig::default).host = Some(value.to_string())
            }
            "forge.branch_prefix" => {
                self.forge
                    .get_or_insert_with(ForgeConfig::default)
                    .branch_prefix = value.to_string()
            }
            "forge.base_branch" => {
                self.forge
                    .get_or_insert_with(ForgeConfig::default)
                    .base_branch = value.to_string()
            }
            "forge.tracker" => {
                self.forge.get_or_insert_with(ForgeConfig::default).tracker = value
                    .parse()
                    .map_err(|_| bad("tracker (native|jira|linear|gh_issues|gl_issues)"))?
            }
            "forge.tracker_project" => {
                self.forge
                    .get_or_insert_with(ForgeConfig::default)
                    .tracker_project = Some(value.to_string())
            }
            "forge.tracker_host" => {
                self.forge
                    .get_or_insert_with(ForgeConfig::default)
                    .tracker_host = Some(value.to_string())
            }
            _ => return Err(format!("unknown config key `{key}`")),
        }
        Ok(())
    }
}

/// The scalar config keys `adroit config` shows / gets / sets, in display order.
pub const CONFIG_KEYS: &[&str] = &[
    "dir",
    "editor",
    "format",
    "layout",
    "default_template",
    "default_status",
    "open_on_new",
    "review_days",
    "review_quorum",
    "review_overdue_days",
    "tui_theme",
    "date_source",
    "naming",
    "relink_scope",
    "forge.provider",
    "forge.repo",
    "forge.host",
    "forge.branch_prefix",
    "forge.base_branch",
    "forge.tracker",
    "forge.tracker_project",
    "forge.tracker_host",
];

/// The environment variable that overrides a config key (for `.env` writes and
/// source reporting), or `None` for keys with no env override.
pub fn env_var_for(key: &str) -> Option<&'static str> {
    Some(match key {
        "dir" => "ADROIT_DIR",
        "format" => "ADROIT_FORMAT",
        "layout" => "ADROIT_LAYOUT",
        "tui_theme" => "ADROIT_THEME",
        "default_template" => "ADROIT_TEMPLATE",
        "review_overdue_days" => "ADROIT_REVIEW_OVERDUE_DAYS",
        "date_source" => "ADROIT_DATE_SOURCE",
        "naming" => "ADROIT_NAMING",
        "relink_scope" => "ADROIT_RELINK_SCOPE",
        _ => return None,
    })
}

/// Best-effort parse of a git remote URL into `(provider, "owner/repo", host)`
/// for `adroit init`. Handles `git@host:path.git`, `https://host/path.git`, and
/// `ssh://git@host/path`. `host` is `Some` only for a non-default host
/// (self-managed GitLab); `None` for github.com / gitlab.com. Returns `None`
/// when the host isn't a recognizable GitHub/GitLab.
pub fn parse_remote_url(url: &str) -> Option<(Provider, String, Option<String>)> {
    let u = url.trim();
    let (host, path) = if let Some(rest) = u.strip_prefix("git@") {
        // scp-style: git@host:owner/repo.git
        let (h, p) = rest.split_once(':')?;
        (h.to_string(), p.to_string())
    } else {
        let rest = u.split_once("://").map(|(_, r)| r).unwrap_or(u);
        let rest = rest.rsplit('@').next().unwrap_or(rest); // drop optional user@
        let (h, p) = rest.split_once('/')?;
        (h.to_string(), p.to_string())
    };
    let repo = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if repo.is_empty() {
        return None;
    }
    let (provider, host_opt) = match host.as_str() {
        "github.com" => (Provider::Github, None),
        "gitlab.com" => (Provider::Gitlab, None),
        h if h.contains("gitlab") => (Provider::Gitlab, Some(host)),
        _ => return None,
    };
    Some((provider, repo, host_opt))
}

/// Upsert `KEY=value` into a `.env`-style file: replace the first active (un-
/// commented) `KEY=` line, else append. Other lines (incl. comments) are kept.
/// Creates the file if missing.
pub fn upsert_env_file(path: &std::path::Path, key: &str, value: &str) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let prefix = format!("{key}=");
    let new_line = format!("{key}={value}");
    if let Some(slot) = lines
        .iter_mut()
        .find(|l| l.trim_start().starts_with(&prefix))
    {
        *slot = new_line;
    } else {
        lines.push(new_line);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(path, out)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine config directory (HOME not set?)")]
    NoConfigDir,

    #[error("failed to write config file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },

    #[error("no editor found — install an editor or set the EDITOR environment variable")]
    NoEditor,
}

/// Return the path to the config file, or `None` if the home directory
/// cannot be determined.
/// Is `key` present in raw config YAML? Walks a dotted key (`forge.provider`)
/// into the nested map it serializes to, so `config show` can tell a file-set
/// forge key from a defaulted one (a flat `raw.get("forge.provider")` always
/// misses the nested `forge:` block and would mislabel it `default`).
pub fn yaml_has_key(raw: &serde_yaml_ng::Value, key: &str) -> bool {
    let mut node = raw;
    for part in key.split('.') {
        match node.get(part) {
            Some(v) => node = v,
            None => return false,
        }
    }
    true
}

pub fn config_path() -> Option<PathBuf> {
    Some(
        ProjectDirs::from("", "", "adroit")?
            .config_dir()
            .join("config.yaml"),
    )
}

/// Return `true` if the config file already exists on disk.
pub fn config_file_exists() -> bool {
    config_path().is_some_and(|p| p.exists())
}

/// The forge-token store (`adroit auth`) — a `provider: token` YAML map next to
/// the config, created `0600`. A dependency-free, persistent, CI-safe credential
/// store (an OS-keychain backend could be added later behind its own feature).
pub fn credentials_path() -> Option<PathBuf> {
    Some(
        ProjectDirs::from("", "", "adroit")?
            .config_dir()
            .join("credentials.yaml"),
    )
}

/// Load the saved token for `key` (e.g. `"github"`), if any.
pub fn load_credential(key: &str) -> Option<String> {
    let map: BTreeMap<String, String> =
        serde_yaml_ng::from_str(&std::fs::read_to_string(credentials_path()?).ok()?).ok()?;
    map.get(key).cloned()
}

/// Save `token` for `key`, preserving other entries; the file is created `0600`.
pub fn store_credential(key: &str, token: &str) -> Result<(), ConfigError> {
    let path = credentials_path().ok_or(ConfigError::NoConfigDir)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: path.clone(),
            source,
        })?;
    }
    let mut map: BTreeMap<String, String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_yaml_ng::from_str(&c).ok())
        .unwrap_or_default();
    map.insert(key.to_string(), token.to_string());
    let yaml = serde_yaml_ng::to_string(&map).expect("serialize credentials");
    std::fs::write(&path, yaml).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

impl Config {
    /// Load config from the XDG config file.
    ///
    /// Returns `Config::default()` if the file doesn't exist or the
    /// home directory can't be determined. Errors only on malformed YAML.
    pub fn load() -> Result<Self, ConfigError> {
        let Some(path) = config_path() else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_yaml_ng::from_str(&contents)
                .map_err(|source| ConfigError::Parse { path, source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Ok(Self::default()),
        }
    }

    /// Save config to the XDG config file, creating parent dirs as needed.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_path().ok_or(ConfigError::NoConfigDir)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: path.clone(),
                source,
            })?;
        }
        let yaml = serde_yaml_ng::to_string(self).expect("Config serialization should never fail");
        std::fs::write(&path, yaml).map_err(|source| ConfigError::Write { path, source })
    }
}

/// Bootstrap the config file on first run.
///
/// Detects the system editor, writes it to `config.yaml`, and prints the
/// path so the user knows the file exists. Does nothing if the config
/// file already exists.
pub fn bootstrap(config: &mut Config) {
    if config_file_exists() {
        return;
    }
    if config.editor.is_none() {
        config.editor = detect_editor();
    }
    if config.save().is_ok()
        && let Some(path) = config_path()
    {
        eprintln!("Created {}", path.display());
    }
}

/// Resolve the user's preferred editor.
///
/// Precedence: `$VISUAL` / `$EDITOR` > `config.yaml` > auto-detect > interactive prompt.
///
/// Returns `Ok(Some(cmd))` when the editor is an explicit command string
/// that the caller should split and spawn. Returns `Ok(None)` when the
/// `edit` crate auto-detected an editor and the caller should use
/// [`edit::edit_file`] instead (it handles platform-specific flags).
pub fn resolve_editor(config: &mut Config) -> Result<Option<String>, ConfigError> {
    // 1. Environment variables (session override, standard Unix convention).
    let env_editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    if let Some(editor) = env_editor {
        return Ok(Some(editor));
    }

    // 2. User's persisted choice in config.yaml.
    if let Some(ref editor) = config.editor {
        return Ok(Some(editor.clone()));
    }

    // 3. Auto-detect via the edit crate (PATH probing).
    if edit::get_editor().is_ok() {
        return Ok(None);
    }

    // 4. Interactive prompt when running in a terminal.
    if std::io::stdin().is_terminal() {
        let editor = prompt_for_editor()?;
        config.editor = Some(editor.clone());
        let _ = config.save();
        return Ok(Some(editor));
    }

    Err(ConfigError::NoEditor)
}

/// Known editors with their display names and invocation commands.
/// Commands that need flags for "wait" behaviour include them here.
const EDITOR_CANDIDATES: &[(&str, &str)] = &[
    ("nano", "nano"),
    ("vim", "vim"),
    ("Neovim", "nvim"),
    ("vi", "vi"),
    ("Emacs", "emacs"),
    ("Helix", "hx"),
    ("micro", "micro"),
    ("VS Code", "code --wait"),
    ("Sublime Text", "subl --wait"),
    ("Kakoune", "kak"),
];

/// Detect the system editor via the `edit` crate and map it to a full
/// command (e.g. `code` → `code --wait`) using our candidate list.
pub fn detect_editor() -> Option<String> {
    let path = edit::get_editor().ok()?;
    let bin = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if bin.is_empty() {
        return None;
    }
    Some(
        EDITOR_CANDIDATES
            .iter()
            .find(|(_, cmd)| cmd.split_whitespace().next() == Some(bin))
            .map(|(_, cmd)| cmd.to_string())
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
    )
}

/// Probe PATH for installed editors and let the user pick one.
fn prompt_for_editor() -> Result<String, ConfigError> {
    let available: Vec<(&str, &str)> = EDITOR_CANDIDATES
        .iter()
        .filter(|(_, cmd)| {
            let bin = cmd.split_whitespace().next().unwrap();
            which::which(bin).is_ok()
        })
        .copied()
        .collect();

    if available.is_empty() {
        return dialoguer::Input::new()
            .with_prompt("No editor detected. Enter your editor command")
            .interact_text()
            .map_err(|_| ConfigError::NoEditor);
    }

    let mut labels: Vec<String> = available
        .iter()
        .map(|(name, cmd)| format!("{name} ({cmd})"))
        .collect();
    labels.push("Other (enter custom command)".to_string());

    let selection = dialoguer::Select::new()
        .with_prompt("Choose your preferred editor")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(|_| ConfigError::NoEditor)?;

    if selection < available.len() {
        Ok(available[selection].1.to_string())
    } else {
        dialoguer::Input::new()
            .with_prompt("Enter your editor command")
            .interact_text()
            .map_err(|_| ConfigError::NoEditor)
    }
}

/// Return the default ADR data directory (`$XDG_DATA_HOME/adroit/`).
///
/// On Linux this is typically `~/.local/share/adroit/`.
pub fn default_dir() -> PathBuf {
    ProjectDirs::from("", "", "adroit")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".adroit"))
}

/// Expand a leading `~` and any `$VAR` in a path, falling back to the original
/// on error. A no-op for a path with neither (e.g. one the shell already
/// expanded, or a plain relative path).
fn expand_path(raw: &std::path::Path) -> PathBuf {
    shellexpand::full(&raw.to_string_lossy())
        .map(|s| PathBuf::from(s.as_ref()))
        .unwrap_or_else(|_| raw.to_path_buf())
}

/// Resolve the ADR directory from the precedence chain:
/// CLI flag > config file > XDG data directory.
///
/// - **CLI / env paths** are tilde / env-var expanded, but relative paths stay
///   CWD-relative. A `--dir` typed at the shell is already expanded, but an
///   `ADROIT_DIR=~/foo` sourced from a `.env` reaches clap *literally* — without
///   expanding it here, the `~` becomes a stray directory name.
/// - **Config paths** undergo the same expansion, then absolute paths are used
///   directly while relative paths resolve against [`default_dir`].
pub fn resolve_dir(cli_dir: Option<PathBuf>, config: &Config) -> PathBuf {
    if let Some(dir) = cli_dir {
        return expand_path(&dir);
    }
    if let Some(ref raw) = config.dir {
        let expanded = expand_path(raw);
        if expanded.is_absolute() {
            return expanded;
        }
        return default_dir().join(expanded);
    }
    default_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_dir() {
        let config = Config::default();
        assert!(config.dir.is_none());
        assert!(config.editor.is_none());
    }

    #[test]
    fn resolve_cli_takes_precedence() {
        let config = Config {
            dir: Some(PathBuf::from("from-config")),
            ..Config::default()
        };
        let result = resolve_dir(Some(PathBuf::from("from-cli")), &config);
        assert_eq!(result, PathBuf::from("from-cli"));
    }

    #[test]
    fn resolve_config_relative_joins_data_dir() {
        let config = Config {
            dir: Some(PathBuf::from("my-project")),
            ..Config::default()
        };
        let result = resolve_dir(None, &config);
        assert_eq!(result, default_dir().join("my-project"));
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_config_nested_relative() {
        let config = Config {
            dir: Some(PathBuf::from("org/team/decisions")),
            ..Config::default()
        };
        let result = resolve_dir(None, &config);
        assert_eq!(result, default_dir().join("org/team/decisions"));
    }

    #[test]
    fn resolve_config_absolute_passes_through() {
        let config = Config {
            dir: Some(PathBuf::from("/opt/adrs")),
            ..Config::default()
        };
        let result = resolve_dir(None, &config);
        assert_eq!(result, PathBuf::from("/opt/adrs"));
    }

    #[test]
    fn resolve_config_tilde_expands() {
        let config = Config {
            dir: Some(PathBuf::from("~/my-adrs")),
            ..Config::default()
        };
        let result = resolve_dir(None, &config);
        assert!(result.is_absolute());
        assert!(result.ends_with("my-adrs"));
        assert!(!result.to_string_lossy().contains('~'));
    }

    #[test]
    fn resolve_cli_relative_stays_relative() {
        let config = Config::default();
        let result = resolve_dir(Some(PathBuf::from("local-adrs")), &config);
        assert_eq!(result, PathBuf::from("local-adrs"));
    }

    #[test]
    fn resolve_cli_tilde_expands() {
        // `ADROIT_DIR=~/foo` from a `.env` reaches clap literally (the shell
        // never sees it), so resolve_dir must expand the `~` itself — otherwise
        // it becomes a stray `~` directory.
        let config = Config::default();
        let result = resolve_dir(Some(PathBuf::from("~/my-adrs")), &config);
        assert!(result.is_absolute());
        assert!(result.ends_with("my-adrs"));
        assert!(!result.to_string_lossy().contains('~'));
    }

    #[test]
    fn resolve_falls_back_to_xdg_data_dir() {
        let config = Config::default();
        let result = resolve_dir(None, &config);
        assert_eq!(result, default_dir());
        assert!(result.is_absolute());
    }

    #[test]
    fn default_dir_ends_with_adroit() {
        let dir = default_dir();
        assert!(dir.ends_with("adroit"));
        assert!(dir.is_absolute());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let config: Config = serde_yaml_ng::from_str("{}").unwrap();
        assert!(config.dir.is_none());
        assert!(config.editor.is_none());
    }

    #[test]
    fn round_trip_serde() {
        let config = Config {
            dir: Some(PathBuf::from("my/adrs")),
            editor: Some("vim".to_string()),
            ..Config::default()
        };
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: Config = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed.dir, config.dir);
        assert_eq!(parsed.editor, config.editor);
        assert_eq!(parsed.format, config.format);
        assert_eq!(parsed.layout, config.layout);
    }

    #[test]
    fn defaults_are_markdown_by_status() {
        let config = Config::default();
        assert_eq!(config.format, Format::Markdown);
        assert_eq!(config.layout, Layout::ByStatus);
        assert_eq!(config.default_template, "madr");
        assert_eq!(config.default_status, Status::Proposed);
        assert!(config.open_on_new);
    }

    #[test]
    fn dir_for_status_defaults_to_lowercase() {
        let config = Config::default();
        assert_eq!(config.dir_for(Status::Proposed), "proposed");
        assert_eq!(config.dir_for(Status::Superseded), "superseded");
        assert_eq!(config.dir_for(Status::Rejected), "rejected");
    }

    #[test]
    fn missing_keys_use_defaults() {
        // A legacy config with only dir/editor must still load.
        let yaml = "dir: ~/old-adrs\neditor: nano\n";
        let config: Config = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.format, Format::Markdown);
        assert_eq!(config.layout, Layout::ByStatus);
        assert_eq!(config.default_template, "madr");
        // Review keys absent from a legacy config fall back to defaults.
        assert_eq!(config.review_days, 3);
        assert_eq!(config.review_quorum, 3);
    }

    #[test]
    fn review_defaults() {
        let config = Config::default();
        assert_eq!(config.review_days, 3);
        assert_eq!(config.review_quorum, 3);
        assert_eq!(config.review_overdue_days, 30);
    }

    #[test]
    fn date_source_defaults_to_auto() {
        assert_eq!(Config::default().date_source, DateSource::Auto);
        // Legacy configs without the key still load.
        let cfg: Config = serde_yaml_ng::from_str("dir: ~/x\n").unwrap();
        assert_eq!(cfg.date_source, DateSource::Auto);
    }

    #[test]
    fn config_path_returns_some() {
        if std::env::var("HOME").is_ok() {
            assert!(config_path().is_some());
        }
    }

    #[test]
    fn resolve_editor_uses_config() {
        let mut config = Config {
            editor: Some("nano".to_string()),
            ..Config::default()
        };
        let result = resolve_editor(&mut config).unwrap();
        // env vars may override config; if not set, config wins
        if std::env::var("VISUAL").is_err() && std::env::var("EDITOR").is_err() {
            assert_eq!(result, Some("nano".to_string()));
        }
    }

    #[test]
    fn detect_editor_returns_some_on_typical_system() {
        // Most systems have at least vi/nano on PATH or EDITOR set.
        if edit::get_editor().is_ok() {
            assert!(detect_editor().is_some());
        }
    }

    #[test]
    fn editor_field_omitted_when_none() {
        let config = Config::default();
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        assert!(!yaml.contains("editor"));
    }

    #[test]
    fn config_get_set_str_round_trip_and_validation() {
        let mut c = Config::default();
        assert_eq!(c.get_str("layout").as_deref(), Some("by_status"));
        assert_eq!(c.get_str("date_source").as_deref(), Some("auto"));
        // relink_scope defaults to `all` and round-trips `self`/`none`.
        assert_eq!(c.get_str("relink_scope").as_deref(), Some("all"));
        c.set_str("relink_scope", "self").unwrap();
        assert_eq!(c.get_str("relink_scope").as_deref(), Some("self"));
        c.set_str("relink_scope", "none").unwrap();
        assert_eq!(c.get_str("relink_scope").as_deref(), Some("none"));
        // Forge sub-keys: lazily create the block, round-trip, validate.
        assert_eq!(c.get_str("forge.provider").as_deref(), Some("none"));
        assert_eq!(c.get_str("forge.branch_prefix").as_deref(), Some("adr/"));
        assert_eq!(c.get_str("forge.repo"), None); // optional, unset
        c.set_str("forge.provider", "github").unwrap();
        c.set_str("forge.repo", "como-technologies/adroit").unwrap();
        c.set_str("forge.tracker", "jira").unwrap();
        assert_eq!(c.get_str("forge.provider").as_deref(), Some("github"));
        assert_eq!(
            c.get_str("forge.repo").as_deref(),
            Some("como-technologies/adroit")
        );
        assert_eq!(c.get_str("forge.tracker").as_deref(), Some("jira"));
        assert!(c.set_str("forge.provider", "bitbucket").is_err());
        // The token is never a config key (env-only).
        assert!(c.set_str("forge.token", "secret").is_err());
        c.set_str("layout", "flat").unwrap();
        c.set_str("review_overdue_days", "45").unwrap();
        assert_eq!(c.get_str("layout").as_deref(), Some("flat"));
        assert_eq!(c.get_str("review_overdue_days").as_deref(), Some("45"));
        // Validation + unknown keys.
        assert!(c.set_str("layout", "sideways").is_err());
        assert!(c.set_str("relink_scope", "partial").is_err());
        assert!(c.set_str("review_days", "lots").is_err());
        assert!(c.set_str("bogus", "x").is_err());
        assert_eq!(c.get_str("bogus"), None);
        // Unset optionals read as None.
        assert_eq!(c.get_str("editor"), None);
    }

    #[test]
    fn yaml_has_key_walks_dotted_keys() {
        let raw: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("layout: flat\nforge:\n  provider: github\n  repo: o/r\n")
                .unwrap();
        // Flat key present / absent.
        assert!(yaml_has_key(&raw, "layout"));
        assert!(!yaml_has_key(&raw, "format"));
        // Nested (dotted) keys: present ones resolve, unset ones don't — the bug
        // was a flat lookup labeling every `forge.*` key `default`.
        assert!(yaml_has_key(&raw, "forge.provider"));
        assert!(yaml_has_key(&raw, "forge.repo"));
        assert!(!yaml_has_key(&raw, "forge.host"));
        assert!(!yaml_has_key(&raw, "forge.tracker"));
    }

    #[test]
    fn parse_remote_url_handles_common_forms() {
        let gh = (Provider::Github, "owner/repo".to_string(), None);
        assert_eq!(
            parse_remote_url("git@github.com:owner/repo.git"),
            Some(gh.clone())
        );
        assert_eq!(
            parse_remote_url("https://github.com/owner/repo.git"),
            Some(gh.clone())
        );
        assert_eq!(parse_remote_url("https://github.com/owner/repo"), Some(gh));
        assert_eq!(
            parse_remote_url("git@gitlab.com:grp/sub/proj.git"),
            Some((Provider::Gitlab, "grp/sub/proj".to_string(), None))
        );
        assert_eq!(
            parse_remote_url("https://gitlab.example.com/grp/proj.git"),
            Some((
                Provider::Gitlab,
                "grp/proj".to_string(),
                Some("gitlab.example.com".to_string())
            ))
        );
        // Unknown host → None (user configures manually).
        assert_eq!(parse_remote_url("https://bitbucket.org/o/r.git"), None);
    }

    #[test]
    fn env_var_mapping() {
        assert_eq!(env_var_for("layout"), Some("ADROIT_LAYOUT"));
        assert_eq!(env_var_for("date_source"), Some("ADROIT_DATE_SOURCE"));
        assert_eq!(env_var_for("relink_scope"), Some("ADROIT_RELINK_SCOPE"));
        assert_eq!(
            env_var_for("review_overdue_days"),
            Some("ADROIT_REVIEW_OVERDUE_DAYS")
        );
        assert_eq!(env_var_for("editor"), None); // no env override
    }

    #[test]
    fn upsert_env_replaces_or_appends_preserving_other_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".env");
        std::fs::write(&p, "# keep me\nADROIT_DIR=/x\n").unwrap();
        upsert_env_file(&p, "ADROIT_LAYOUT", "flat").unwrap(); // append
        upsert_env_file(&p, "ADROIT_DIR", "/y").unwrap(); // replace
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("ADROIT_DIR=/y"));
        assert!(!out.contains("ADROIT_DIR=/x"));
        assert!(out.contains("ADROIT_LAYOUT=flat"));
        assert!(out.contains("# keep me"));
    }
}
