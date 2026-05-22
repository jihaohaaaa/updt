use serde_json::Value;

use crate::command::{
    command_exists, run_capture, run_cargo_install_update_capture, run_nvim_headless_capture,
};
use crate::output::{MsgKind, log_pkg_line};
use crate::parse::{
    extract_marker_count, first_json_payload, first_token, parse_cargo_list,
    parse_fnm_version_token, parse_scoop_status_output,
};
use crate::profile::parse_profile;
use crate::state::AppState;

pub struct CheckResult {
    pub target: String,
    pub state: AppState,
    pub logs: Vec<String>,
}

async fn check_brew_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.brew {
        logs.push(log_pkg_line("brew", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("brew").await {
        logs.push(log_pkg_line("brew", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.brew.installed = true;
    logs.push(log_pkg_line(
        "brew",
        "正在检查可升级项 (brew outdated --greedy --json=v2)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("brew", &["outdated", "--greedy", "--json=v2"]).await
    else {
        state.brew.check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: 无法执行 brew 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.brew.check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            &format!("检查失败 (brew outdated --greedy --json=v2, exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Some(json_text) = first_json_payload(&output) else {
        state.brew.check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: 未找到 JSON 内容.",
            MsgKind::Warn,
        ));
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        state.brew.check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: JSON 解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    state.brew.formula_list = root
        .get("formulae")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();
    state.brew.cask_list = root
        .get("casks")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();
    if state.brew.formula_list.is_empty() {
        logs.push(log_pkg_line("brew", "Formula: 已是最新.", MsgKind::Ok));
    } else {
        state.brew.has_updates = true;
        logs.push(log_pkg_line("brew", "Formula 可升级:", MsgKind::Info));
        for p in &state.brew.formula_list {
            logs.push(format!("  - {p}"));
        }
    }
    if state.brew.cask_list.is_empty() {
        logs.push(log_pkg_line("brew", "Cask: 已是最新.", MsgKind::Ok));
    } else {
        state.brew.has_updates = true;
        logs.push(log_pkg_line("brew", "Cask 可升级:", MsgKind::Info));
        for p in &state.brew.cask_list {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_npm_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.npm {
        logs.push(log_pkg_line("npm", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("npm").await {
        logs.push(log_pkg_line("npm", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.npm.installed = true;
    logs.push(log_pkg_line(
        "npm",
        "正在检查全局包更新 (npm outdated --json --global)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("npm", &["outdated", "--json", "--global"]).await else {
        state.npm.check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            "检查失败: 无法执行 npm 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status == 0 {
        logs.push(log_pkg_line("npm", "全局包已是最新.", MsgKind::Ok));
        return;
    }
    if status != 1 {
        state.npm.check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Some(json_text) = first_json_payload(&output) else {
        state.npm.check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            "检查失败: JSON 解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        state.npm.check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            "检查失败: JSON 解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    let Some(obj) = root.as_object() else {
        logs.push(log_pkg_line("npm", "全局包已是最新.", MsgKind::Ok));
        return;
    };
    if obj.is_empty() {
        logs.push(log_pkg_line("npm", "全局包已是最新.", MsgKind::Ok));
        return;
    }
    state.npm.has_updates = true;
    state.npm.updatable_items = obj.keys().cloned().collect();
    logs.push(log_pkg_line("npm", "以下全局包可升级:", MsgKind::Info));
    for name in &state.npm.updatable_items {
        logs.push(format!("  - {name}"));
    }
}

pub async fn check_cargo_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.cargo {
        logs.push(log_pkg_line("cargo", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("cargo").await {
        logs.push(log_pkg_line("cargo", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.cargo.installed = true;
    if !command_exists("cargo-install-update").await {
        logs.push(log_pkg_line(
            "cargo",
            "未安装 cargo-install-update, 跳过.",
            MsgKind::Warn,
        ));
        return;
    }
    state.cargo.updater_installed = true;
    logs.push(log_pkg_line(
        "cargo",
        "正在检查已安装 crate 更新 (cargo install-update --locked --list)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_cargo_install_update_capture(&["--list"]).await else {
        state.cargo.check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            "检查失败: 命令执行失败.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.cargo.check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Ok(pkgs) = parse_cargo_list(&output) else {
        state.cargo.check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            "检查失败: 输出解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    state.cargo.updatable_packages = pkgs;
    if state.cargo.updatable_packages.is_empty() {
        logs.push(log_pkg_line("cargo", "已安装 crate 已是最新.", MsgKind::Ok));
    } else {
        state.cargo.has_updates = true;
        logs.push(log_pkg_line("cargo", "以下 crate 可升级:", MsgKind::Info));
        for p in &state.cargo.updatable_packages {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_rustup_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.rustup {
        logs.push(log_pkg_line("rustup", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("rustup").await {
        logs.push(log_pkg_line("rustup", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.rustup.installed = true;
    logs.push(log_pkg_line(
        "rustup",
        "正在检查 toolchain 更新 (rustup check --no-self-update)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("rustup", &["check", "--no-self-update"]).await else {
        state.rustup.check_failed = true;
        logs.push(log_pkg_line(
            "rustup",
            "检查失败: 无法执行 rustup 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    match status {
        0 => logs.push(log_pkg_line("rustup", "toolchain 已是最新.", MsgKind::Ok)),
        100 => {
            state.rustup.has_updates = true;
            logs.push(log_pkg_line(
                "rustup",
                "以下 toolchain 可升级:",
                MsgKind::Info,
            ));
            for line in output.lines().map(str::trim).filter(|x| !x.is_empty()) {
                state.rustup.updatable_items.push(line.to_string());
                logs.push(format!("  - {line}"));
            }
        }
        _ => {
            state.rustup.check_failed = true;
            logs.push(log_pkg_line(
                "rustup",
                &format!("检查失败 (exit {status})."),
                MsgKind::Warn,
            ));
        }
    }
}

async fn check_fnm_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.fnm {
        logs.push(log_pkg_line("fnm", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("fnm").await {
        logs.push(log_pkg_line("fnm", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.fnm.installed = true;
    logs.push(log_pkg_line(
        "fnm",
        "正在检查 Node.js 版本更新 (fnm list/list-remote)...",
        MsgKind::Info,
    ));

    let Ok((list_status, list_output)) = run_capture("fnm", &["list"]).await else {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            "检查失败: 无法执行 fnm list.",
            MsgKind::Warn,
        ));
        return;
    };
    if list_status != 0 {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            &format!("检查失败 (fnm list, exit {list_status})."),
            MsgKind::Warn,
        ));
        return;
    }

    let installed_versions = list_output
        .lines()
        .filter_map(parse_fnm_version_token)
        .collect::<Vec<_>>();

    let Ok((latest_status, latest_output)) = run_capture("fnm", &["list-remote", "--latest"]).await
    else {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            "检查失败: 无法获取 latest 远端版本.",
            MsgKind::Warn,
        ));
        return;
    };
    if latest_status != 0 {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            &format!("检查失败 (fnm list-remote --latest, exit {latest_status})."),
            MsgKind::Warn,
        ));
        return;
    }

    let Ok((lts_status, lts_output)) =
        run_capture("fnm", &["list-remote", "--lts", "--latest"]).await
    else {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            "检查失败: 无法获取 LTS 远端版本.",
            MsgKind::Warn,
        ));
        return;
    };
    if lts_status != 0 {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            &format!("检查失败 (fnm list-remote --lts --latest, exit {lts_status})."),
            MsgKind::Warn,
        ));
        return;
    }

    let latest = latest_output.lines().find_map(parse_fnm_version_token);
    let lts = lts_output.lines().find_map(parse_fnm_version_token);
    let Some(latest_version) = latest else {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            "检查失败: latest 版本解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    let Some(lts_version) = lts else {
        state.fnm.check_failed = true;
        logs.push(log_pkg_line(
            "fnm",
            "检查失败: LTS 版本解析失败.",
            MsgKind::Warn,
        ));
        return;
    };

    if !installed_versions.iter().any(|v| v == &latest_version) {
        state
            .fnm
            .updatable_items
            .push(format!("latest -> {latest_version}"));
    }
    if !installed_versions.iter().any(|v| v == &lts_version) {
        state
            .fnm
            .updatable_items
            .push(format!("lts -> {lts_version}"));
    }

    if state.fnm.updatable_items.is_empty() {
        logs.push(log_pkg_line("fnm", "latest/LTS 已安装到最新.", MsgKind::Ok));
    } else {
        state.fnm.has_updates = true;
        logs.push(log_pkg_line(
            "fnm",
            "以下 Node.js 版本可安装/更新:",
            MsgKind::Info,
        ));
        for v in &state.fnm.updatable_items {
            logs.push(format!("  - {v}"));
        }
    }
}

async fn check_scoop_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.scoop {
        logs.push(log_pkg_line("scoop", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("scoop").await {
        logs.push(log_pkg_line("scoop", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.scoop.installed = true;
    logs.push(log_pkg_line(
        "scoop",
        "正在检查可升级项 (scoop status)...",
        MsgKind::Info,
    ));

    let Ok((status, output)) = run_capture("scoop", &["status"]).await else {
        state.scoop.check_failed = true;
        logs.push(log_pkg_line(
            "scoop",
            "检查失败: 无法执行 scoop status.",
            MsgKind::Warn,
        ));
        return;
    };

    if status != 0 {
        state.scoop.check_failed = true;
        logs.push(log_pkg_line(
            "scoop",
            &format!("检查失败 (scoop status, exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }

    let mut parsed = parse_scoop_status_output(&output);
    let mut metadata_outdated = parsed.metadata_outdated;
    if parsed.updatable_items.is_empty() && metadata_outdated {
        logs.push(log_pkg_line(
            "scoop",
            "Scoop/桶元数据过期, 正在刷新后重新检查 (scoop update --quiet)...",
            MsgKind::Warn,
        ));
        match run_capture("scoop", &["update", "--quiet"]).await {
            Ok((0, _)) => match run_capture("scoop", &["status", "--local"]).await {
                Ok((0, refreshed_output)) => {
                    parsed = parse_scoop_status_output(&refreshed_output);
                    metadata_outdated = parsed.metadata_outdated;
                }
                Ok((refresh_status, _)) => logs.push(log_pkg_line(
                    "scoop",
                    &format!("重新检查失败 (scoop status --local, exit {refresh_status})."),
                    MsgKind::Warn,
                )),
                Err(_) => logs.push(log_pkg_line(
                    "scoop",
                    "重新检查失败: 无法执行 scoop status --local.",
                    MsgKind::Warn,
                )),
            },
            Ok((refresh_status, _)) => logs.push(log_pkg_line(
                "scoop",
                &format!("元数据刷新失败 (scoop update --quiet, exit {refresh_status})."),
                MsgKind::Warn,
            )),
            Err(_) => logs.push(log_pkg_line(
                "scoop",
                "元数据刷新失败: 无法执行 scoop update --quiet.",
                MsgKind::Warn,
            )),
        }
    }
    state.scoop.updatable_items = parsed.updatable_items;

    if state.scoop.updatable_items.is_empty() {
        if metadata_outdated {
            state.scoop.has_updates = true;
            logs.push(log_pkg_line(
                "scoop",
                "Scoop/桶元数据有更新, 可执行 upgrade 阶段刷新.",
                MsgKind::Warn,
            ));
        } else {
            logs.push(log_pkg_line("scoop", "已是最新.", MsgKind::Ok));
        }
    } else {
        state.scoop.has_updates = true;
        logs.push(log_pkg_line("scoop", "以下包可升级:", MsgKind::Info));
        for p in &state.scoop.updatable_items {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_paru_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.paru {
        logs.push(log_pkg_line("paru", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("paru").await {
        logs.push(log_pkg_line("paru", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.paru.installed = true;
    logs.push(log_pkg_line(
        "paru",
        "正在检查 AUR 可升级项 (paru -Qua)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("paru", &["-Qua"]).await else {
        state.paru.check_failed = true;
        logs.push(log_pkg_line(
            "paru",
            "检查失败: 无法执行 paru 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    let trimmed_output = output.trim();
    if status != 0 {
        if status == 1 && trimmed_output.is_empty() {
            logs.push(log_pkg_line("paru", "AUR 包已是最新.", MsgKind::Ok));
            return;
        }
        state.paru.check_failed = true;
        logs.push(log_pkg_line(
            "paru",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    for line in trimmed_output
        .lines()
        .map(str::trim)
        .filter(|x| !x.is_empty())
    {
        if let Some(name) = first_token(line) {
            state.paru.updatable_items.push(name);
        }
    }
    if state.paru.updatable_items.is_empty() {
        logs.push(log_pkg_line("paru", "AUR 包已是最新.", MsgKind::Ok));
    } else {
        state.paru.has_updates = true;
        logs.push(log_pkg_line("paru", "以下 AUR 包可升级:", MsgKind::Info));
        for p in &state.paru.updatable_items {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_flatpak_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.flatpak {
        logs.push(log_pkg_line("flatpak", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("flatpak").await {
        logs.push(log_pkg_line("flatpak", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.flatpak.installed = true;
    logs.push(log_pkg_line(
        "flatpak",
        "正在检查可升级项 (flatpak remote-ls --updates --columns=application)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application"],
    )
    .await
    else {
        state.flatpak.check_failed = true;
        logs.push(log_pkg_line(
            "flatpak",
            "检查失败: 无法执行 flatpak 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.flatpak.check_failed = true;
        logs.push(log_pkg_line(
            "flatpak",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    state.flatpak.updatable_items = output
        .lines()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if state.flatpak.updatable_items.is_empty() {
        logs.push(log_pkg_line("flatpak", "已是最新.", MsgKind::Ok));
    } else {
        state.flatpak.has_updates = true;
        logs.push(log_pkg_line("flatpak", "以下应用可升级:", MsgKind::Info));
        for p in &state.flatpak.updatable_items {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_pacman_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.pacman {
        logs.push(log_pkg_line("pacman", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("pacman").await {
        logs.push(log_pkg_line("pacman", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.pacman.installed = true;
    let use_checkupdates = command_exists("checkupdates").await;
    let (status, output) = if use_checkupdates {
        run_capture("checkupdates", &[])
            .await
            .unwrap_or((-1, String::new()))
    } else {
        run_capture("pacman", &["-Qu"])
            .await
            .unwrap_or((-1, String::new()))
    };
    if status != 0 {
        let no_updates = if use_checkupdates {
            status == 2
        } else {
            status == 1 && output.trim().is_empty()
        };
        if no_updates {
            logs.push(log_pkg_line("pacman", "已是最新.", MsgKind::Ok));
        } else {
            state.pacman.check_failed = true;
            logs.push(log_pkg_line(
                "pacman",
                &format!("检查失败 (exit {status})."),
                MsgKind::Warn,
            ));
        }
        return;
    }
    for line in output.lines().map(str::trim).filter(|x| !x.is_empty()) {
        if let Some(name) = first_token(line) {
            state.pacman.updatable_items.push(name);
        }
    }
    if state.pacman.updatable_items.is_empty() {
        logs.push(log_pkg_line("pacman", "已是最新.", MsgKind::Ok));
    } else {
        state.pacman.has_updates = true;
        logs.push(log_pkg_line("pacman", "以下包可升级:", MsgKind::Info));
        for p in &state.pacman.updatable_items {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_pkg_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.pkg {
        logs.push(log_pkg_line("pkg", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("pkg").await {
        logs.push(log_pkg_line("pkg", "未安装, 跳过.", MsgKind::Warn));
        return;
    }

    state.pkg.installed = true;
    logs.push(log_pkg_line(
        "pkg",
        "正在检查可升级项 (apt list --upgradable)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("apt", &["list", "--upgradable"]).await else {
        state.pkg.check_failed = true;
        logs.push(log_pkg_line(
            "pkg",
            "检查失败: 无法执行 apt 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.pkg.check_failed = true;
        logs.push(log_pkg_line(
            "pkg",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }

    for line in output.lines().map(str::trim).filter(|x| !x.is_empty()) {
        if line.starts_with("WARNING:")
            || line.eq_ignore_ascii_case("listing...")
            || line.eq_ignore_ascii_case("listing")
            || line.eq_ignore_ascii_case("done")
        {
            continue;
        }
        let token = line.split_whitespace().next().unwrap_or_default();
        if token.is_empty() || !token.contains('/') {
            continue;
        }
        let name = token.split('/').next().unwrap_or(token).to_string();
        if !name.is_empty() {
            state.pkg.updatable_items.push(name);
        }
    }

    if state.pkg.updatable_items.is_empty() {
        logs.push(log_pkg_line("pkg", "已是最新.", MsgKind::Ok));
    } else {
        state.pkg.has_updates = true;
        logs.push(log_pkg_line("pkg", "以下包可升级:", MsgKind::Info));
        for p in &state.pkg.updatable_items {
            logs.push(format!("  - {p}"));
        }
    }
}

async fn check_nvim_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable.nvim {
        logs.push(log_pkg_line("nvim", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("nvim").await {
        logs.push(log_pkg_line("nvim", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.nvim.installed = true;
    logs.push(log_pkg_line(
        "nvim",
        "正在检查 Lazy/Mason 可用性...",
        MsgKind::Info,
    ));

    let Ok((lazy_status, lazy_out)) = run_nvim_headless_capture(&[
        "+lua local ok=pcall(require,'lazy'); print(ok and 'UPDT_LAZY_OK' or 'UPDT_LAZY_MISSING')",
        "+qa",
    ])
    .await
    else {
        state.nvim.check_failed = true;
        logs.push(log_pkg_line(
            "nvim",
            "检查失败: 无法启动 nvim.",
            MsgKind::Warn,
        ));
        return;
    };
    if lazy_status != 0 {
        state.nvim.check_failed = true;
        logs.push(log_pkg_line(
            "nvim",
            &format!("检查失败: Lazy 探测退出码 {lazy_status}."),
            MsgKind::Warn,
        ));
        return;
    }
    state.nvim.lazy_available = lazy_out.contains("UPDT_LAZY_OK");

    let Ok((mason_status, mason_out)) = run_nvim_headless_capture(&[
        "+lua local ok=pcall(require,'mason'); print(ok and 'UPDT_MASON_OK' or 'UPDT_MASON_MISSING')",
        "+qa",
    ])
    .await
    else {
        state.nvim.check_failed = true;
        logs.push(log_pkg_line(
            "nvim",
            "检查失败: Mason 探测命令失败.",
            MsgKind::Warn,
        ));
        return;
    };
    if mason_status != 0 {
        state.nvim.check_failed = true;
        logs.push(log_pkg_line(
            "nvim",
            &format!("检查失败: Mason 探测退出码 {mason_status}."),
            MsgKind::Warn,
        ));
        return;
    }
    state.nvim.mason_available = mason_out.contains("UPDT_MASON_OK");

    if !state.nvim.lazy_available && !state.nvim.mason_available {
        logs.push(log_pkg_line(
            "nvim",
            "未检测到 Lazy 或 Mason, 跳过.",
            MsgKind::Warn,
        ));
        return;
    }

    if state.nvim.lazy_available {
        let Ok((lazy_count_status, lazy_count_out)) = run_nvim_headless_capture(&[
            "+lua local checker=require('lazy.manage.checker'); checker.check({show=false}); vim.wait(120000, function() return not checker.running end, 200); local n=0; for _ in pairs(checker.updated or {}) do n=n+1 end; print('UPDT_LAZY_COUNT='..n)",
            "+qa",
        ])
        .await
        else {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                "检查失败: Lazy 更新计数命令失败.",
                MsgKind::Warn,
            ));
            return;
        };
        if lazy_count_status != 0 {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                &format!("检查失败: Lazy 计数退出码 {lazy_count_status}."),
                MsgKind::Warn,
            ));
            return;
        }
        if let Some(lazy_count) = extract_marker_count(&lazy_count_out, "UPDT_LAZY_COUNT=") {
            if lazy_count > 0 {
                state
                    .nvim
                    .updatable_components
                    .push(format!("Lazy plugins: {lazy_count} 项可更新"));
            }
        } else {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                "检查失败: Lazy 计数输出解析失败.",
                MsgKind::Warn,
            ));
            return;
        }
    }

    if state.nvim.mason_available {
        let Ok((mason_count_status, mason_count_out)) = run_nvim_headless_capture(&[
            "+lua local reg=require('mason-registry'); local ok,pkgs=pcall(reg.get_installed_packages); if not ok then print('UPDT_MASON_COUNT=0') return end; local n=0; for _,p in ipairs(pkgs) do local ok_i,iv=pcall(p.get_installed_version,p); local ok_l,lv=pcall(p.get_latest_version,p); if ok_i and ok_l and tostring(iv)~=tostring(lv) then n=n+1 end end; print('UPDT_MASON_COUNT='..n)",
            "+qa",
        ])
        .await
        else {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                "检查失败: Mason 更新计数命令失败.",
                MsgKind::Warn,
            ));
            return;
        };
        if mason_count_status != 0 {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                &format!("检查失败: Mason 计数退出码 {mason_count_status}."),
                MsgKind::Warn,
            ));
            return;
        }
        if let Some(mason_count) = extract_marker_count(&mason_count_out, "UPDT_MASON_COUNT=") {
            if mason_count > 0 {
                state
                    .nvim
                    .updatable_components
                    .push(format!("Mason tools: {mason_count} 项可更新"));
            }
        } else {
            state.nvim.check_failed = true;
            logs.push(log_pkg_line(
                "nvim",
                "检查失败: Mason 计数输出解析失败.",
                MsgKind::Warn,
            ));
            return;
        }
    }

    if state.nvim.updatable_components.is_empty() {
        logs.push(log_pkg_line(
            "nvim",
            "Neovim 插件与 Mason 已是最新.",
            MsgKind::Ok,
        ));
        return;
    }

    state.nvim.has_updates = true;
    logs.push(log_pkg_line("nvim", "检测到可更新项:", MsgKind::Info));
    for item in &state.nvim.updatable_components {
        logs.push(format!("  - {item}"));
    }
}

pub async fn run_single_check(target: &str) -> CheckResult {
    let mut local = AppState::default();
    let mut logs = Vec::new();
    parse_profile(&mut local).await;
    match target {
        "brew" => check_brew_quiet(&mut local, &mut logs).await,
        "npm" => check_npm_quiet(&mut local, &mut logs).await,
        "cargo" => check_cargo_quiet(&mut local, &mut logs).await,
        "nvim" => check_nvim_quiet(&mut local, &mut logs).await,
        "rustup" => check_rustup_quiet(&mut local, &mut logs).await,
        "fnm" => check_fnm_quiet(&mut local, &mut logs).await,
        "scoop" => check_scoop_quiet(&mut local, &mut logs).await,
        "paru" => check_paru_quiet(&mut local, &mut logs).await,
        "flatpak" => check_flatpak_quiet(&mut local, &mut logs).await,
        "pacman" => check_pacman_quiet(&mut local, &mut logs).await,
        "pkg" => check_pkg_quiet(&mut local, &mut logs).await,
        _ => {}
    }
    CheckResult {
        target: target.to_string(),
        state: local,
        logs,
    }
}

pub fn merge_check_result(state: &mut AppState, target: &str, local: AppState) {
    match target {
        "brew" => state.brew = local.brew,
        "npm" => state.npm = local.npm,
        "cargo" => state.cargo = local.cargo,
        "nvim" => state.nvim = local.nvim,
        "rustup" => state.rustup = local.rustup,
        "fnm" => state.fnm = local.fnm,
        "scoop" => state.scoop = local.scoop,
        "paru" => state.paru = local.paru,
        "flatpak" => state.flatpak = local.flatpak,
        "pacman" => state.pacman = local.pacman,
        "pkg" => state.pkg = local.pkg,
        _ => {}
    }
}
