use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AssembleError {
    #[error("failed to read manifest at {path}: {source}")]
    ManifestIo {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse manifest: {0}")]
    ManifestFormat(#[from] serde_json::Error),
    #[error("missing prompt fragment: {path}")]
    MissingFragment {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("missing axis fragment: {path}")]
    MissingAxisFragment {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("missing modifier fragment: {path}")]
    MissingModifierFragment {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Deserialize)]
struct Manifest(Vec<String>);

/// Options for prompt assembly.
///
/// All paths are pre-resolved — the assembler reads directly from them.
/// No need to know about the prompts/ directory structure or built-in names.
#[derive(Debug)]
pub struct AssembleOptions {
    /// Directory containing base.json and base fragment files.
    pub base_dir: PathBuf,
    /// Absolute paths to axis .md files (agency, quality, scope).
    pub axis_paths: Vec<PathBuf>,
    /// Absolute paths to modifier .md files.
    pub modifier_paths: Vec<PathBuf>,
}

pub fn assemble_prompt(options: &AssembleOptions) -> Result<String, AssembleError> {
    let manifest_path = options.base_dir.join("base.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|e| AssembleError::ManifestIo {
        path: manifest_path.clone(),
        source: e,
    })?;
    let manifest: Manifest = serde_json::from_str(&raw)?;

    let mut sections: Vec<String> = Vec::new();

    for entry in &manifest.0 {
        match entry.as_str() {
            "axes" => {
                for axis_path in &options.axis_paths {
                    let content = fs::read_to_string(axis_path).map_err(|e| {
                        AssembleError::MissingAxisFragment {
                            path: axis_path.clone(),
                            source: e,
                        }
                    })?;
                    sections.push(content.trim().to_string());
                }
            }
            "modifiers" => {
                for mod_path in &options.modifier_paths {
                    let content = fs::read_to_string(mod_path).map_err(|e| {
                        AssembleError::MissingModifierFragment {
                            path: mod_path.clone(),
                            source: e,
                        }
                    })?;
                    sections.push(content.trim().to_string());
                }
            }
            name => {
                let path = options.base_dir.join(name);
                let content = fs::read_to_string(&path).map_err(|e| {
                    AssembleError::MissingFragment {
                        path: path.clone(),
                        source: e,
                    }
                })?;
                sections.push(content.trim().to_string());
            }
        }
    }

    Ok(sections.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn assemble_none_mode() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();

        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "system.md", "axes", "tone.md", "modifiers"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro content.").unwrap();
        fs::write(base_dir.join("system.md"), "System content.").unwrap();
        fs::write(base_dir.join("tone.md"), "Tone content.").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![],
            modifier_paths: vec![],
        };

        let result = assemble_prompt(&options).unwrap();
        assert!(result.contains("Intro content."));
        assert!(result.contains("Tone content."));
        assert_eq!(result, "Intro content.\n\nSystem content.\n\nTone content.");
    }

    #[test]
    fn assemble_safe_mode() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();

        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "axes", "tone.md"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();
        fs::write(base_dir.join("tone.md"), "Tone.").unwrap();

        let axis_dir = dir.path().join("axis");
        fs::create_dir_all(&axis_dir.join("agency")).unwrap();
        fs::create_dir_all(&axis_dir.join("quality")).unwrap();
        fs::create_dir_all(&axis_dir.join("scope")).unwrap();
        fs::write(
            axis_dir.join("agency/collaborative.md"),
            "Agency: collaborative",
        )
        .unwrap();
        fs::write(axis_dir.join("quality/minimal.md"), "Quality: minimal").unwrap();
        fs::write(axis_dir.join("scope/narrow.md"), "Scope: narrow").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![
                axis_dir.join("agency/collaborative.md"),
                axis_dir.join("quality/minimal.md"),
                axis_dir.join("scope/narrow.md"),
            ],
            modifier_paths: vec![],
        };

        let result = assemble_prompt(&options).unwrap();
        assert!(result.contains("Agency: collaborative"));
        assert!(result.contains("Quality: minimal"));
        assert!(result.contains("Scope: narrow"));
        assert_eq!(
            result,
            "Intro.\n\nAgency: collaborative\n\nQuality: minimal\n\nScope: narrow\n\nTone."
        );
    }

    #[test]
    fn assemble_missing_fragment() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "nonexistent.md"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![],
            modifier_paths: vec![],
        };
        let err = assemble_prompt(&options).unwrap_err();
        assert!(matches!(err, AssembleError::MissingFragment { .. }));
    }

    #[test]
    fn assemble_missing_axis_fragment() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("base.json"), r#"["intro.md", "axes"]"#).unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![PathBuf::from("/nonexistent/agency.md")],
            modifier_paths: vec![],
        };
        let err = assemble_prompt(&options).unwrap_err();
        assert!(matches!(err, AssembleError::MissingAxisFragment { .. }));
    }

    #[test]
    fn assemble_with_modifiers() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();

        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "axes", "modifiers", "tone.md"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();
        fs::write(base_dir.join("tone.md"), "Tone.").unwrap();

        let mod_dir = dir.path().join("modifiers");
        fs::create_dir_all(&mod_dir).unwrap();
        fs::write(mod_dir.join("debug.md"), "Modifier: debug").unwrap();
        fs::write(mod_dir.join("bold.md"), "Modifier: bold").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![],
            modifier_paths: vec![
                mod_dir.join("debug.md"),
                mod_dir.join("bold.md"),
            ],
        };

        let result = assemble_prompt(&options).unwrap();
        assert!(result.contains("Modifier: debug"));
        assert!(result.contains("Modifier: bold"));
        assert_eq!(
            result,
            "Intro.\n\nModifier: debug\n\nModifier: bold\n\nTone."
        );
    }

    #[test]
    fn assemble_missing_modifier_fragment() {
        let dir = tempdir().unwrap();
        let base_dir = dir.path().join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("base.json"), r#"["intro.md", "modifiers"]"#).unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();

        let options = AssembleOptions {
            base_dir,
            axis_paths: vec![],
            modifier_paths: vec![PathBuf::from("/nonexistent/mod.md")],
        };
        let err = assemble_prompt(&options).unwrap_err();
        assert!(matches!(err, AssembleError::MissingModifierFragment { .. }));
    }
}
