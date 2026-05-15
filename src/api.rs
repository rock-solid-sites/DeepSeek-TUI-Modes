use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("doctor command failed: {0}")]
    DoctorExec(String),
    #[error("unable to parse doctor output: {0}")]
    DoctorParse(String),
    #[error("deepseek-tui version {version} is too old; minimum required is 0.8.10")]
    VersionTooOld { version: String },
    #[error("thread creation failed (HTTP {status}): {body}")]
    ThreadCreate { status: u16, body: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Deserialize)]
struct DoctorOutput {
    version: String,
}

#[derive(Deserialize)]
struct CreateThreadResponse {
    id: String,
}

/// Parses a semver-like string into a comparable tuple.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = v.splitn(3, '.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        // Take only the numeric prefix of the patch component (handles
        // pre-release suffixes like "0.8.37-rc1").
        parts[2]
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()?,
    ))
}

/// Runs `deepseek-tui doctor --json` and asserts the version is >= 0.8.10.
pub fn check_version(binary: &Path) -> Result<(), ApiError> {
    let output = std::process::Command::new(binary)
        .args(["doctor", "--json"])
        .output()
        .map_err(|e| ApiError::DoctorExec(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::DoctorExec(format!(
            "exit code {}: {stderr}",
            output.status.code().unwrap_or(-1)
        )));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let doctor: DoctorOutput =
        serde_json::from_str(&raw).map_err(|e| ApiError::DoctorParse(e.to_string()))?;

    let version = parse_version(&doctor.version).ok_or_else(|| {
        ApiError::DoctorParse(format!("unparseable version string: {}", doctor.version))
    })?;

    let min_version = (0, 8, 10);
    if version < min_version {
        return Err(ApiError::VersionTooOld {
            version: doctor.version,
        });
    }

    Ok(())
}

/// Creates a thread on the daemon with the assembled system prompt.
///
/// Uses a short connect timeout (500ms) and a 10s overall timeout for this
/// structural route.
pub fn create_thread(
    port: u16,
    auth_token: &str,
    system_prompt: &str,
    workspace: &str,
) -> Result<String, ApiError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client build should never fail");

    let url = format!("http://127.0.0.1:{port}/v1/threads");

    let body = serde_json::json!({
        "system_prompt": system_prompt,
        "workspace": workspace,
        "auto_approve": true,
        "mode": "agent",
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .json(&body)
        .send()?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(ApiError::ThreadCreate {
            status: status.as_u16(),
            body: text,
        });
    }

    let data: CreateThreadResponse = resp.json()?;
    Ok(data.id)
}
