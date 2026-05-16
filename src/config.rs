use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::presets::{BUILTIN_MODIFIERS, BUILTIN_PRESETS};

/// Errors from config file loading and validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("config file parse: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Loaded and resolved configuration.
///
/// All paths in this struct are absolute — resolved relative to the config
/// file's parent directory at load time.
#[derive(Debug, Clone)]
pub struct Config {
    #[allow(dead_code)]
    pub config_dir: Option<PathBuf>,
    pub default_base: Option<String>,
    pub default_modifiers: Vec<String>,
    pub bases: HashMap<String, PathBuf>,
    pub modifiers: HashMap<String, PathBuf>,
    pub axes: AxesConfig,
    pub presets: HashMap<String, CustomPreset>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_dir: None,
            default_base: None,
            default_modifiers: Vec::new(),
            bases: HashMap::new(),
            modifiers: HashMap::new(),
            axes: AxesConfig::default(),
            presets: HashMap::new(),
        }
    }
}

impl Config {
    /// Search for and load the config file.
    ///
    /// 1. `$CWD/.deepseek-tui-modes.json` (project-local, wins entirely)
    /// 2. `~/.config/deepseek-tui-modes/config.json` (global fallback)
    /// 3. Neither — returns default empty `Config`
    pub fn load() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        let candidates = [
            cwd.join(".deepseek-tui-modes.json"),
            dirs_config_dir().join("config.json"),
        ];

        for path in &candidates {
            if path.exists() {
                match Self::load_from(path) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("Warning: failed to load config at {path:?}: {e}");
                        return Self::default();
                    }
                }
            }
        }

        Self::default()
    }

    /// Load and parse config from a specific path.
    fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let raw_text = fs::read_to_string(path)?;
        let raw: RawConfig = serde_json::from_str(&raw_text)?;

        let config_dir = path.parent().map(|p| p.to_path_buf());
        let resolve_path =
            |s: &str| -> PathBuf { resolve_relative_path(s, config_dir.as_deref()) };

        let bases: HashMap<String, PathBuf> = raw
            .bases
            .into_iter()
            .map(|(k, v)| (k, resolve_path(&v)))
            .collect();
        let modifiers: HashMap<String, PathBuf> = raw
            .modifiers
            .into_iter()
            .map(|(k, v)| (k, resolve_path(&v)))
            .collect();
        let axes = AxesConfig {
            agency: raw
                .axes
                .agency
                .into_iter()
                .map(|(k, v)| (k, resolve_path(&v)))
                .collect(),
            quality: raw
                .axes
                .quality
                .into_iter()
                .map(|(k, v)| (k, resolve_path(&v)))
                .collect(),
            scope: raw
                .axes
                .scope
                .into_iter()
                .map(|(k, v)| (k, resolve_path(&v)))
                .collect(),
        };

        let config = Self {
            config_dir,
            default_base: raw.default_base,
            default_modifiers: raw.default_modifiers,
            bases,
            modifiers,
            axes,
            presets: raw.presets,
        };

        config.validate();
        Ok(config)
    }

    /// Validate config: check collisions, warn about missing paths.
    fn validate(&self) {
        for name in self.presets.keys() {
            if BUILTIN_PRESETS.contains(&name.as_str()) {
                eprintln!(
                    "Warning: custom preset '{name}' collides with built-in preset name — ignoring"
                );
            }
        }

        for name in self.modifiers.keys() {
            if BUILTIN_MODIFIERS.contains(&name.as_str()) {
                eprintln!(
                    "Warning: custom modifier '{name}' collides with built-in modifier name — ignoring"
                );
            }
        }

        warn_missing("base", self.bases.values());
        warn_missing("modifier", self.modifiers.values());
        warn_missing("axis", self.axes.agency.values());
        warn_missing("axis", self.axes.quality.values());
        warn_missing("axis", self.axes.scope.values());
    }
}

fn warn_missing<'a>(kind: &str, paths: impl Iterator<Item = &'a PathBuf>) {
    for path in paths {
        if !path.exists() {
            eprintln!("Warning: {kind} path does not exist: {path:?}");
        }
    }
}

/// Resolve a path from config relative to the config file directory.
fn resolve_relative_path(path: &str, config_dir: Option<&Path>) -> PathBuf {
    let pb = PathBuf::from(path);
    if pb.is_relative() {
        if let Some(dir) = config_dir {
            dir.join(pb)
        } else {
            pb
        }
    } else {
        pb
    }
}

/// Get the user config directory (~/.config/deepseek-tui-modes/).
fn dirs_config_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/deepseek-tui-modes")
}

// ---------------------------------------------------------------------------
// Raw (deserialization) types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawConfig {
    default_base: Option<String>,
    default_modifiers: Vec<String>,
    bases: HashMap<String, String>,
    modifiers: HashMap<String, String>,
    axes: RawAxesConfig,
    presets: HashMap<String, CustomPreset>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawAxesConfig {
    agency: HashMap<String, String>,
    quality: HashMap<String, String>,
    scope: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Public config types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct AxesConfig {
    pub agency: HashMap<String, PathBuf>,
    pub quality: HashMap<String, PathBuf>,
    pub scope: HashMap<String, PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomPreset {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub agency: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub context_pacing: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn load_valid_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "default_base": "standard",
                "default_modifiers": ["readonly"],
                "modifiers": {
                    "my-mod": "/tmp/test.md"
                },
                "bases": {
                    "mybase": "/tmp/mybase"
                },
                "axes": {
                    "agency": {
                        "custom-agent": "/tmp/agent.md"
                    }
                },
                "presets": {
                    "custom-preset": {
                        "base": "mybase",
                        "agency": "custom-agent",
                        "quality": "pragmatic",
                        "scope": "narrow",
                        "modifiers": ["my-mod"],
                        "readonly": false,
                        "context_pacing": true
                    }
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(config.default_base.as_deref(), Some("standard"));
        assert_eq!(config.default_modifiers, vec!["readonly"]);
        assert_eq!(config.modifiers.len(), 1);
        assert!(config.modifiers.contains_key("my-mod"));
        assert_eq!(
            config.modifiers.get("my-mod").unwrap(),
            &PathBuf::from("/tmp/test.md")
        );
        assert!(config.bases.contains_key("mybase"));
        assert!(config.axes.agency.contains_key("custom-agent"));
        assert!(config.presets.contains_key("custom-preset"));
    }

    #[test]
    fn missing_config_file_returns_empty() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("nonexistent.json");
        assert!(!config_path.exists());
        assert!(Config::load_from(&config_path).is_err());
    }

    #[test]
    fn load_empty_default() {
        let config = Config::default();
        assert!(config.default_base.is_none());
        assert!(config.default_modifiers.is_empty());
        assert!(config.bases.is_empty());
        assert!(config.modifiers.is_empty());
        assert!(config.axes.agency.is_empty());
        assert!(config.axes.quality.is_empty());
        assert!(config.axes.scope.is_empty());
        assert!(config.presets.is_empty());
    }

    #[test]
    fn relative_paths_resolved_against_config_dir() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "modifiers": {
                    "mymod": "relative/path/haiku.md"
                },
                "bases": {
                    "mybase": "relative/base"
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        let expected_mod = dir.path().join("relative/path/haiku.md");
        let expected_base = dir.path().join("relative/base");
        assert_eq!(config.modifiers.get("mymod").unwrap(), &expected_mod);
        assert_eq!(config.bases.get("mybase").unwrap(), &expected_base);
    }

    #[test]
    fn absolute_paths_not_resolved_relative() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "modifiers": {
                    "mymod": "/absolute/path/haiku.md"
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(
            config.modifiers.get("mymod").unwrap(),
            &PathBuf::from("/absolute/path/haiku.md")
        );
    }

    #[test]
    fn collision_preset_name_warns() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "presets": {
                    "safe": {
                        "agency": "collaborative",
                        "quality": "minimal",
                        "scope": "narrow"
                    }
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert!(config.presets.contains_key("safe"));
    }

    #[test]
    fn collision_modifier_name_warns() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "modifiers": {
                    "readonly": "/tmp/ro.md"
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert!(config.modifiers.contains_key("readonly"));
    }

    #[test]
    fn missing_path_warns() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        fs::write(
            &config_path,
            r#"{
                "modifiers": {
                    "ghost": "/nonexistent/path.md"
                }
            }"#,
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert!(config.modifiers.contains_key("ghost"));
    }
}
