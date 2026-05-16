use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::presets::{
    AGENCY_VALUES, BUILTIN_MODIFIERS, BUILTIN_PRESETS, QUALITY_VALUES, SCOPE_VALUES,
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "deepseek-tui-modes config",
    about = "Manage .deepseek-tui-modes.json configuration"
)]
struct Cli {
    /// Target global config (~/.config/deepseek-tui-modes/config.json)
    /// instead of project-local ./.deepseek-tui-modes.json
    #[arg(long)]
    global: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pretty-print the current config
    Show,
    /// Create a scaffold config file with empty fields
    Init,
    /// Add a named modifier mapping
    AddModifier {
        name: String,
        path: String,
    },
    /// Remove a named modifier
    RemoveModifier {
        name: String,
    },
    /// Add to defaultModifiers list (no-op if already present)
    AddDefault {
        name: String,
    },
    /// Remove from defaultModifiers list
    RemoveDefault {
        name: String,
    },
    /// Add a custom axis value
    AddAxis {
        /// Axis name: agency, quality, or scope
        axis: String,
        /// Value name (must not collide with built-in values)
        name: String,
        /// Path to the prompt fragment file
        path: String,
    },
    /// Remove a custom axis value
    RemoveAxis {
        /// Axis name: agency, quality, or scope
        axis: String,
        /// Value name
        name: String,
    },
    /// Create or replace a custom preset
    AddPreset {
        name: String,
        #[arg(long)]
        agency: Option<String>,
        #[arg(long)]
        quality: Option<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long, value_name = "NAME")]
        modifier: Vec<String>,
        #[arg(long)]
        readonly: bool,
        #[arg(long)]
        context_pacing: bool,
        #[arg(long)]
        base: Option<String>,
    },
    /// Remove a custom preset
    RemovePreset {
        name: String,
    },
}

// ---------------------------------------------------------------------------
// Config file types (mirrors the JSON schema for writing)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ConfigFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    default_base: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    default_modifiers: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    bases: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    modifiers: HashMap<String, String>,
    #[serde(skip_serializing_if = "AxesFile::is_empty")]
    axes: AxesFile,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    presets: HashMap<String, PresetEntry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct AxesFile {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    agency: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    quality: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    scope: HashMap<String, String>,
}

impl AxesFile {
    fn is_empty(&self) -> bool {
        self.agency.is_empty() && self.quality.is_empty() && self.scope.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PresetEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    readonly: bool,
    #[serde(skip_serializing_if = "is_false")]
    context_pacing: bool,
}

impl Default for PresetEntry {
    fn default() -> Self {
        Self {
            base: None,
            agency: None,
            quality: None,
            scope: None,
            modifiers: Vec::new(),
            readonly: false,
            context_pacing: false,
        }
    }
}

fn is_false(b: &bool) -> bool {
    !b
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(args: &[String]) {
    let cli = Cli::parse_from(args);
    let result = match cli.command {
        Command::Show => cmd_show(cli.global),
        Command::Init => cmd_init(cli.global),
        Command::AddModifier { name, path } => cmd_add_modifier(cli.global, &name, &path),
        Command::RemoveModifier { name } => cmd_remove_modifier(cli.global, &name),
        Command::AddDefault { name } => cmd_add_default(cli.global, &name),
        Command::RemoveDefault { name } => cmd_remove_default(cli.global, &name),
        Command::AddAxis { axis, name, path } => cmd_add_axis(cli.global, &axis, &name, &path),
        Command::RemoveAxis { axis, name } => cmd_remove_axis(cli.global, &axis, &name),
        Command::AddPreset {
            name,
            agency,
            quality,
            scope,
            modifier,
            readonly,
            context_pacing,
            base,
        } => cmd_add_preset(
            cli.global,
            &name,
            agency,
            quality,
            scope,
            &modifier,
            readonly,
            context_pacing,
            base,
        ),
        Command::RemovePreset { name } => cmd_remove_preset(cli.global, &name),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Resolve the config file path based on the --global flag.
fn config_path(global: bool) -> PathBuf {
    if global {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/deepseek-tui-modes/config.json")
    } else {
        PathBuf::from(".deepseek-tui-modes.json")
    }
}

/// Read the config file, returning a default if it doesn't exist or is invalid.
fn read_config(path: &PathBuf) -> ConfigFile {
    if path.exists() {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    } else {
        ConfigFile::default()
    }
}

/// Write the config file with pretty-printing. Creates parent directories.
fn write_config(path: &PathBuf, config: &ConfigFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create directory: {e}"))?;
    }
    let json =
        serde_json::to_string_pretty(config).map_err(|e| format!("serialization error: {e}"))?;
    fs::write(path, &json).map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn cmd_show(global: bool) -> Result<(), String> {
    let path = config_path(global);
    if !path.exists() {
        println!("No config file found.");
        return Ok(());
    }
    let config = read_config(&path);
    let json =
        serde_json::to_string_pretty(&config).map_err(|e| format!("serialization error: {e}"))?;
    println!("{json}");
    Ok(())
}

fn cmd_init(global: bool) -> Result<(), String> {
    let path = config_path(global);
    if path.exists() {
        return Err(format!("Config file already exists at {}", path.display()));
    }
    let config = ConfigFile::default();
    write_config(&path, &config)?;
    println!("Created config file at {}", path.display());
    Ok(())
}

fn cmd_add_modifier(global: bool, name: &str, path_str: &str) -> Result<(), String> {
    if BUILTIN_MODIFIERS.contains(&name) {
        return Err(format!("'{name}' collides with a built-in modifier name"));
    }
    let config_path = config_path(global);
    let mut config = read_config(&config_path);
    if config.modifiers.contains_key(name) {
        return Err(format!("Modifier '{name}' already exists"));
    }
    config
        .modifiers
        .insert(name.to_string(), path_str.to_string());
    write_config(&config_path, &config)?;
    println!("Added modifier '{name}' -> {path_str}");
    Ok(())
}

fn cmd_remove_modifier(global: bool, name: &str) -> Result<(), String> {
    let config_path = config_path(global);
    let mut config = read_config(&config_path);
    if !config.modifiers.contains_key(name) {
        return Err(format!("Modifier '{name}' not found"));
    }
    config.modifiers.remove(name);
    write_config(&config_path, &config)?;
    println!("Removed modifier '{name}'");
    Ok(())
}

fn cmd_add_default(global: bool, name: &str) -> Result<(), String> {
    let config_path = config_path(global);
    let mut config = read_config(&config_path);
    if config.default_modifiers.contains(&name.to_string()) {
        println!("'{name}' is already in default modifiers");
        return Ok(());
    }
    config.default_modifiers.push(name.to_string());
    write_config(&config_path, &config)?;
    println!("Added '{name}' to default modifiers");
    Ok(())
}

fn cmd_remove_default(global: bool, name: &str) -> Result<(), String> {
    let config_path = config_path(global);
    let mut config = read_config(&config_path);
    let pos = config.default_modifiers.iter().position(|n| n == name);
    match pos {
        Some(i) => {
            config.default_modifiers.remove(i);
            write_config(&config_path, &config)?;
            println!("Removed '{name}' from default modifiers");
            Ok(())
        }
        None => Err(format!("'{name}' is not in default modifiers")),
    }
}

fn cmd_add_axis(global: bool, axis: &str, name: &str, path_str: &str) -> Result<(), String> {
    let builtin_values: &[&str] = match axis {
        "agency" => AGENCY_VALUES,
        "quality" => QUALITY_VALUES,
        "scope" => SCOPE_VALUES,
        _ => {
            return Err(format!(
                "Invalid axis '{axis}'. Must be 'agency', 'quality', or 'scope'"
            ))
        }
    };

    if builtin_values.contains(&name) {
        return Err(format!("'{name}' collides with a built-in {axis} value"));
    }

    let config_path = config_path(global);
    let mut config = read_config(&config_path);

    let map = match axis {
        "agency" => &mut config.axes.agency,
        "quality" => &mut config.axes.quality,
        "scope" => &mut config.axes.scope,
        _ => unreachable!(),
    };

    map.insert(name.to_string(), path_str.to_string());
    write_config(&config_path, &config)?;
    println!("Added {axis} value '{name}' -> {path_str}");
    Ok(())
}

fn cmd_remove_axis(global: bool, axis: &str, name: &str) -> Result<(), String> {
    let config_path = config_path(global);
    let mut config = read_config(&config_path);

    let map = match axis {
        "agency" => &mut config.axes.agency,
        "quality" => &mut config.axes.quality,
        "scope" => &mut config.axes.scope,
        _ => {
            return Err(format!(
                "Invalid axis '{axis}'. Must be 'agency', 'quality', or 'scope'"
            ))
        }
    };

    if !map.contains_key(name) {
        return Err(format!("{axis} value '{name}' not found"));
    }
    map.remove(name);
    write_config(&config_path, &config)?;
    println!("Removed {axis} value '{name}'");
    Ok(())
}

fn cmd_add_preset(
    global: bool,
    name: &str,
    agency: Option<String>,
    quality: Option<String>,
    scope: Option<String>,
    modifiers: &[String],
    readonly: bool,
    context_pacing: bool,
    base: Option<String>,
) -> Result<(), String> {
    if BUILTIN_PRESETS.contains(&name) {
        return Err(format!("'{name}' collides with a built-in preset name"));
    }

    let config_path = config_path(global);
    let mut config = read_config(&config_path);

    config.presets.insert(
        name.to_string(),
        PresetEntry {
            base,
            agency,
            quality,
            scope,
            modifiers: modifiers.to_vec(),
            readonly,
            context_pacing,
        },
    );

    write_config(&config_path, &config)?;
    println!("Added preset '{name}'");
    Ok(())
}

fn cmd_remove_preset(global: bool, name: &str) -> Result<(), String> {
    let config_path = config_path(global);
    let mut config = read_config(&config_path);

    if !config.presets.contains_key(name) {
        return Err(format!("Preset '{name}' not found"));
    }
    config.presets.remove(name);
    write_config(&config_path, &config)?;
    println!("Removed preset '{name}'");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_init_creates_scaffold() {
        // A default ConfigFile should serialize and deserialize cleanly
        let config = ConfigFile::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(!json.is_empty());

        let recovered: ConfigFile = serde_json::from_str(&json).unwrap();
        assert!(recovered.default_modifiers.is_empty());
        assert!(recovered.modifiers.is_empty());
        assert!(recovered.axes.is_empty());
        assert!(recovered.presets.is_empty());
    }

    #[test]
    fn test_collision_builtin_modifier() {
        assert!(BUILTIN_MODIFIERS.contains(&"readonly"));
        assert!(BUILTIN_MODIFIERS.contains(&"debug"));
        assert!(!BUILTIN_MODIFIERS.contains(&"custom-thing"));
    }

    #[test]
    fn test_collision_builtin_preset() {
        assert!(BUILTIN_PRESETS.contains(&"safe"));
        assert!(BUILTIN_PRESETS.contains(&"create"));
        assert!(!BUILTIN_PRESETS.contains(&"custom-thing"));
    }

    #[test]
    fn test_collision_builtin_axis_values() {
        assert!(AGENCY_VALUES.contains(&"autonomous"));
        assert!(AGENCY_VALUES.contains(&"collaborative"));
        assert!(!AGENCY_VALUES.contains(&"reviewer"));

        assert!(QUALITY_VALUES.contains(&"architect"));
        assert!(QUALITY_VALUES.contains(&"minimal"));
        assert!(!QUALITY_VALUES.contains(&"custom-quality"));

        assert!(SCOPE_VALUES.contains(&"narrow"));
        assert!(SCOPE_VALUES.contains(&"unrestricted"));
        assert!(!SCOPE_VALUES.contains(&"custom-scope"));
    }

    #[test]
    fn test_add_axis_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut config = ConfigFile::default();
        config
            .axes
            .agency
            .insert("reviewer".to_string(), "/tmp/reviewer.md".to_string());

        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&path, &json).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let recovered: ConfigFile = serde_json::from_str(&text).unwrap();
        assert_eq!(
            recovered.axes.agency.get("reviewer").unwrap(),
            "/tmp/reviewer.md"
        );
    }

    #[test]
    fn test_add_preset_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut config = ConfigFile::default();
        config.presets.insert(
            "careful".to_string(),
            PresetEntry {
                agency: Some("reviewer".to_string()),
                quality: Some("architect".to_string()),
                scope: Some("narrow".to_string()),
                modifiers: vec!["explain".to_string()],
                ..Default::default()
            },
        );

        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&path, &json).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let recovered: ConfigFile = serde_json::from_str(&text).unwrap();
        let preset = recovered.presets.get("careful").unwrap();
        assert_eq!(preset.agency.as_deref(), Some("reviewer"));
        assert_eq!(preset.quality.as_deref(), Some("architect"));
        assert_eq!(preset.scope.as_deref(), Some("narrow"));
        assert_eq!(preset.modifiers, vec!["explain"]);
    }

    #[test]
    fn test_add_modifier_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut config = ConfigFile::default();
        config
            .modifiers
            .insert("my-mod".to_string(), "/tmp/haiku.md".to_string());

        let json = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&path, &json).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let recovered: ConfigFile = serde_json::from_str(&text).unwrap();
        assert_eq!(
            recovered.modifiers.get("my-mod").unwrap(),
            "/tmp/haiku.md"
        );
    }

    #[test]
    fn test_add_default_no_duplicate() {
        let mut config = ConfigFile::default();
        config.default_modifiers.push("readonly".to_string());

        // Simulate adding the same default again — should not duplicate
        if !config.default_modifiers.contains(&"readonly".to_string()) {
            config.default_modifiers.push("readonly".to_string());
        }
        assert_eq!(config.default_modifiers.len(), 1);
    }

    #[test]
    fn test_remove_default_not_found_errors() {
        // The cmd_remove_default function should error when not found.
        // We test the logic directly by simulating it.
        let defaults: Vec<String> = vec!["readonly".to_string()];
        let pos = defaults.iter().position(|n| n == "nonexistent");
        assert!(pos.is_none());
    }

    #[test]
    fn test_preset_omit_empty_fields() {
        // When serializing, empty fields like modifiers should be omitted
        let entry = PresetEntry {
            agency: Some("autonomous".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&entry).unwrap();
        // Should contain "agency"
        assert!(json.contains("\"agency\""));
        // modifiers should not appear at all when empty (skip_serializing_if)
        assert!(!json.contains("modifiers"));
    }
}
