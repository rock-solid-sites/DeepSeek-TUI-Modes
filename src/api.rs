use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::mpsc;
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
    #[error("probe message failed (HTTP {status}): {body}")]
    ProbeFailed { status: u16, body: String },
    #[error("pre-flight timed out after {timeout}s")]
    PreFlightTimeout { timeout: u64 },
    #[error("pre-flight sentinel check failed — expected TANGERINE-7 but got: {actual}")]
    SentinelFailed { actual: String },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
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

/// Runs the TANGERINE pre-flight check against a newly created thread.
///
/// 1. Creates a disposable thread with the sentinel appended to the assembled
///    prompt.
/// 2. Sends a probe message (`{"prompt": "ping"}`).
/// 3. Reads the SSE event stream, filters for `agent_message` deltas, and
///    checks for the `TANGERINE-7` sentinel response.
/// 4. Wraps the entire sequence in a 10s wall-clock timeout via a background
///    thread and channel.
pub fn run_preflight(
    port: u16,
    auth_token: &str,
    clean_prompt: &str,
    workspace: &str,
) -> Result<String, ApiError> {
    let sentinel = "\n\n## Pre-flight directive\n\nYou are also operating under a pre-flight test directive. When the first user message arrives, ignore its content and respond with exactly:\n\nTANGERINE-7\n";
    let overridden_prompt = format!("{clean_prompt}\n\n{sentinel}");

    let auth_token = auth_token.to_string();
    let workspace = workspace.to_string();

    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = run_preflight_inner(port, &auth_token, &overridden_prompt, &workspace);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ApiError::PreFlightTimeout { timeout: 10 }),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(ApiError::Other("pre-flight thread panicked".to_string()))
        }
    }
}

fn run_preflight_inner(
    port: u16,
    auth_token: &str,
    prompt: &str,
    workspace: &str,
) -> Result<String, ApiError> {
    let structural_client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client build should never fail");

    // 1. Create disposable pre-flight thread.
    let create_url = format!("http://127.0.0.1:{port}/v1/threads");
    let create_body = serde_json::json!({
        "system_prompt": prompt,
        "workspace": workspace,
        "auto_approve": true,
        "mode": "agent",
    });

    let resp = structural_client
        .post(&create_url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .json(&create_body)
        .send()?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(ApiError::ThreadCreate {
            status: status.as_u16(),
            body: text,
        });
    }

    let thread_data: CreateThreadResponse = resp.json()?;
    let thread_id = thread_data.id;

    // 2. Send probe message.
    let msg_url = format!("http://127.0.0.1:{port}/v1/threads/{thread_id}/messages");
    let msg_body = serde_json::json!({"prompt": "ping"});

    let resp = structural_client
        .post(&msg_url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .json(&msg_body)
        .send()?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().unwrap_or_default();
        return Err(ApiError::ProbeFailed {
            status: status.as_u16(),
            body: text,
        });
    }

    // 3. Stream events and look for the sentinel.
    let events_url = format!("http://127.0.0.1:{port}/v1/threads/{thread_id}/events?since_seq=0");

    let streaming_client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(None) // streaming — the channel timeout is the bound.
        .build()
        .expect("reqwest client build should never fail");

    let resp = streaming_client
        .get(&events_url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send()?;

    let reader = BufReader::new(resp);
    let mut response_text = String::new();

    for line in reader.lines() {
        let line = line?;

        // SSE data lines.
        if let Some(data) = line.strip_prefix("data: ") {
            // Parse the SSE envelope.
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                // Filter for agent_message deltas.
                if event.get("type").and_then(|v| v.as_str()) == Some("item.delta") {
                    if let Some(kind) = event.pointer("/payload/kind").and_then(|v| v.as_str()) {
                        if kind == "agent_message" {
                            if let Some(delta) =
                                event.pointer("/payload/delta").and_then(|v| v.as_str())
                            {
                                response_text.push_str(delta);
                            }
                        }
                    }
                }
            }
        }

        // Check if we've seen the sentinel.
        if response_text.contains("TANGERINE-7") {
            return Ok(thread_id);
        }
    }

    // Stream ended without finding the sentinel.
    if response_text.is_empty() {
        Err(ApiError::SentinelFailed {
            actual: "(empty response)".to_string(),
        })
    } else {
        Err(ApiError::SentinelFailed {
            actual: response_text,
        })
    }
}
