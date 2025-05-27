# Spine

A universal meta package manager for *nix systems that automatically detects and updates all installed package managers in parallel.

## Overview

Spine is a Rust-based command-line tool that serves as the **backbone** of your package management ecosystem. Just as the spine supports the entire body, Spine supports all your package managers by automatically discovering them on your *nix system and running their update workflows in parallel. It provides a terminal user interface (TUI) to monitor progress and handles the complexity of different package manager syntaxes and requirements.

The name "Spine" reflects its role as the central support structure that connects and coordinates all your package managers, providing a unified interface to keep your entire system up to date.

## Features

- **Universal Detection**: Automatically detects 15+ package managers including Homebrew, APT, DNF, Pacman, Nix, Snap, Flatpak, and more
- **Parallel Execution**: Runs all package manager workflows simultaneously for maximum efficiency
- **Interactive TUI**: Real-time progress monitoring with a clean terminal interface
- **Cross-Platform**: Works across all *nix systems including Linux, macOS, and BSD variants
- **Smart Sudo Handling**: Automatically handles sudo requirements per package manager
- **Configurable**: Extensible configuration system via TOML files
- **Robust Error Handling**: Graceful failure handling with detailed error reporting
- **Comprehensive Workflow**: Handles refresh, self-update, upgrade, and cleanup operations

## Supported Package Managers

| Package Manager | *nix Systems | Sudo Required |
|----------------|---------------|---------------|
| Homebrew | macOS, Linux | No |
| APT | Debian, Ubuntu, derivatives | Yes |
| DNF | Fedora, CentOS Stream | Yes |
| YUM | RHEL, CentOS, derivatives | Yes |
| Pacman | Arch Linux, Manjaro | Yes |
| Zypper | openSUSE, SLES | Yes |
| Portage | Gentoo, derivatives | Yes |
| Nix | NixOS, macOS, Linux | No |
| Snap | Ubuntu, various Linux | Yes |
| Flatpak | Most Linux distributions | No |
| MacPorts | macOS | Yes |
| FreeBSD pkg | FreeBSD, derivatives | Yes |
| Alpine apk | Alpine Linux | Yes |
| XBPS | Void Linux | Yes |

## Installation

### From Source

```bash
git clone https://github.com/yourusername/spine.git
cd spine
cargo build --release
sudo cp target/release/spn /usr/local/bin/
sudo cp backbone.toml /usr/local/etc/spine/
```

### Using Cargo

```bash
cargo install spine
```

## Usage

### Basic Commands

```bash
# List detected package managers
spn list

# Upgrade all package managers
spn upgrade
```

### Example Output

```
$ spn list
Detected 3 package manager(s):
  ✓ brew (Homebrew)
    Check command: brew --version
    Requires sudo: false

  ✓ snap (Snap)
    Check command: snap version
    Requires sudo: true

  ✓ flatpak (Flatpak)
    Check command: flatpak --version
    Requires sudo: false
```

### TUI Interface

When running `spn upgrade`, Spine presents an interactive terminal interface:

- **Navigation**: Use ↑↓ or j/k to navigate between package managers
- **Details**: Press Enter to view detailed configuration for a package manager
- **Exit**: Press 'q' to quit or Esc to go back from detail view

The interface shows real-time status updates:
- **Pending**: Waiting to start
- **Refreshing**: Updating package lists/repositories
- **Self-updating**: Updating the package manager itself
- **Upgrading**: Installing package updates
- **Cleaning**: Removing unnecessary files
- **✓ Complete**: Successfully finished
- **✗ Failed**: Encountered an error

## Configuration

Spine uses a `backbone.toml` configuration file that defines package manager commands and behaviors. The file is searched in these locations:

1. Current working directory
2. Same directory as the binary
3. `/etc/spine/backbone.toml`
4. `/usr/local/etc/spine/backbone.toml`

### Configuration Format

```toml
[managers.brew]
name = "Homebrew"
check_command = "brew --version"
refresh = "brew update"
self_update = "brew upgrade brew"
upgrade_all = "brew upgrade"
cleanup = "brew cleanup"
requires_sudo = false

[managers.apt]
name = "APT"
check_command = "apt --version"
refresh = "apt update"
upgrade_all = "apt upgrade -y"
cleanup = "apt autoremove -y && apt autoclean"
requires_sudo = true
```

### Adding Custom Package Managers

To add support for a new package manager, add a new section to `backbone.toml`:

```toml
[managers.custom]
name = "Custom Package Manager"
check_command = "custom --version"
refresh = "custom update"           # Optional
self_update = "custom self-update"  # Optional
upgrade_all = "custom upgrade -y"   # Required
cleanup = "custom clean"            # Optional
requires_sudo = false
```

## Architecture

Spine is built with several key components:

- **Config Module** (`src/config.rs`): Handles loading and parsing of configuration files
- **Detection Module** (`src/detect.rs`): Discovers available package managers on the system
- **Execution Module** (`src/execute.rs`): Manages command execution with timeout and sudo handling
- **TUI Module** (`src/tui.rs`): Provides the terminal user interface using Ratatui
- **Main Module** (`src/main.rs`): CLI argument parsing and application orchestration

### Workflow

As the backbone of your package management system, Spine orchestrates the following workflow:

1. Load configuration from `backbone.toml`
2. Detect available package managers by checking their existence across your *nix system
3. Launch parallel tasks for each detected manager
4. Execute the complete workflow: refresh → self-update → upgrade → cleanup
5. Display real-time progress in the TUI
6. Present a comprehensive summary of results

This unified approach ensures all your package managers work in harmony, just like how the spine coordinates different parts of the body.

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Development Dependencies

- Rust 1.70+
- Cargo

### Key Dependencies

- **clap**: Command-line argument parsing
- **ratatui**: Terminal user interface framework
- **crossterm**: Cross-platform terminal manipulation
- **tokio**: Async runtime
- **serde/toml**: Configuration parsing
- **anyhow**: Error handling
- **which**: Command detection

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

### Adding Package Manager Support

To add support for a new package manager:

1. Add its configuration to `backbone.toml`
2. Test detection and execution on the target system
3. Update the README with the new package manager

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Security Considerations

- Spine runs package manager commands that may require sudo privileges
- Commands are executed with timeouts to prevent hanging
- Sudo availability is checked before execution
- Commands are validated before execution to prevent injection

## Troubleshooting

### Common Issues

**Configuration file not found**
```
Error loading configuration: backbone.toml configuration file not found.
```
Ensure `backbone.toml` is in the current directory or installed system-wide.

**Sudo privileges required**
```
Warning: Some package managers require sudo access.
```
Run with appropriate sudo privileges or ensure your user has the necessary permissions.

**Package manager not detected**
```
No package managers detected on this system.
```
Verify that the package managers are installed and in your PATH on your *nix system.

### Debug Mode

Run with increased verbosity:
```bash
RUST_LOG=debug spn upgrade
```

## Roadmap

- [ ] Package manager-specific configuration overrides
- [ ] Plugin system for custom package managers
- [ ] Web dashboard for remote monitoring
- [ ] Package installation tracking
- [ ] Scheduled execution support
- [ ] Docker container support