#[derive(Clone, Copy)]
pub struct TargetStateFlags {
    pub enabled: bool,
    pub installed: bool,
    pub check_failed: bool,
    pub has_updates: bool,
    pub needs_cargo_updater: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemProfile {
    #[default]
    Unknown,
    Windows,
    Macos,
    Arch,
    Termux,
}

#[derive(Default, Clone)]
pub struct TargetBucketState {
    pub installed: bool,
    pub has_updates: bool,
    pub check_failed: bool,
    pub updatable_items: Vec<String>,
}

#[derive(Default, Clone)]
pub struct BrewState {
    pub installed: bool,
    pub has_updates: bool,
    pub check_failed: bool,
    pub formula_list: Vec<String>,
    pub cask_list: Vec<String>,
}

#[derive(Default, Clone)]
pub struct CargoState {
    pub installed: bool,
    pub has_updates: bool,
    pub check_failed: bool,
    pub updater_installed: bool,
    pub updatable_packages: Vec<String>,
}

#[derive(Default, Clone)]
pub struct NvimState {
    pub installed: bool,
    pub has_updates: bool,
    pub check_failed: bool,
    pub lazy_available: bool,
    pub mason_available: bool,
    pub updatable_components: Vec<String>,
}

#[derive(Default, Clone)]
pub struct EnablePolicy {
    pub brew: bool,
    pub npm: bool,
    pub cargo: bool,
    pub rustup: bool,
    pub fnm: bool,
    pub scoop: bool,
    pub paru: bool,
    pub pacman: bool,
    pub flatpak: bool,
    pub pkg: bool,
    pub nvim: bool,
}

#[derive(Default, Clone)]
pub struct AppState {
    pub brew: BrewState,
    pub npm: TargetBucketState,
    pub cargo: CargoState,
    pub rustup: TargetBucketState,
    pub fnm: TargetBucketState,
    pub scoop: TargetBucketState,
    pub paru: TargetBucketState,
    pub flatpak: TargetBucketState,
    pub pacman: TargetBucketState,
    pub pkg: TargetBucketState,
    pub nvim: NvimState,
    pub is_arch_linux: bool,
    pub is_termux: bool,
    pub system_profile: SystemProfile,
    pub enable: EnablePolicy,
}

pub const TARGET_IDS: [&str; 11] = [
    "brew", "npm", "cargo", "nvim", "rustup", "fnm", "scoop", "paru", "flatpak", "pacman", "pkg",
];

struct TargetMeta {
    id: &'static str,
    label: &'static str,
    section: &'static str,
    update_summary: &'static str,
}

const TARGET_META: [TargetMeta; 11] = [
    TargetMeta {
        id: "brew",
        label: "Homebrew",
        section: "Homebrew",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "npm",
        label: "npm",
        section: "npm (global)",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "cargo",
        label: "cargo",
        section: "cargo",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "nvim",
        label: "Neovim",
        section: "Neovim (Lazy/Mason)",
        update_summary: "可执行更新",
    },
    TargetMeta {
        id: "rustup",
        label: "rustup",
        section: "rustup",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "fnm",
        label: "fnm",
        section: "fnm (Node.js runtime)",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "scoop",
        label: "scoop",
        section: "scoop",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "paru",
        label: "paru",
        section: "paru (AUR)",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "flatpak",
        label: "flatpak",
        section: "flatpak",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "pacman",
        label: "pacman",
        section: "pacman",
        update_summary: "发现可升级项",
    },
    TargetMeta {
        id: "pkg",
        label: "pkg",
        section: "pkg (Termux)",
        update_summary: "发现可升级项",
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
    match target {
        "brew" => Some(TargetStateFlags {
            enabled: state.enable.brew,
            installed: state.brew.installed,
            check_failed: state.brew.check_failed,
            has_updates: state.brew.has_updates,
            needs_cargo_updater: false,
        }),
        "npm" => Some(TargetStateFlags {
            enabled: state.enable.npm,
            installed: state.npm.installed,
            check_failed: state.npm.check_failed,
            has_updates: state.npm.has_updates,
            needs_cargo_updater: false,
        }),
        "cargo" => Some(TargetStateFlags {
            enabled: state.enable.cargo,
            installed: state.cargo.installed,
            check_failed: state.cargo.check_failed,
            has_updates: state.cargo.has_updates,
            needs_cargo_updater: !state.cargo.updater_installed,
        }),
        "nvim" => Some(TargetStateFlags {
            enabled: state.enable.nvim,
            installed: state.nvim.installed,
            check_failed: state.nvim.check_failed,
            has_updates: state.nvim.has_updates,
            needs_cargo_updater: false,
        }),
        "rustup" => Some(TargetStateFlags {
            enabled: state.enable.rustup,
            installed: state.rustup.installed,
            check_failed: state.rustup.check_failed,
            has_updates: state.rustup.has_updates,
            needs_cargo_updater: false,
        }),
        "fnm" => Some(TargetStateFlags {
            enabled: state.enable.fnm,
            installed: state.fnm.installed,
            check_failed: state.fnm.check_failed,
            has_updates: state.fnm.has_updates,
            needs_cargo_updater: false,
        }),
        "scoop" => Some(TargetStateFlags {
            enabled: state.enable.scoop,
            installed: state.scoop.installed,
            check_failed: state.scoop.check_failed,
            has_updates: state.scoop.has_updates,
            needs_cargo_updater: false,
        }),
        "paru" => Some(TargetStateFlags {
            enabled: state.enable.paru,
            installed: state.paru.installed,
            check_failed: state.paru.check_failed,
            has_updates: state.paru.has_updates,
            needs_cargo_updater: false,
        }),
        "flatpak" => Some(TargetStateFlags {
            enabled: state.enable.flatpak,
            installed: state.flatpak.installed,
            check_failed: state.flatpak.check_failed,
            has_updates: state.flatpak.has_updates,
            needs_cargo_updater: false,
        }),
        "pacman" => Some(TargetStateFlags {
            enabled: state.enable.pacman,
            installed: state.pacman.installed,
            check_failed: state.pacman.check_failed,
            has_updates: state.pacman.has_updates,
            needs_cargo_updater: false,
        }),
        "pkg" => Some(TargetStateFlags {
            enabled: state.enable.pkg,
            installed: state.pkg.installed,
            check_failed: state.pkg.check_failed,
            has_updates: state.pkg.has_updates,
            needs_cargo_updater: false,
        }),
        _ => None,
    }
}

pub fn profile_name(profile: SystemProfile) -> &'static str {
    match profile {
        SystemProfile::Unknown => "unknown",
        SystemProfile::Windows => "windows",
        SystemProfile::Macos => "macos",
        SystemProfile::Arch => "arch",
        SystemProfile::Termux => "termux",
    }
}

pub fn target_enabled(state: &AppState, target: &str) -> bool {
    target_state_flags(state, target)
        .map(|flags| flags.enabled)
        .unwrap_or(false)
}

pub fn updatable_items_for_target(state: &AppState, target: &str) -> Vec<String> {
    match target {
        "brew" => state
            .brew
            .formula_list
            .iter()
            .chain(state.brew.cask_list.iter())
            .cloned()
            .collect(),
        "npm" => state.npm.updatable_items.clone(),
        "cargo" => state.cargo.updatable_packages.clone(),
        "nvim" => state.nvim.updatable_components.clone(),
        "rustup" => state.rustup.updatable_items.clone(),
        "fnm" => state.fnm.updatable_items.clone(),
        "scoop" => state.scoop.updatable_items.clone(),
        "paru" => state.paru.updatable_items.clone(),
        "flatpak" => state.flatpak.updatable_items.clone(),
        "pacman" => state.pacman.updatable_items.clone(),
        "pkg" => state.pkg.updatable_items.clone(),
        _ => Vec::new(),
    }
}
