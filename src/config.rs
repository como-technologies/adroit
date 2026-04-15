use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Application configuration, persisted as YAML.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Default ADR directory path. Relative paths resolve from CWD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
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
pub fn resolve_dir(cli_dir: Option<PathBuf>, config: &Config) -> PathBuf {
    cli_dir
        .or_else(|| config.dir.clone())
        .unwrap_or_else(default_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_dir() {
        let config = Config::default();
        assert!(config.dir.is_none());
    }

    #[test]
    fn resolve_cli_takes_precedence() {
        let config = Config {
            dir: Some(PathBuf::from("from-config")),
        };
        let result = resolve_dir(Some(PathBuf::from("from-cli")), &config);
        assert_eq!(result, PathBuf::from("from-cli"));
    }

    #[test]
    fn resolve_config_over_default() {
        let config = Config {
            dir: Some(PathBuf::from("from-config")),
        };
        let result = resolve_dir(None, &config);
        assert_eq!(result, PathBuf::from("from-config"));
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
        // Config::load() reads from the real XDG path, but if the file
        // doesn't exist it returns default. We can't easily override the
        // path in a unit test, so we test the deserialize path directly.
        let config: Config = serde_yaml_ng::from_str("{}").unwrap();
        assert!(config.dir.is_none());
    }

    #[test]
    fn round_trip_serde() {
        let config = Config {
            dir: Some(PathBuf::from("my/adrs")),
        };
        let yaml = serde_yaml_ng::to_string(&config).unwrap();
        let parsed: Config = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed.dir, config.dir);
    }

    #[test]
    fn config_path_returns_some() {
        // Should work on any system where HOME is set
        if std::env::var("HOME").is_ok() {
            assert!(config_path().is_some());
        }
    }
}
