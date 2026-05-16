use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::presets::AxisValues;

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
}

#[derive(Debug, Deserialize)]
struct Manifest(Vec<String>);

#[derive(Debug)]
pub struct AssembleOptions {
    pub prompts_dir: PathBuf,
    pub base: String,
    pub axes: AxisValues,
}

fn base_to_dir(name: &str) -> &str {
    match name {
        "standard" => "base",
        other => other,
    }
}

pub fn assemble_prompt(options: &AssembleOptions) -> Result<String, AssembleError> {
    let base_dir = options.prompts_dir.join(base_to_dir(&options.base));

    let manifest_path = base_dir.join("base.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|e| AssembleError::ManifestIo {
        path: manifest_path.clone(),
        source: e,
    })?;
    let manifest: Manifest = serde_json::from_str(&raw)?;

    let mut sections: Vec<String> = Vec::new();

    for entry in &manifest.0 {
        match entry.as_str() {
            "axes" => {
                if !options.axes.is_empty() {
                    let axis_dir = options.prompts_dir.join("axis");

                    if let Some(ref val) = options.axes.agency {
                        let path = axis_dir.join("agency").join(format!("{val}.md"));
                        let content = fs::read_to_string(&path).map_err(|e| {
                            AssembleError::MissingAxisFragment {
                                path: path.clone(),
                                source: e,
                            }
                        })?;
                        sections.push(content.trim().to_string());
                    }
                    if let Some(ref val) = options.axes.quality {
                        let path = axis_dir.join("quality").join(format!("{val}.md"));
                        let content = fs::read_to_string(&path).map_err(|e| {
                            AssembleError::MissingAxisFragment {
                                path: path.clone(),
                                source: e,
                            }
                        })?;
                        sections.push(content.trim().to_string());
                    }
                    if let Some(ref val) = options.axes.scope {
                        let path = axis_dir.join("scope").join(format!("{val}.md"));
                        let content = fs::read_to_string(&path).map_err(|e| {
                            AssembleError::MissingAxisFragment {
                                path: path.clone(),
                                source: e,
                            }
                        })?;
                        sections.push(content.trim().to_string());
                    }
                }
            }
            "modifiers" => {}
            name => {
                let path = base_dir.join(name);
                let content =
                    fs::read_to_string(&path).map_err(|e| AssembleError::MissingFragment {
                        path: path.clone(),
                        source: e,
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

    #[test]
    fn assemble_none_mode() {
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        let base_dir = prompts_dir.join("base");
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
            prompts_dir,
            base: "standard".to_string(),
            axes: AxisValues::default(),
        };

        let result = assemble_prompt(&options).unwrap();
        assert!(result.contains("Intro content."));
        assert!(result.contains("Tone content."));
        assert_eq!(result, "Intro content.\n\nSystem content.\n\nTone content.");
    }

    #[test]
    fn assemble_safe_mode() {
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        let base_dir = prompts_dir.join("base");
        let axis_dir = prompts_dir.join("axis");
        fs::create_dir_all(&base_dir).unwrap();
        fs::create_dir_all(&axis_dir.join("agency")).unwrap();
        fs::create_dir_all(&axis_dir.join("quality")).unwrap();
        fs::create_dir_all(&axis_dir.join("scope")).unwrap();

        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "axes", "tone.md"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();
        fs::write(base_dir.join("tone.md"), "Tone.").unwrap();
        fs::write(
            axis_dir.join("agency/collaborative.md"),
            "Agency: collaborative",
        )
        .unwrap();
        fs::write(axis_dir.join("quality/minimal.md"), "Quality: minimal").unwrap();
        fs::write(axis_dir.join("scope/narrow.md"), "Scope: narrow").unwrap();

        let options = AssembleOptions {
            prompts_dir,
            base: "standard".to_string(),
            axes: AxisValues {
                agency: Some("collaborative".to_string()),
                quality: Some("minimal".to_string()),
                scope: Some("narrow".to_string()),
            },
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
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        let base_dir = prompts_dir.join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "nonexistent.md"]"#,
        )
        .unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();

        let options = AssembleOptions {
            prompts_dir,
            base: "standard".to_string(),
            axes: AxisValues::default(),
        };
        let err = assemble_prompt(&options).unwrap_err();
        assert!(matches!(err, AssembleError::MissingFragment { .. }));
    }

    #[test]
    fn assemble_missing_axis_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join("prompts");
        let base_dir = prompts_dir.join("base");
        fs::create_dir_all(&base_dir).unwrap();
        fs::write(base_dir.join("base.json"), r#"["intro.md", "axes"]"#).unwrap();
        fs::write(base_dir.join("intro.md"), "Intro.").unwrap();
        fs::create_dir_all(&prompts_dir.join("axis/agency")).unwrap();

        let options = AssembleOptions {
            prompts_dir,
            base: "standard".to_string(),
            axes: AxisValues {
                agency: Some("collaborative".to_string()),
                quality: None,
                scope: None,
            },
        };
        let err = assemble_prompt(&options).unwrap_err();
        assert!(matches!(err, AssembleError::MissingAxisFragment { .. }));
    }
}
