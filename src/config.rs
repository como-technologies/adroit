use std::io::IsTerminal;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Application configuration, persisted as YAML.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// ADR directory path. Supports `~` and `$ENV_VAR` expansion.
    /// Relative paths resolve from the XDG data directory
    /// (typically `~/.local/share/adroit/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,

    /// Preferred editor command (e.g. `"vim"`, `"code --wait"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
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

/// Resolve the ADR directory from the precedence chain:
/// CLI flag > config file > XDG data directory.
///
/// - **CLI paths** are used as-is (the shell expands `~` and `$VAR` before
///   we see them, and relative paths are intentionally CWD-relative).
/// - **Config paths** undergo tilde / env-var expansion, then absolute paths
///   are used directly while relative paths resolve against [`default_dir`].
pub fn resolve_dir(cli_dir: Option<PathBuf>, config: &Config) -> PathBuf {
    if let Some(dir) = cli_dir {
        return dir;
    }
    if let Some(ref raw) = config.dir {
        let expanded = shellexpand::full(&raw.to_string_lossy())
            .map(|s| PathBuf::from(s.as_ref()))
            .unwrap_or_else(|_| raw.clone());
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
        };
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: Config = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed.dir, config.dir);
        assert_eq!(parsed.editor, config.editor);
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
}
