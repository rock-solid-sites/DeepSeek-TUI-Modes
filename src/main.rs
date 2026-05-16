mod api;
mod assemble;
mod daemon;
mod presets;

use std::collections::HashSet;
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
/// | `muse`       | autonomous     | architect    | unrestricted   |
///
/// # Modifiers
///
/// Built-in modifiers can be combined with any preset via `--modifier`.
/// See `--help` for the full list.
#[derive(Parser)]
#[command(name = "deepseek-tui-modes", version, about)]
struct Cli {
    /// Preset name. One of: none, safe, create, extend, refactor,
    /// explore, debug, methodical, muse. Defaults to "none" when omitted.
    preset: Option<String>,

    /// Agency axis: autonomous, collaborative, partner, surgical.
    #[arg(long)]
    agency: Option<String>,

    /// Quality axis: architect, minimal, pragmatic.
    #[arg(long)]
    quality: Option<String>,

    /// Scope axis: adjacent, narrow, unrestricted.
    #[arg(long)]
    scope: Option<String>,

    /// Workspace path (defaults to current directory).
    #[arg(long)]
    workspace: Option<String>,

    /// Debug: print the assembled prompt and exit.
    #[arg(long)]
    print: bool,

    /// Built-in modifier name. Repeatable. Valid values: readonly,
    /// context-pacing, debug, methodical, director, bold, speak-plain,
    /// tdd, muse.
    #[arg(long, value_name = "NAME")]
    modifier: Vec<String>,

    /// Shorthand for --modifier readonly.
    #[arg(long)]
    readonly: bool,

    /// Shorthand for --modifier context-pacing.
    #[arg(long)]
    context_pacing: bool,

    /// Passthrough arguments forwarded after `--`.
    #[arg(last = true)]
    passthrough: Vec<String>,
}

/// Result of computing axes and modifiers from preset + CLI overrides.
struct ModeConfig {
    axes: presets::AxisValues,
    modifiers: Vec<String>,
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
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    });

    let preset = cli.preset.clone().unwrap_or_else(|| "none".to_string());
    let prompts_dir = find_prompts_dir();

    // -- Mode computation ----------------------------------------------------
    let mode = compute_mode(&cli);

    // -- Assembly ---------------------------------------------------------------
    let options = assemble::AssembleOptions {
        prompts_dir,
        base: "standard".to_string(),
        axes: mode.axes,
        modifiers: mode.modifiers,
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
fn print_and_exit(cli: &Cli) {
    let prompts_dir = find_prompts_dir();
    let mode = compute_mode(cli);

    let options = assemble::AssembleOptions {
        prompts_dir,
        base: "standard".to_string(),
        axes: mode.axes,
        modifiers: mode.modifiers,
    };

    match assemble::assemble_prompt(&options) {
        Ok(prompt) => println!("{prompt}"),
        Err(e) => {
            eprintln!("Error: failed to assemble prompt: {e}");
            process::exit(1);
        }
    }
}

/// Valid built-in modifier names.
const VALID_MODIFIERS: &[&str] = &[
    "readonly",
    "context-pacing",
    "debug",
    "methodical",
    "director",
    "bold",
    "speak-plain",
    "tdd",
    "muse",
];

/// Compute effective mode (axes + modifiers) from preset + CLI overrides.
///
/// Resolution order:
/// 1. Start with all axes `None` (no axes) and empty modifiers.
/// 2. If a known preset is given, merge its axis values and modifiers.
/// 3. CLI flag values override the preset for that axis.
/// 4. CLI modifiers are appended after preset modifiers, deduplicated
///    preserving order.
fn compute_mode(cli: &Cli) -> ModeConfig {
    let mut axes = presets::AxisValues::default();
    let preset_name = cli.preset.as_deref().unwrap_or("none");

    // Start with preset modifiers.
    let mut modifiers: Vec<String> = if preset_name != "none" {
        if let Some(preset) = presets::get_preset(preset_name) {
            axes.merge(&preset.axes);
            preset.modifiers
        } else {
            eprintln!("Warning: unknown preset '{preset_name}', using none");
            vec![]
        }
    } else {
        vec![]
    };

    // CLI axis overrides on a per-axis basis.
    if let Some(ref v) = cli.agency {
        axes.agency = Some(v.clone());
    }
    if let Some(ref v) = cli.quality {
        axes.quality = Some(v.clone());
    }
    if let Some(ref v) = cli.scope {
        axes.scope = Some(v.clone());
    }

    // CLI modifiers: append deduplicated, preserving order.
    let mut seen: HashSet<String> = modifiers.iter().cloned().collect();
    for m in &cli.modifier {
        if seen.insert(m.clone()) {
            modifiers.push(m.clone());
        }
    }
    if cli.readonly && seen.insert("readonly".to_string()) {
        modifiers.push("readonly".to_string());
    }
    if cli.context_pacing && seen.insert("context-pacing".to_string()) {
        modifiers.push("context-pacing".to_string());
    }

    // Validate modifier names.
    for m in &modifiers {
        if !VALID_MODIFIERS.contains(&m.as_str()) {
            eprintln!(
                "Error: unknown modifier '{m}'. Valid modifiers: {}",
                VALID_MODIFIERS.join(", ")
            );
            process::exit(1);
        }
    }

    ModeConfig { axes, modifiers }
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
