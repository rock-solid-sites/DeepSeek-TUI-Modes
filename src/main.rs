mod api;
mod assemble;
mod config;
mod daemon;
mod presets;
mod resolve;

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
///
/// # Presets
///
/// | Preset       | Agency         | Quality      | Scope          |
/// |--------------|----------------|--------------|----------------|
/// | `none`       | — (no axes)    | —            | —              |
/// | `safe`       | collaborative  | minimal      | narrow         |
/// | `create`     | autonomous     | architect    | unrestricted   |
/// | `extend`     | autonomous     | pragmatic    | adjacent       |
/// | `refactor`   | autonomous     | pragmatic    | unrestricted   |
/// | `explore`    | collaborative  | architect    | narrow         |
/// | `debug`      | collaborative  | pragmatic    | narrow         |
/// | `methodical` | surgical       | architect    | narrow         |
/// | `director`   | collaborative  | architect    | unrestricted   |
/// | `partner`    | partner        | pragmatic    | adjacent       |
/// | `muse`       | autonomous     | architect    | unrestricted   |
///
/// # Modifiers
///
/// Built-in modifiers can be combined with any preset via `--modifier`.
/// Valid: readonly, context-pacing, debug, methodical, director, bold,
/// speak-plain, tdd, muse.
#[derive(Parser)]
#[command(name = "deepseek-tui-modes", version, about)]
struct Cli {
    /// Preset name. One of: none, safe, create, extend, refactor,
    /// explore, debug, methodical, director, partner, muse.
    /// Defaults to "none" when omitted.
    preset: Option<String>,

    /// Agency axis: name, path, or built-in (autonomous, collaborative,
    /// partner, surgical).
    #[arg(long)]
    agency: Option<String>,

    /// Quality axis: name, path, or built-in (architect, minimal, pragmatic).
    #[arg(long)]
    quality: Option<String>,

    /// Scope axis: name, path, or built-in (adjacent, narrow, unrestricted).
    #[arg(long)]
    scope: Option<String>,

    /// Base selection: "standard", config-defined name, or directory path.
    #[arg(long)]
    base: Option<String>,

    /// Workspace path (defaults to current directory).
    #[arg(long)]
    workspace: Option<String>,

    /// Debug: print the assembled prompt and exit.
    #[arg(long)]
    print: bool,

    /// Modifier name or file path. Repeatable.
    #[arg(long, value_name = "NAME_OR_PATH")]
    modifier: Vec<String>,

    /// Shorthand for --modifier readonly.
    #[arg(long)]
    readonly: bool,

    /// Shorthand for --modifier context-pacing.
    #[arg(long)]
    context_pacing: bool,

    /// Append additional text after the assembled prompt.
    #[arg(long)]
    append_system_prompt: Option<String>,

    /// Passthrough arguments forwarded after `--`.
    #[arg(last = true)]
    passthrough: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    // Load config (empty default if no file found).
    let config = config::Config::load();
    let prompts_dir = find_prompts_dir();

    if cli.print {
        print_and_exit(&cli, &config, &prompts_dir);
        return;
    }

    if let Err(e) = run(cli, &config, &prompts_dir) {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

/// Assembles the prompt, spawns the daemon, creates the operational thread,
/// and blocks until Ctrl+C.
fn run(
    cli: Cli,
    config: &config::Config,
    prompts_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    });

    let preset = cli.preset.clone().unwrap_or_else(|| "none".to_string());

    // -- Resolution --------------------------------------------------------------
    let args = resolve::ResolveArgs {
        preset: cli.preset.as_deref(),
        agency: cli.agency.as_deref(),
        quality: cli.quality.as_deref(),
        scope: cli.scope.as_deref(),
        modifiers: &cli.modifier,
        readonly: cli.readonly,
        context_pacing: cli.context_pacing,
        base: cli.base.as_deref(),
        append_system_prompt: cli.append_system_prompt.as_deref(),
    };
    let resolved = resolve::resolve(&args, config, prompts_dir)?;

    // -- Assembly ---------------------------------------------------------------
    let options = assemble::AssembleOptions {
        base_dir: resolved.base_dir,
        axis_paths: resolved.axis_paths,
        modifier_paths: resolved.modifier_paths,
    };
    let mut assembled = assemble::assemble_prompt(&options)?;

    // Append system prompt if provided
    if let Some(ref text) = resolved.append_system_prompt {
        assembled.push_str("\n\n");
        assembled.push_str(text);
    }

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
    println!("  curl -X POST http://127.0.0.1:{port}/v1/threads/{thread_id}/turns \\");
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
fn print_and_exit(cli: &Cli, config: &config::Config, prompts_dir: &PathBuf) {
    let args = resolve::ResolveArgs {
        preset: cli.preset.as_deref(),
        agency: cli.agency.as_deref(),
        quality: cli.quality.as_deref(),
        scope: cli.scope.as_deref(),
        modifiers: &cli.modifier,
        readonly: cli.readonly,
        context_pacing: cli.context_pacing,
        base: cli.base.as_deref(),
        append_system_prompt: cli.append_system_prompt.as_deref(),
    };

    let resolved = match resolve::resolve(&args, config, prompts_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let options = assemble::AssembleOptions {
        base_dir: resolved.base_dir,
        axis_paths: resolved.axis_paths,
        modifier_paths: resolved.modifier_paths,
    };

    match assemble::assemble_prompt(&options) {
        Ok(mut prompt) => {
            if let Some(ref text) = resolved.append_system_prompt {
                prompt.push_str("\n\n");
                prompt.push_str(text);
            }
            println!("{prompt}");
        }
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
