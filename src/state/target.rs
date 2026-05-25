use super::{AppState, TargetStateFlags};

pub const TARGET_IDS: [&str; 11] = [
    "brew", "npm", "cargo", "nvim", "rustup", "fnm", "scoop", "paru", "flatpak", "pacman", "pkg",
];

struct TargetMeta {
    id: &'static str,
    label: &'static str,
    section: &'static str,
    update_summary: &'static str,
    flags: fn(&AppState) -> TargetStateFlags,
    items: fn(&AppState) -> Vec<String>,
}

const TARGET_META: [TargetMeta; 11] = [
    TargetMeta {
        id: "brew",
        label: "Homebrew",
        section: "Homebrew",
        update_summary: "发现可升级项",
        flags: brew_flags,
        items: brew_items,
    },
    TargetMeta {
        id: "npm",
        label: "npm",
        section: "npm (global)",
        update_summary: "发现可升级项",
        flags: npm_flags,
        items: npm_items,
    },
    TargetMeta {
        id: "cargo",
        label: "cargo",
        section: "cargo",
        update_summary: "发现可升级项",
        flags: cargo_flags,
        items: cargo_items,
    },
    TargetMeta {
        id: "nvim",
        label: "Neovim",
        section: "Neovim (Lazy/Mason)",
        update_summary: "可执行更新",
        flags: nvim_flags,
        items: nvim_items,
    },
    TargetMeta {
        id: "rustup",
        label: "rustup",
        section: "rustup",
        update_summary: "发现可升级项",
        flags: rustup_flags,
        items: rustup_items,
    },
    TargetMeta {
        id: "fnm",
        label: "fnm",
        section: "fnm (Node.js runtime)",
        update_summary: "发现可升级项",
        flags: fnm_flags,
        items: fnm_items,
    },
    TargetMeta {
        id: "scoop",
        label: "scoop",
        section: "scoop",
        update_summary: "发现可升级项",
        flags: scoop_flags,
        items: scoop_items,
    },
    TargetMeta {
        id: "paru",
        label: "paru",
        section: "paru (AUR)",
        update_summary: "发现可升级项",
        flags: paru_flags,
        items: paru_items,
    },
    TargetMeta {
        id: "flatpak",
        label: "flatpak",
        section: "flatpak",
        update_summary: "发现可升级项",
        flags: flatpak_flags,
        items: flatpak_items,
    },
    TargetMeta {
        id: "pacman",
        label: "pacman",
        section: "pacman",
        update_summary: "发现可升级项",
        flags: pacman_flags,
        items: pacman_items,
    },
    TargetMeta {
        id: "pkg",
        label: "pkg",
        section: "pkg (Termux)",
        update_summary: "发现可升级项",
        flags: pkg_flags,
        items: pkg_items,
    },
];

fn target_meta(id: &str) -> Option<&'static TargetMeta> {
    TARGET_META.iter().find(|meta| meta.id == id)
}

pub fn target_label(id: &str) -> &'static str {
    target_meta(id).map(|m| m.label).unwrap_or("unknown")
}

pub fn section_title(target: &str) -> &'static str {
    target_meta(target).map(|m| m.section).unwrap_or("unknown")
}

pub fn target_update_summary(target: &str) -> &'static str {
    target_meta(target)
        .map(|m| m.update_summary)
        .unwrap_or("发现可升级项")
}

pub fn target_state_flags(state: &AppState, target: &str) -> Option<TargetStateFlags> {
    target_meta(target).map(|meta| (meta.flags)(state))
}

pub fn target_enabled(state: &AppState, target: &str) -> bool {
    target_state_flags(state, target)
        .map(|flags| flags.enabled)
        .unwrap_or(false)
}

pub fn updatable_items_for_target(state: &AppState, target: &str) -> Vec<String> {
    target_meta(target)
        .map(|meta| (meta.items)(state))
        .unwrap_or_default()
}

fn bucket_flags(
    enabled: bool,
    installed: bool,
    check_failed: bool,
    has_updates: bool,
) -> TargetStateFlags {
    TargetStateFlags {
        enabled,
        installed,
        check_failed,
        has_updates,
        needs_cargo_updater: false,
    }
}

fn brew_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.brew,
        state.brew.installed,
        state.brew.check_failed,
        state.brew.has_updates,
    )
}

fn npm_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.npm,
        state.npm.installed,
        state.npm.check_failed,
        state.npm.has_updates,
    )
}

fn cargo_flags(state: &AppState) -> TargetStateFlags {
    TargetStateFlags {
        enabled: state.enable.cargo,
        installed: state.cargo.installed,
        check_failed: state.cargo.check_failed,
        has_updates: state.cargo.has_updates,
        needs_cargo_updater: !state.cargo.updater_installed,
    }
}

fn nvim_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.nvim,
        state.nvim.installed,
        state.nvim.check_failed,
        state.nvim.has_updates,
    )
}

fn rustup_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.rustup,
        state.rustup.installed,
        state.rustup.check_failed,
        state.rustup.has_updates,
    )
}

fn fnm_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.fnm,
        state.fnm.installed,
        state.fnm.check_failed,
        state.fnm.has_updates,
    )
}

fn scoop_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.scoop,
        state.scoop.installed,
        state.scoop.check_failed,
        state.scoop.has_updates,
    )
}

fn paru_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.paru,
        state.paru.installed,
        state.paru.check_failed,
        state.paru.has_updates,
    )
}

fn flatpak_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.flatpak,
        state.flatpak.installed,
        state.flatpak.check_failed,
        state.flatpak.has_updates,
    )
}

fn pacman_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.pacman,
        state.pacman.installed,
        state.pacman.check_failed,
        state.pacman.has_updates,
    )
}

fn pkg_flags(state: &AppState) -> TargetStateFlags {
    bucket_flags(
        state.enable.pkg,
        state.pkg.installed,
        state.pkg.check_failed,
        state.pkg.has_updates,
    )
}

fn brew_items(state: &AppState) -> Vec<String> {
    state
        .brew
        .formula_list
        .iter()
        .chain(state.brew.cask_list.iter())
        .cloned()
        .collect()
}

fn npm_items(state: &AppState) -> Vec<String> {
    state.npm.updatable_items.clone()
}

fn cargo_items(state: &AppState) -> Vec<String> {
    state.cargo.updatable_packages.clone()
}

fn nvim_items(state: &AppState) -> Vec<String> {
    state.nvim.updatable_components.clone()
}

fn rustup_items(state: &AppState) -> Vec<String> {
    state.rustup.updatable_items.clone()
}

fn fnm_items(state: &AppState) -> Vec<String> {
    state.fnm.updatable_items.clone()
}

fn scoop_items(state: &AppState) -> Vec<String> {
    state.scoop.updatable_items.clone()
}

fn paru_items(state: &AppState) -> Vec<String> {
    state.paru.updatable_items.clone()
}

fn flatpak_items(state: &AppState) -> Vec<String> {
    state.flatpak.updatable_items.clone()
}

fn pacman_items(state: &AppState) -> Vec<String> {
    state.pacman.updatable_items.clone()
}

fn pkg_items(state: &AppState) -> Vec<String> {
    state.pkg.updatable_items.clone()
}
