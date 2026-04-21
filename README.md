# updt

A cross-platform system update helper written in Rust.

`updt` checks upgrade candidates and lets you choose what to upgrade interactively.
It mirrors the behavior of the original script, but the workflow is implemented in Rust.

## Supported update targets

- Homebrew (macOS)
- npm global packages
- cargo installed crates (via `cargo-install-update`)
- rustup toolchains
- paru AUR packages (Arch Linux)
- flatpak apps (Arch Linux profile)
- pacman packages (Arch Linux)
- pkg packages (Termux)

## System policy

`updt` enables targets by OS profile.

- macOS: Homebrew, npm, cargo, rustup
- Arch Linux: npm, cargo, rustup, paru, pacman, flatpak
- Termux: pkg, npm, cargo
- other systems: all targets disabled by policy

## Install

```bash
cargo install updt
```

## Run

```bash
updt
```

The program does 3 stages.

1. Check upgrade candidates.
2. Select targets to upgrade.
3. Run upgrades for selected targets.

Check stage runs in parallel across enabled targets for faster detection.

Default selection is TUI:

- `Up` / `Down`: move cursor
- `Space`: toggle selected target
- `Enter`: confirm selection
- `q` / `Esc`: quit selection

## CLI

Show version:

```bash
updt --version
```

Update only selected targets (skip interactive selector):

```bash
updt update npm
updt update npm,cargo
updt update npm cargo
```

Install fish completion script to `~/.config/fish/completions/updt.fish`:

```bash
updt fish
```

Available subcommands:

- `update`
- `fish`

## Notes

- `cargo` checks require `cargo-install-update` in `PATH`.
- `pacman` upgrade uses `sudo pacman -Syu`.
- Termux `pkg` checks use `apt list --upgradable`.
- `updt` calls existing package manager commands and forwards their output.
