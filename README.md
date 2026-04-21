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

## System policy

`updt` enables targets by OS profile.

- macOS: Homebrew, npm, cargo, rustup
- Arch Linux: npm, cargo, rustup, paru, pacman, flatpak
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
2. Ask for per-target confirmation.
3. Run upgrades for selected targets.

## Notes

- `cargo` checks require `cargo-install-update` in `PATH`.
- `pacman` upgrade uses `sudo pacman -Syu`.
- `updt` calls existing package manager commands and forwards their output.
