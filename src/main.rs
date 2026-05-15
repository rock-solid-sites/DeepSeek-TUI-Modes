mod api;
mod assemble;
mod daemon;

use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;

/// Behavioral mode presets for DeepSeek-TUI.
///
/// Ports claude-code-modes to the DeepSeek-TUI daemon API, replacing the
/// behavioral layer of the system prompt while preserving DeepSeek-TUI's
/// infrastructure.
#[derive(Parser)]
#[command(name = "deepseek-tui-modes", version, about)]
struct Cli {
    /// Preset name (none, create, safe, ...).
    preset: Option<String>,

    /// Workspace path (defaults to current directory).
    #[arg(long)]
    workspace: Option<String>,

    /// Debug: print the assembled prompt and exit.
    #[arg(long)]
    print: bool,

    /// Passthrough arguments forwarded after `--`.
    #[arg(last = true)]
    passthrough: Vec<String>,
}

fn main() {
    // Simple print mode doesn't need daemon lifecycle.
    let cli = Cli::parse();
    if cli.print {
        print_and_exit(&cli);
        return;
    }

    // Run the full lifecycle. The Daemon's Drop impl runs when `run` returns,
    // regardless of success or error.
    if let Err(e) = run(cli) {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

/// Assembles the prompt, spawns the daemon, creates the operational thread,
/// and blocks until Ctrl+C.
fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = cli.workspace.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    });

    let preset = cli.preset.unwrap_or_else(|| "none".to_string());
    let prompts_dir = find_prompts_dir();

    // -- Assembly ---------------------------------------------------------------
    let options = assemble::AssembleOptions {
        prompts_dir,
        base: "standard".to_string(),
        preset: preset.clone(),
    };
    let assembled = assemble::assemble_prompt(&options)?;

    // -- Binary lookup -----------------------------------------------------------
    let binary = find_binary()?;

    // -- Version check -----------------------------------------------------------
    api::check_version(&binary)?;

    // -- Daemon lifecycle --------------------------------------------------------
    let daemon = daemon::Daemon::spawn(&binary)?;
    let port = daemon.port;
    let auth_token = daemon.auth_token.clone();

    eprintln!("Waiting for daemon on port {port}...");
    daemon::wait_for_health(port)?;
    eprintln!("Daemon is healthy.");

    // -- Create operational thread -----------------------------------------------
    eprintln!("Creating operational thread with assembled prompt...");
    let thread_id = api::create_thread(port, &auth_token, &assembled, &workspace)?;

    // -- Report ------------------------------------------------------------------
    println!("=== deepseek-tui-modes ({preset}) ===");
    println!("Thread ID:  {thread_id}");
    println!("Daemon URL: http://127.0.0.1:{port}");
    println!("Auth token: {auth_token}");
    println!();
    println!("Attach manually in another terminal:");
    println!("  curl -X POST http://127.0.0.1:{port}/v1/threads/{thread_id}/messages \\");
    println!("    -H \"Authorization: Bearer {auth_token}\" \\");
    println!("    -H \"Content-Type: application/json\" \\");
    println!("    -d '{{\"prompt\": \"hello from modes\"}}'");
    println!();
    println!("Or connect with the patched TUI binary.");
    println!("Press Ctrl+C to stop the daemon.");

    // -- Keep alive until Ctrl+C -------------------------------------------------
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }

    // daemon drops here, killing the child process.
    drop(daemon);
    Ok(())
}

/// Handles `--print`: assemble and print the prompt, then exit.
fn print_and_exit(cli: &Cli) {
    let prompts_dir = find_prompts_dir();
    let preset = cli.preset.clone().unwrap_or_else(|| "none".to_string());

    let options = assemble::AssembleOptions {
        prompts_dir,
        base: "standard".to_string(),
        preset,
    };

    match assemble::assemble_prompt(&options) {
        Ok(prompt) => println!("{prompt}"),
        Err(e) => {
            eprintln!("Error: failed to assemble prompt: {e}");
            process::exit(1);
        }
    }
}

/// Finds the `prompts/` directory relative to the executable or CWD.
fn find_prompts_dir() -> PathBuf {
    if let Some(exe) = std::env::current_exe()
        .ok()
        .as_ref()
        .and_then(|p| p.parent())
    {
        let candidate = exe.join("prompts");
        if candidate.is_dir() {
            return candidate;
        }
        // One level up (cargo run places binary in target/debug/).
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("prompts");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("prompts")
}

/// Locates the `deepseek-tui` binary — first in PATH, then at the fork path.
fn find_binary() -> Result<PathBuf, String> {
    if let Ok(path) = which::which("deepseek-tui") {
        return Ok(path);
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let fork_path = PathBuf::from(home).join("deepseek-tui-fork/target/release/deepseek-tui");
    if fork_path.exists() {
        return Ok(fork_path);
    }

    Err(format!(
        "deepseek-tui not found in PATH or at {fork_path:?}"
    ))
}
