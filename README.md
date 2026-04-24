<div align='center'>
    <br/>
    <br/>
    <img src="https://github.com/user-attachments/assets/cc330a78-001a-42af-a778-ef4d69599407" alt="19a97d4a6a32afcd37b173d3d1e3ad02_upscayl_4x_upscayl-standard-4x" width="300"/>
    <br/>
    <br/>
    <h3>Spine</h3>
    <p>Automatically detects and updates all installed package managers in parallel across *nix systems</p>
    <br/>
    <br/>
</div>

The backbone of your package management ecosystem. Just as the spine supports the entire body, Spine supports all package managers by discovering them on your system and running their update workflows simultaneously with a clean TUI interface.

## Features

- **Universal Detection**: Auto-discovers 15+ package managers (Homebrew, APT, DNF, Pacman, Nix, Snap, Flatpak, etc.)
- **Parallel Execution**: Runs all workflows simultaneously for maximum efficiency
- **Interactive TUI**: Real-time progress monitoring with vim-style navigation
- **Cross-Platform**: Works across Linux, macOS, and BSD variants
- **Smart Sudo Handling**: Automatically handles privilege requirements per manager
- **Configurable**: Extensible via TOML configuration

## Install

```bash
# From source
git clone https://github.com/plyght/spine.git
cd spine
cargo build --release
sudo cp target/release/spn /usr/local/bin/

# Using Cargo
cargo install spine-pkgman
```

## Usage

```bash
# Open the TUI and upgrade all package managers
spn

# Run without the TUI
spn cli
spn --no-tui

# Preview detected managers and commands without running anything
spn --dry-run

# Check configuration, detection, sudo, and terminal readiness
spn doctor

# List detected package managers
spn list

# Backward-compatible explicit upgrade command
spn upgrade
```

The TUI interface shows real-time status: Pending → Refreshing → Self-updating → Upgrading → Cleaning → Complete

Managers that require administrator privileges are marked with a `sudo` badge. Spine asks for sudo once before opening the TUI when any detected manager needs it, then keeps that sudo session alive during long upgrade runs.

Navigate with ↑↓/j/k, press Enter for details, 'q' to quit. In selective mode, press Space to toggle managers and `r` to run the selected set.

## Configuration

Spine uses `backbone.toml` to define package manager commands:

```toml
[managers.brew]
name = "Homebrew"
check_command = "brew --version"
refresh = "brew update"
upgrade_all = "brew upgrade"
cleanup = "brew cleanup"
requires_sudo = false
```

Configuration is searched in: platform config directory (`~/.config/spine/backbone.toml` on many Linux systems, `~/Library/Application Support/spine/backbone.toml` on macOS) → current directory → home (`~/.spine/backbone.toml`) → binary directory → `/etc/spine/backbone.toml` → `/usr/local/etc/spine/backbone.toml`. Use `spn list` or `spn doctor` to see the active config path.

## Architecture

- `config.rs`: Configuration loading and parsing
- `detect.rs`: Package manager discovery
- `execute.rs`: Command execution with timeout/sudo handling
- `tui.rs`: Terminal interface using Ratatui
- `main.rs`: CLI orchestration

## Development

```bash
cargo build
cargo test
```

Requires Rust 1.70+. Key dependencies: clap, ratatui, crossterm, tokio, serde/toml.

## License

MIT License