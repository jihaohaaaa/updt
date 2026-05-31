# updt

[![Crates.io](https://img.shields.io/crates/v/updt.svg)](https://crates.io/crates/updt)
[![Docs.rs](https://docs.rs/updt/badge.svg)](https://docs.rs/updt)
[![CI](https://github.com/jihaohaaaa/updt/actions/workflows/ci.yml/badge.svg)](https://github.com/jihaohaaaa/updt/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`updt` is a cross-platform system update helper written in Rust. It checks upgrade candidates across supported package managers, presents an interactive selector, and then runs the selected upgrades with the original package manager commands.

It is designed for people who regularly maintain the same development tools across macOS, Windows, Arch Linux, and Termux, and want one small command to answer: what can be upgraded now?

## Features

- Parallel checks across enabled targets for faster detection.
- Interactive terminal UI for selecting which targets to upgrade.
- Direct non-interactive updates with explicit target names.
- OS profile policy that only enables targets that make sense for the current platform.
- Fish shell completion installer.
- Uses existing package manager commands and forwards their output.

## Supported Targets

| Target | What it updates | Notes |
| --- | --- | --- |
| `brew` | Homebrew formulae and casks | macOS profile |
| `npm` | npm global packages | Requires `npm` |
| `cargo` | cargo-installed crates | Requires `cargo-install-update` |
| `nvim` | Neovim Lazy and Mason components | Requires `nvim` |
| `rustup` | Rust toolchains | Requires `rustup` |
| `fnm` | fnm-managed Node.js versions | Checks latest and LTS versions |
| `scoop` | Scoop packages | Windows profile |
| `paru` | AUR packages | Arch Linux profile |
| `flatpak` | Flatpak apps | Arch Linux profile |
| `pacman` | pacman packages | Arch Linux profile |
| `pkg` | Termux packages | Termux profile |

Windows system package managers `winget` and `choco` are intentionally unsupported.

## System Policy

`updt` enables targets by OS profile:

| Profile | Enabled targets |
| --- | --- |
| macOS | `brew`, `npm`, `cargo`, `nvim`, `rustup`, `fnm` |
| Windows | `npm`, `cargo`, `nvim`, `rustup`, `fnm`, `scoop` |
| Arch Linux | `npm`, `cargo`, `nvim`, `rustup`, `fnm`, `paru`, `pacman`, `flatpak` |
| Termux | `pkg`, `npm`, `cargo`, `nvim`, `fnm` |
| Other systems | `nvim` only |

Unsupported or missing commands are skipped during checks.

## Install

Install the latest release from crates.io:

```bash
cargo install updt
```

For cargo package checks, install `cargo-update` so `cargo-install-update` is available:

```bash
cargo install cargo-update
```

## Usage

Run the interactive flow:

```bash
updt
```

The default flow has three stages:

1. Check upgrade candidates.
2. Select targets to upgrade.
3. Run upgrades for selected targets.

Interactive selector controls:

| Key | Action |
| --- | --- |
| `Up` / `Down` | Move cursor |
| `Space` | Toggle selected target |
| `Enter` | Confirm selection |
| `q` / `Esc` | Quit selection |

Show the installed version:

```bash
updt --version
```

Update selected targets without the interactive selector:

```bash
updt update npm
updt update npm,cargo
updt update npm cargo
```

Install fish completion to `~/.config/fish/completions/updt.fish`:

```bash
updt fish
```

## Behavior Notes

- `cargo` checks require `cargo-install-update` in `PATH`.
- `pacman` upgrade uses `sudo pacman -Syu` when the terminal is focused, or `pkexec pacman -Syu` on desktop Linux when the terminal is not focused.
- Termux `pkg` checks use `apt list --upgradable`.
- Neovim checks run headless and look for Lazy and Mason update availability.
- `updt` does not replace package managers; it orchestrates the installed tools already on the system.

## Development

Build the project:

```bash
cargo build
```

Run tests:

```bash
cargo test
```

Run formatting and lint checks before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Contributing

Issues and pull requests are welcome. Please include the operating system, the target package manager, and the command output when reporting an update detection or upgrade problem.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow and [SECURITY.md](SECURITY.md) for responsible vulnerability reporting.

## License

This project is licensed under the [MIT License](LICENSE).
