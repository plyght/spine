# Spine

The backbone of your package management ecosystem. Automatically detects and updates all installed package managers in parallel across \*nix systems.

## Overview

Spine serves as the central support structure that connects and coordinates all your package managers. Just as the spine supports the entire body, Spine supports all package managers by discovering them on your system and running their update workflows simultaneously with a clean TUI interface.

## Features

- **Universal Detection**: Auto-discovers 15+ package managers (Homebrew, APT, DNF, Pacman, Nix, Snap, Flatpak, etc.)
- **Parallel Execution**: Runs all workflows simultaneously for maximum efficiency
- **Interactive TUI**: Real-time progress monitoring with vim-style navigation
- **Cross-Platform**: Works across Linux, macOS, and BSD variants
- **Smart Sudo Handling**: Automatically handles privilege requirements per manager
- **Configurable**: Extensible via TOML configuration

## Installation

```bash
# From source
git clone https://github.com/yourusername/spine.git
cd spine
cargo build --release
sudo cp target/release/spn /usr/local/bin/

# Using Cargo
cargo install spine
```

## Usage

```bash
# List detected package managers
spn list

# Upgrade all package managers
spn upgrade
```

The TUI interface shows real-time status: Pending → Refreshing → Self-updating → Upgrading → Cleaning → Complete

Navigate with ↑↓/j/k, press Enter for details, 'q' to quit.

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

Configuration is searched in: current directory → binary directory → `/etc/spine/` → `/usr/local/etc/spine/`

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
