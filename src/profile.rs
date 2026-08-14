use std::env;
use std::io::{self, IsTerminal};
use tokio::fs;

use crate::state::{AppState, SystemProfile};

pub async fn parse_profile(state: &mut AppState) {
    let prefix = env::var("PREFIX").unwrap_or_default();
    state.is_termux = prefix.contains("com.termux")
        || fs::metadata("/data/data/com.termux/files/usr/bin/pkg")
            .await
            .is_ok();
    state.is_arch_linux = fs::metadata("/etc/arch-release").await.is_ok();
    apply_profile_policy(state, detect_profile(state.is_termux, state.is_arch_linux));
}

struct ProfileRule {
    profile: SystemProfile,
    matches: fn(bool, bool) -> bool,
    apply: fn(&mut AppState),
}

const PROFILE_RULES: &[ProfileRule] = &[
    ProfileRule {
        profile: SystemProfile::Termux,
        matches: is_termux_profile,
        apply: apply_termux_policy,
    },
    ProfileRule {
        profile: SystemProfile::Windows,
        matches: is_windows_profile,
        apply: apply_windows_policy,
    },
    ProfileRule {
        profile: SystemProfile::Macos,
        matches: is_macos_profile,
        apply: apply_macos_policy,
    },
    ProfileRule {
        profile: SystemProfile::Arch,
        matches: is_arch_profile,
        apply: apply_arch_policy,
    },
];

fn detect_profile(is_termux: bool, is_arch_linux: bool) -> SystemProfile {
    PROFILE_RULES
        .iter()
        .find(|rule| (rule.matches)(is_termux, is_arch_linux))
        .map(|rule| rule.profile)
        .unwrap_or_default()
}

fn apply_profile_policy(state: &mut AppState, profile: SystemProfile) {
    state.system_profile = profile;
    if let Some(rule) = PROFILE_RULES.iter().find(|rule| rule.profile == profile) {
        (rule.apply)(state);
    }
}

fn is_termux_profile(is_termux: bool, _is_arch_linux: bool) -> bool {
    is_termux
}

fn is_windows_profile(is_termux: bool, _is_arch_linux: bool) -> bool {
    !is_termux && env::consts::OS == "windows"
}

fn is_macos_profile(is_termux: bool, _is_arch_linux: bool) -> bool {
    !is_termux && env::consts::OS == "macos"
}

fn is_arch_profile(is_termux: bool, is_arch_linux: bool) -> bool {
    !is_termux && is_arch_linux
}

fn apply_termux_policy(state: &mut AppState) {
    state.enable.pkg = true;
    state.enable.npm = true;
    state.enable.cargo = true;
    state.enable.rustup = false;
}

fn apply_windows_policy(state: &mut AppState) {
    state.enable.npm = true;
    state.enable.cargo = true;
    state.enable.rustup = true;
    state.enable.scoop = true;
}

fn apply_macos_policy(state: &mut AppState) {
    state.enable.brew = true;
    state.enable.npm = true;
    state.enable.cargo = true;
    state.enable.rustup = true;
}

fn apply_arch_policy(state: &mut AppState) {
    state.enable.npm = true;
    state.enable.cargo = true;
    state.enable.rustup = true;
    state.enable.paru = true;
    state.enable.pacman = true;
    state.enable.flatpak = true;
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
