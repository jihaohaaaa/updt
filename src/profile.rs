use std::env;
use std::io::{self, IsTerminal};
use tokio::fs;

use crate::state::{AppState, SystemProfile};

pub async fn parse_profile(state: &mut AppState) {
    let prefix = env::var("PREFIX").unwrap_or_default();
    state.enable.nvim = true;
    state.is_termux = prefix.contains("com.termux")
        || fs::metadata("/data/data/com.termux/files/usr/bin/pkg")
            .await
            .is_ok();
    state.is_arch_linux = fs::metadata("/etc/arch-release").await.is_ok();
    if state.is_termux {
        state.system_profile = SystemProfile::Termux;
        state.enable.pkg = true;
        state.enable.npm = true;
        state.enable.cargo = true;
        state.enable.fnm = true;
        state.enable.rustup = false;
    } else if env::consts::OS == "windows" {
        state.system_profile = SystemProfile::Windows;
        state.enable.npm = true;
        state.enable.cargo = true;
        state.enable.rustup = true;
        state.enable.fnm = true;
        state.enable.scoop = true;
    } else if env::consts::OS == "macos" {
        state.system_profile = SystemProfile::Macos;
        state.enable.brew = true;
        state.enable.npm = true;
        state.enable.cargo = true;
        state.enable.rustup = true;
        state.enable.fnm = true;
    } else if state.is_arch_linux {
        state.system_profile = SystemProfile::Arch;
        state.enable.npm = true;
        state.enable.cargo = true;
        state.enable.rustup = true;
        state.enable.fnm = true;
        state.enable.paru = true;
        state.enable.pacman = true;
        state.enable.flatpak = true;
    }
}

pub fn interactive_terminal() -> bool {
    io::stdout().is_terminal() && io::stdin().is_terminal()
}

pub fn desktop_linux_session() -> bool {
    env::consts::OS == "linux"
        && [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "DESKTOP_SESSION",
        ]
        .iter()
        .any(|key| env::var_os(key).is_some_and(|value| !value.is_empty()))
}
