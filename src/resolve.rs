use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::Config;
use crate::presets::{self, AGENCY_VALUES, QUALITY_VALUES, SCOPE_VALUES};
use crate::presets::{BUILTIN_MODIFIERS, DEFAULT_AGENCY, DEFAULT_QUALITY, DEFAULT_SCOPE};

/// Errors from the resolution process.
#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("unknown preset '{name}'. Available: {available}")]
    UnknownPreset { name: String, available: String },
    #[error("unknown base '{name}'")]
    UnknownBase { name: String },
    #[error("unknown base '{name}'. The 'chill' base is not yet ported.")]
    ChillBaseNotPorted { name: String },
    #[error("unknown modifier '{name}'")]
    UnknownModifier { name: String },
    #[error("unknown {axis} axis value '{name}'")]
    UnknownAxisValue { axis: String, name: String },
    #[error("{0}")]
    Other(String),
}

/// Fully resolved configuration ready for assembly.
#[derive(Debug)]
pub struct Resolved {
    pub base_dir: PathBuf,
    pub axis_paths: Vec<PathBuf>,
    pub modifier_paths: Vec<PathBuf>,
    pub append_system_prompt: Option<String>,
}

/// Input parameters for the resolver.
pub struct ResolveArgs<'a> {
    pub preset: Option<&'a str>,
    pub agency: Option<&'a str>,
    pub quality: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub modifiers: &'a [String],
    pub readonly: bool,
    pub context_pacing: bool,
    pub base: Option<&'a str>,
    pub append_system_prompt: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Main resolve entry point
// ---------------------------------------------------------------------------

/// Resolve CLI arguments + config into fully resolved paths.
pub fn resolve(
    args: &ResolveArgs,
    config: &Config,
    prompts_dir: &Path,
) -> Result<Resolved, ResolveError> {
    // 1. Preset resolution
    let preset_name = args.preset.unwrap_or("none");
    let (preset_axes, preset_modifiers, preset_base) = resolve_preset(preset_name, config)?;

    // 2. Axis values: CLI override > preset > default (if not "none")
    let is_none_mode = preset_name == "none" && args.agency.is_none()
        && args.quality.is_none() && args.scope.is_none();

    let agency_val = if is_none_mode {
        None
    } else {
        Some(
            args.agency
                .or(preset_axes.agency.as_deref())
                .unwrap_or(DEFAULT_AGENCY),
        )
    };
    let quality_val = if is_none_mode {
        None
    } else {
        Some(
            args.quality
                .or(preset_axes.quality.as_deref())
                .unwrap_or(DEFAULT_QUALITY),
        )
    };
    let scope_val = if is_none_mode {
        None
    } else {
        Some(
            args.scope
                .or(preset_axes.scope.as_deref())
                .unwrap_or(DEFAULT_SCOPE),
        )
    };

    // 3. Resolve each axis value to a path
    let mut axis_paths = Vec::new();
    if let Some(v) = agency_val {
        axis_paths.push(resolve_axis_value("agency", v, config, prompts_dir)?);
    }
    if let Some(v) = quality_val {
        axis_paths.push(resolve_axis_value("quality", v, config, prompts_dir)?);
    }
    if let Some(v) = scope_val {
        axis_paths.push(resolve_axis_value("scope", v, config, prompts_dir)?);
    }

    // 4. Base resolution
    let base_value = args
        .base
        .or(preset_base.as_deref())
        .or(config.default_base.as_deref())
        .unwrap_or("standard");
    let base_dir = resolve_base(base_value, config, prompts_dir)?;

    // 5. Modifier resolution
    let modifiers = resolve_modifier_list(
        config,
        preset_modifiers,
        args.modifiers,
        args.readonly,
        args.context_pacing,
    );

    let mut modifier_paths = Vec::new();
    for m in &modifiers {
        let path = resolve_modifier(m, config, prompts_dir)?;
        modifier_paths.push(path);
    }

    Ok(Resolved {
        base_dir,
        axis_paths,
        modifier_paths,
        append_system_prompt: args.append_system_prompt.map(|s| s.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Preset resolution
// ---------------------------------------------------------------------------

/// Resolve a preset name to its axes, modifiers, and base.
fn resolve_preset<'a>(
    name: &str,
    config: &'a Config,
) -> Result<(presets::AxisValues, Vec<String>, Option<String>), ResolveError> {
    // "none" is a special case — just returns empty
    if name == "none" {
        return Ok((presets::AxisValues::default(), Vec::new(), None));
    }

    // Built-in preset
    if let Some(preset) = presets::get_preset(name) {
        return Ok((preset.axes, preset.modifiers, None));
    }

    // Config custom preset
    if let Some(custom) = config.presets.get(name) {
        let axes = presets::AxisValues {
            agency: custom.agency.clone(),
            quality: custom.quality.clone(),
            scope: custom.scope.clone(),
        };
        let mut modifiers = custom.modifiers.clone();
        if custom.readonly {
            modifiers.push("readonly".to_string());
        }
        if custom.context_pacing {
            modifiers.push("context-pacing".to_string());
        }
        let base = custom.base.clone();
        return Ok((axes, modifiers, base));
    }

    // Unknown — list available presets
    let available = build_available_presets(config);
    Err(ResolveError::UnknownPreset {
        name: name.to_string(),
        available,
    })
}

fn build_available_presets(config: &Config) -> String {
    let mut all: Vec<&str> = presets::BUILTIN_PRESETS.to_vec();
    let mut custom: Vec<&str> = config.presets.keys().map(|s| s.as_str()).collect();
    custom.sort();
    all.extend(custom);
    all.join(", ")
}

// ---------------------------------------------------------------------------
// Axis value resolution
// ---------------------------------------------------------------------------

fn resolve_axis_value(
    axis: &str,
    value: &str,
    config: &Config,
    prompts_dir: &Path,
) -> Result<PathBuf, ResolveError> {
    // File path heuristic
    if is_path_like(value) {
        return Ok(PathBuf::from(value));
    }

    // Built-in axis value
    let builtin = match axis {
        "agency" => AGENCY_VALUES,
        "quality" => QUALITY_VALUES,
        "scope" => SCOPE_VALUES,
        _ => {
            return Err(ResolveError::Other(format!("unknown axis: {axis}")));
        }
    };
    if builtin.contains(&value) {
        let subdir = prompts_dir.join("axis").join(axis);
        return Ok(subdir.join(format!("{value}.md")));
    }

    // Config-defined axis name
    let config_map = match axis {
        "agency" => &config.axes.agency,
        "quality" => &config.axes.quality,
        "scope" => &config.axes.scope,
        _ => unreachable!(),
    };
    if let Some(path) = config_map.get(value) {
        return Ok(path.clone());
    }

    Err(ResolveError::UnknownAxisValue {
        axis: axis.to_string(),
        name: value.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Base resolution
// ---------------------------------------------------------------------------

fn resolve_base(
    name: &str,
    config: &Config,
    prompts_dir: &Path,
) -> Result<PathBuf, ResolveError> {
    // Directory path heuristic
    if name.contains('/') || name.contains('\\') {
        return Ok(PathBuf::from(name));
    }

    match name {
        "standard" => return Ok(prompts_dir.join("base")),
        "chill" => {
            return Err(ResolveError::ChillBaseNotPorted {
                name: name.to_string(),
            });
        }
        _ => {}
    }

    // Config-defined base
    if let Some(path) = config.bases.get(name) {
        return Ok(path.clone());
    }

    Err(ResolveError::UnknownBase {
        name: name.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Modifier resolution
// ---------------------------------------------------------------------------

/// Collect and deduplicate all modifier names from their sources.
fn resolve_modifier_list(
    config: &Config,
    preset_modifiers: Vec<String>,
    cli_modifiers: &[String],
    cli_readonly: bool,
    cli_context_pacing: bool,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    // 1. Config default modifiers
    for m in &config.default_modifiers {
        if seen.insert(m.clone()) {
            result.push(m.clone());
        }
    }

    // 2. Preset modifiers
    for m in preset_modifiers {
        if seen.insert(m.clone()) {
            result.push(m);
        }
    }

    // 3. CLI --modifier values
    for m in cli_modifiers {
        if seen.insert(m.clone()) {
            result.push(m.clone());
        }
    }

    // 4. --readonly and --context-pacing
    if cli_readonly && seen.insert("readonly".to_string()) {
        result.push("readonly".to_string());
    }
    if cli_context_pacing && seen.insert("context-pacing".to_string()) {
        result.push("context-pacing".to_string());
    }

    result
}

fn resolve_modifier(
    name: &str,
    config: &Config,
    prompts_dir: &Path,
) -> Result<PathBuf, ResolveError> {
    // File path heuristic
    if is_path_like(name) {
        return Ok(PathBuf::from(name));
    }

    // Built-in modifier
    if BUILTIN_MODIFIERS.contains(&name) {
        return Ok(prompts_dir.join("modifiers").join(format!("{name}.md")));
    }

    // Config-defined modifier
    if let Some(path) = config.modifiers.get(name) {
        return Ok(path.clone());
    }

    Err(ResolveError::UnknownModifier {
        name: name.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if the value looks like a file/directory path rather than a
/// built-in or config-defined name.
fn is_path_like(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.ends_with(".md")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, CustomPreset};
    use std::fs;
    use tempfile::tempdir;

    fn empty_config() -> Config {
        Config::default()
    }

    fn test_prompts_dir() -> PathBuf {
        PathBuf::from("/test/prompts")
    }

    // --- Built-in name resolution ---

    #[test]
    fn resolve_builtin_axis_agency() {
        let path =
            resolve_axis_value("agency", "collaborative", &empty_config(), &test_prompts_dir())
                .unwrap();
        assert_eq!(
            path,
            test_prompts_dir().join("axis/agency/collaborative.md")
        );
    }

    #[test]
    fn resolve_builtin_axis_quality() {
        let path =
            resolve_axis_value("quality", "minimal", &empty_config(), &test_prompts_dir()).unwrap();
        assert_eq!(
            path,
            test_prompts_dir().join("axis/quality/minimal.md")
        );
    }

    #[test]
    fn resolve_builtin_axis_scope() {
        let path =
            resolve_axis_value("scope", "narrow", &empty_config(), &test_prompts_dir()).unwrap();
        assert_eq!(
            path,
            test_prompts_dir().join("axis/scope/narrow.md")
        );
    }

    #[test]
    fn resolve_builtin_modifier() {
        let path = resolve_modifier("readonly", &empty_config(), &test_prompts_dir()).unwrap();
        assert_eq!(path, test_prompts_dir().join("modifiers/readonly.md"));
    }

    #[test]
    fn resolve_builtin_base() {
        let path = resolve_base("standard", &empty_config(), &test_prompts_dir()).unwrap();
        assert_eq!(path, test_prompts_dir().join("base"));
    }

    // --- Config-defined name resolution ---

    #[test]
    fn resolve_config_modifier() {
        let mut config = Config::default();
        config
            .modifiers
            .insert("my-mod".to_string(), PathBuf::from("/custom/mod.md"));
        let path = resolve_modifier("my-mod", &config, &test_prompts_dir()).unwrap();
        assert_eq!(path, PathBuf::from("/custom/mod.md"));
    }

    #[test]
    fn resolve_config_axis() {
        let mut config = Config::default();
        config.axes.agency.insert(
            "my-agent".to_string(),
            PathBuf::from("/custom/agent.md"),
        );
        let path =
            resolve_axis_value("agency", "my-agent", &config, &test_prompts_dir()).unwrap();
        assert_eq!(path, PathBuf::from("/custom/agent.md"));
    }

    #[test]
    fn resolve_config_base() {
        let mut config = Config::default();
        config
            .bases
            .insert("mybase".to_string(), PathBuf::from("/custom/base"));
        let path = resolve_base("mybase", &config, &test_prompts_dir()).unwrap();
        assert_eq!(path, PathBuf::from("/custom/base"));
    }

    // --- File path resolution ---

    #[test]
    fn resolve_modifier_path_with_slash() {
        let path =
            resolve_modifier("/tmp/test/haiku.md", &empty_config(), &test_prompts_dir()).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test/haiku.md"));
    }

    #[test]
    fn resolve_axis_value_path_with_dot_md() {
        let path =
            resolve_axis_value("agency", "my-custom.md", &empty_config(), &test_prompts_dir())
                .unwrap();
        assert_eq!(path, PathBuf::from("my-custom.md"));
    }

    // --- Unknown name errors ---

    #[test]
    fn resolve_unknown_modifier() {
        let err =
            resolve_modifier("bogus", &empty_config(), &test_prompts_dir()).unwrap_err();
        assert!(matches!(err, ResolveError::UnknownModifier { .. }));
    }

    #[test]
    fn resolve_unknown_axis() {
        let err = resolve_axis_value("agency", "bogus", &empty_config(), &test_prompts_dir())
            .unwrap_err();
        assert!(matches!(err, ResolveError::UnknownAxisValue { .. }));
    }

    #[test]
    fn resolve_unknown_base() {
        let err = resolve_base("bogus", &empty_config(), &test_prompts_dir()).unwrap_err();
        assert!(matches!(err, ResolveError::UnknownBase { .. }));
    }

    #[test]
    fn resolve_chill_base_error() {
        let err = resolve_base("chill", &empty_config(), &test_prompts_dir()).unwrap_err();
        assert!(matches!(err, ResolveError::ChillBaseNotPorted { .. }));
    }

    // --- Modifier list merging ---

    #[test]
    fn modifier_list_defaults_only() {
        let mut config = Config::default();
        config
            .default_modifiers
            .push("readonly".to_string());
        let result = resolve_modifier_list(&config, vec![], &[], false, false);
        assert_eq!(result, vec!["readonly"]);
    }

    #[test]
    fn modifier_list_preset_plus_cli() {
        let config = Config::default();
        let result = resolve_modifier_list(
            &config,
            vec!["debug".to_string()],
            &["bold".to_string()],
            false,
            false,
        );
        assert_eq!(result, vec!["debug", "bold"]);
    }

    #[test]
    fn modifier_list_dedup() {
        let config = Config::default();
        let result = resolve_modifier_list(
            &config,
            vec!["debug".to_string()],
            &["debug".to_string(), "bold".to_string()],
            false,
            false,
        );
        assert_eq!(result, vec!["debug", "bold"]);
    }

    #[test]
    fn modifier_list_readonly_flag() {
        let config = Config::default();
        let result = resolve_modifier_list(&config, vec![], &[], true, false);
        assert_eq!(result, vec!["readonly"]);
    }

    #[test]
    fn modifier_list_context_pacing_flag() {
        let config = Config::default();
        let result = resolve_modifier_list(&config, vec![], &[], false, true);
        assert_eq!(result, vec!["context-pacing"]);
    }

    #[test]
    fn modifier_list_order_default_preset_cli_flags() {
        let mut config = Config::default();
        config.default_modifiers.push("readonly".to_string());
        let result = resolve_modifier_list(
            &config,
            vec!["methodical".to_string()],
            &["bold".to_string()],
            true,
            true,
        );
        // readonly should not appear twice
        assert_eq!(
            result,
            vec!["readonly", "methodical", "bold", "context-pacing"]
        );
    }

    // --- CLI overrides beat preset ---

    #[test]
    fn cli_override_agency() {
        let dir = tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        fs::create_dir_all(prompts_dir.join("axis/agency")).unwrap();
        fs::create_dir_all(prompts_dir.join("axis/quality")).unwrap();
        fs::create_dir_all(prompts_dir.join("axis/scope")).unwrap();
        fs::write(
            prompts_dir.join("axis/agency/collaborative.md"),
            "agency: collaborative",
        )
        .unwrap();
        fs::write(
            prompts_dir.join("axis/agency/surgical.md"),
            "agency: surgical",
        )
        .unwrap();
        fs::write(
            prompts_dir.join("axis/quality/pragmatic.md"),
            "quality: pragmatic",
        )
        .unwrap();
        fs::write(
            prompts_dir.join("axis/scope/narrow.md"),
            "scope: narrow",
        )
        .unwrap();

        let config = Config::default();

        // Preset "safe" sets agency=collaborative, but CLI overrides to surgical
        let args = ResolveArgs {
            preset: Some("safe"),
            agency: Some("surgical"),
            quality: None,
            scope: None,
            modifiers: &[],
            readonly: false,
            context_pacing: false,
            base: None,
            append_system_prompt: None,
        };
        let resolved = resolve(&args, &config, &prompts_dir).unwrap();
        assert!(resolved
            .axis_paths
            .contains(&prompts_dir.join("axis/agency/surgical.md")));
        assert!(!resolved
            .axis_paths
            .contains(&prompts_dir.join("axis/agency/collaborative.md")));
    }

    // --- Preset resolution ---

    #[test]
    fn resolve_unknown_preset_error() {
        let config = Config::default();
        let err = resolve_preset("bogus", &config).unwrap_err();
        assert!(matches!(err, ResolveError::UnknownPreset { .. }));
    }

    #[test]
    fn resolve_custom_preset() {
        let mut config = Config::default();
        config.presets.insert(
            "custom".to_string(),
            CustomPreset {
                base: None,
                agency: Some("autonomous".to_string()),
                quality: None,
                scope: None,
                modifiers: vec!["readonly".to_string()],
                readonly: false,
                context_pacing: false,
            },
        );
        let (axes, modifiers, _base) = resolve_preset("custom", &config).unwrap();
        assert_eq!(axes.agency.as_deref(), Some("autonomous"));
        assert_eq!(axes.quality, None);
        assert_eq!(modifiers, vec!["readonly"]);
    }

    // --- Integration: full resolve produces paths ---

    #[test]
    fn resolve_safe_preset_no_config() {
        let dir = tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        let base_dir = prompts_dir.join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("base.json"), "[]").unwrap();
        fs::create_dir_all(prompts_dir.join("axis/agency")).unwrap();
        fs::create_dir_all(prompts_dir.join("axis/quality")).unwrap();
        fs::create_dir_all(prompts_dir.join("axis/scope")).unwrap();
        fs::write(
            prompts_dir.join("axis/agency/collaborative.md"),
            "",
        )
        .unwrap();
        fs::write(prompts_dir.join("axis/quality/minimal.md"), "").unwrap();
        fs::write(prompts_dir.join("axis/scope/narrow.md"), "").unwrap();

        let config = Config::default();
        let args = ResolveArgs {
            preset: Some("safe"),
            agency: None,
            quality: None,
            scope: None,
            modifiers: &[],
            readonly: false,
            context_pacing: false,
            base: None,
            append_system_prompt: None,
        };
        let resolved = resolve(&args, &config, &prompts_dir).unwrap();
        assert_eq!(resolved.base_dir, prompts_dir.join("base"));
        assert_eq!(resolved.axis_paths.len(), 3);
        assert_eq!(resolved.modifier_paths.len(), 0);
    }

    #[test]
    fn resolve_is_path_like() {
        assert!(is_path_like("/some/path.md"));
        assert!(is_path_like("relative/path.md"));
        assert!(is_path_like("\\windows\\path.md"));
        assert!(is_path_like("justfile.md"));
        assert!(!is_path_like("readonly"));
        assert!(!is_path_like("my-mod"));
    }
}
