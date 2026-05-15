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
}

#[derive(Debug, Deserialize)]
struct Manifest(Vec<String>);

#[derive(Debug)]
pub struct AssembleOptions {
    pub prompts_dir: PathBuf,
    pub base: String,
    #[allow(dead_code)]
    pub preset: String,
}

/// Returns the directory name for a well-known base.
/// Mirrors upstream's mapping: `"standard"` → `"base"`.
fn base_to_dir(name: &str) -> &str {
    match name {
        "standard" => "base",
        other => other,
    }
}

/// Reads a manifest, walks its entries in order, and joins the resulting
/// fragments with `\n\n`.
///
/// For `"axes"` entries: expands to nothing (v0.1 only supports the `none`
/// preset, which strips all axis fragments).
///
/// For `"modifiers"` entries: expands to nothing (v0.1 supports no modifiers).
///
/// All other entries are treated as fragment filenames relative to
/// `prompts_dir/{base}/`.
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
                // v0.1: only `none` mode, which strips all axis content.
            }
            "modifiers" => {
                // v0.1: no modifiers supported.
            }
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
        // "standard" maps to the "base" directory.
        let base_dir = prompts_dir.join("base");
        fs::create_dir_all(&base_dir).unwrap();

        // Write minimal manifest
        fs::write(
            base_dir.join("base.json"),
            r#"["intro.md", "system.md", "axes", "tone.md", "modifiers"]"#,
        )
        .unwrap();

        // Write fragments with known content
        fs::write(base_dir.join("intro.md"), "Intro content.").unwrap();
        fs::write(base_dir.join("system.md"), "System content.").unwrap();
        fs::write(base_dir.join("tone.md"), "Tone content.").unwrap();

        let options = AssembleOptions {
            prompts_dir,
            base: "standard".to_string(),
            preset: "none".to_string(),
        };

        let result = assemble_prompt(&options).unwrap();

        assert!(
            result.contains("Intro content."),
            "result should contain intro content"
        );
        assert!(
            result.contains("Tone content."),
            "result should contain tone content"
        );
        // Axes and modifiers should NOT insert anything extra
        assert_eq!(result, "Intro content.\n\nSystem content.\n\nTone content.");
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
            preset: "none".to_string(),
        };

        let err = assemble_prompt(&options).unwrap_err();
        assert!(
            matches!(err, AssembleError::MissingFragment { .. }),
            "expected MissingFragment error, got {err}"
        );
    }
}
