# deepseek-tui-modes

Behavioral mode presets for [DeepSeek-TUI](https://github.com/drcomputer/deepseek-tui).  
Ports the *claude-code-modes* concept to the DeepSeek-TUI daemon API — replacing the
behavioral layer of the system prompt while keeping DeepSeek-TUI's infrastructure.

## How it works

The tool assembles a system prompt from modular fragments organized along three axes:

| Axis | Values | What it controls |
|------|--------|-----------------|
| **Agency** | `autonomous`, `collaborative`, `partner`, `surgical` | How independently the LLM acts |
| **Quality** | `architect`, `pragmatic`, `minimal` | Depth and rigor of the output |
| **Scope** | `unrestricted`, `adjacent`, `narrow` | How broadly the LLM changes things |

Eleven built-in presets combine these axes into ready-to-use behaviors.
You can also define custom presets, axes, and modifiers via config files.

The tool spawns a DeepSeek-TUI daemon, creates an operational thread with the
assembled prompt, and keeps it alive until you press Ctrl+C.

## Install

**Prerequisites:** Rust 1.80+, a `deepseek-tui` binary in your PATH (or at
`~/deepseek-tui-fork/target/release/deepseek-tui`).

```bash
git clone https://github.com/drcomputer/deepseek-tui-modes
cd deepseek-tui-modes
cargo build --release
```

The binary lands at `target/release/deepseek-tui-modes`.

## Usage

### Pick a preset

```bash
# Create mode: autonomous agent building unrestricted scope
deepseek-tui-modes create

# Explore mode: read-only agent on narrow scope
deepseek-tui-modes explore

# Safe mode: collaborative, minimal changes, narrow scope
deepseek-tui-modes safe
```

### Override individual axes

```bash
# Start from "none" but set agency explicitly
deepseek-tui-modes --agency surgical

# Extend preset with an override
deepseek-tui-modes extend --quality architect
```

### Add modifiers

```bash
# Read-only modifier with any preset
deepseek-tui-modes explore --readonly

# Stack modifiers
deepseek-tui-modes create --modifier bold --modifier tdd
```

### Preview the assembled prompt

```bash
deepseek-tui-modes methodical --print
```

### Manage config

```bash
deepseek-tui-modes config init
deepseek-tui-modes config add axis my-mode prompts/custom/my_mode.md
deepseek-tui-modes config add modifier my-mod prompts/custom/my_mod.md
deepseek-tui-modes config add preset my-preset --agency my-mode --quality pragmatic
```

## Built-in presets

| Preset | Agency | Quality | Scope | Modifier |
|--------|--------|---------|-------|----------|
| `none` | — | — | — | — |
| `safe` | collaborative | minimal | narrow | — |
| `create` | autonomous | architect | unrestricted | — |
| `extend` | autonomous | pragmatic | adjacent | — |
| `refactor` | autonomous | pragmatic | unrestricted | — |
| `explore` | collaborative | architect | narrow | `readonly` |
| `debug` | collaborative | pragmatic | narrow | `debug` |
| `methodical` | surgical | architect | narrow | `methodical` |
| `director` | collaborative | architect | unrestricted | `director` |
| `partner` | partner | pragmatic | adjacent | `speak-plain` |
| `muse` | autonomous | architect | unrestricted | `muse` |

## Project structure

```
prompts/            # Prompt fragments (base, axes, modifiers)
├── base/           # Core system prompt fragments
├── axis/           # Axis values (agency, quality, scope)
│   ├── agency/
│   ├── quality/
│   └── scope/
└── modifiers/      # Behavioral modifiers
src/
├── main.rs         # CLI entry point
├── presets.rs      # Preset definitions and axis values
├── resolve.rs      # CLI args → resolved paths
├── assemble.rs     # File assembly into a prompt string
├── config.rs       # Config file loading
├── config_cli.rs   # `config` subcommand
├── api.rs          # DeepSeek-TUI daemon API calls
└── daemon.rs       # Daemon lifecycle management
```

## License

MIT
