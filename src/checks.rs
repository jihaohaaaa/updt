use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use crate::command::{
    command_exists, run_capture, run_cargo_install_update_capture, run_nvim_headless_capture,
};
use crate::output::{MsgKind, log_pkg_line};
use crate::parse::{
    extract_marker_count, first_json_payload, first_token, parse_cargo_list,
    parse_fnm_version_token, parse_scoop_status_output,
};
use crate::profile::parse_profile;
use crate::state::{AppState, SystemProfile};

pub struct CheckResult {
    pub target: String,
    pub state: AppState,
    pub logs: Vec<String>,
}

type CheckFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
type CheckRunner = for<'a> fn(&'a mut AppState, &'a mut Vec<String>) -> CheckFuture<'a>;
type CheckMerger = fn(&mut AppState, AppState);

struct CheckTargetHandler {
    target: &'static str,
    check: CheckRunner,
    merge: CheckMerger,
}

async fn ensure_check_target(
    enabled: bool,
    installed: &mut bool,
    command: &str,
    target: &str,
    logs: &mut Vec<String>,
) -> bool {
    if !enabled {
        logs.push(log_pkg_line(target, "按系统策略跳过.", MsgKind::Warn));
        return false;
    }
    if !command_exists(command).await {
        logs.push(log_pkg_line(target, "未安装, 跳过.", MsgKind::Warn));
        return false;
    }
    *installed = true;
    true
}

fn mark_check_failed(check_failed: &mut bool, target: &str, message: &str, logs: &mut Vec<String>) {
    *check_failed = true;
    logs.push(log_pkg_line(target, message, MsgKind::Warn));
}

fn log_item_list(logs: &mut Vec<String>, target: &str, header: &str, items: &[String]) {
    logs.push(log_pkg_line(target, header, MsgKind::Info));
    for item in items {
        logs.push(format!("  - {item}"));
    }
}

fn record_bucket_items(
    has_updates: &mut bool,
    logs: &mut Vec<String>,
    target: &str,
    items: &[String],
    ok_message: &str,
    update_header: &str,
) {
    if items.is_empty() {
        logs.push(log_pkg_line(target, ok_message, MsgKind::Ok));
    } else {
        *has_updates = true;
        log_item_list(logs, target, update_header, items);
    }
}

fn package_names_from_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(first_token)
        .collect()
}

macro_rules! check_runner {
    ($name:ident, $check:ident) => {
        fn $name<'a>(state: &'a mut AppState, logs: &'a mut Vec<String>) -> CheckFuture<'a> {
            Box::pin($check(state, logs))
        }
    };
}

check_runner!(run_brew_check, check_brew_quiet);
check_runner!(run_npm_check, check_npm_quiet);
check_runner!(run_cargo_check, check_cargo_quiet);
check_runner!(run_nvim_check, check_nvim_quiet);
check_runner!(run_rustup_check, check_rustup_quiet);
check_runner!(run_fnm_check, check_fnm_quiet);
check_runner!(run_scoop_check, check_scoop_quiet);
check_runner!(run_paru_check, check_paru_quiet);
check_runner!(run_flatpak_check, check_flatpak_quiet);
check_runner!(run_pacman_check, check_pacman_quiet);
check_runner!(run_pkg_check, check_pkg_quiet);

fn merge_brew_check(state: &mut AppState, local: AppState) {
    state.brew = local.brew;
}

fn merge_npm_check(state: &mut AppState, local: AppState) {
    state.npm = local.npm;
}

fn merge_cargo_check(state: &mut AppState, local: AppState) {
    state.cargo = local.cargo;
}

fn merge_nvim_check(state: &mut AppState, local: AppState) {
    state.nvim = local.nvim;
}

fn merge_rustup_check(state: &mut AppState, local: AppState) {
    state.rustup = local.rustup;
}

fn merge_fnm_check(state: &mut AppState, local: AppState) {
    state.fnm = local.fnm;
}

fn merge_scoop_check(state: &mut AppState, local: AppState) {
    state.scoop = local.scoop;
}

fn merge_paru_check(state: &mut AppState, local: AppState) {
    state.paru = local.paru;
}

fn merge_flatpak_check(state: &mut AppState, local: AppState) {
    state.flatpak = local.flatpak;
}

fn merge_pacman_check(state: &mut AppState, local: AppState) {
    state.pacman = local.pacman;
}

fn merge_pkg_check(state: &mut AppState, local: AppState) {
    state.pkg = local.pkg;
}

static CHECK_TARGET_HANDLERS: &[CheckTargetHandler] = &[
    CheckTargetHandler {
        target: "brew",
        check: run_brew_check,
        merge: merge_brew_check,
    },
    CheckTargetHandler {
        target: "npm",
        check: run_npm_check,
        merge: merge_npm_check,
    },
    CheckTargetHandler {
        target: "cargo",
        check: run_cargo_check,
        merge: merge_cargo_check,
    },
    CheckTargetHandler {
        target: "nvim",
        check: run_nvim_check,
        merge: merge_nvim_check,
    },
    CheckTargetHandler {
        target: "rustup",
        check: run_rustup_check,
        merge: merge_rustup_check,
    },
    CheckTargetHandler {
        target: "fnm",
        check: run_fnm_check,
        merge: merge_fnm_check,
    },
    CheckTargetHandler {
        target: "scoop",
        check: run_scoop_check,
        merge: merge_scoop_check,
    },
    CheckTargetHandler {
        target: "paru",
        check: run_paru_check,
        merge: merge_paru_check,
    },
    CheckTargetHandler {
        target: "flatpak",
        check: run_flatpak_check,
        merge: merge_flatpak_check,
    },
    CheckTargetHandler {
        target: "pacman",
        check: run_pacman_check,
        merge: merge_pacman_check,
    },
    CheckTargetHandler {
        target: "pkg",
        check: run_pkg_check,
        merge: merge_pkg_check,
    },
];

fn check_target_handler(target: &str) -> Option<&'static CheckTargetHandler> {
    CHECK_TARGET_HANDLERS
        .iter()
        .find(|handler| handler.target == target)
}

async fn check_brew_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.brew,
        &mut state.brew.installed,
        "brew",
        "brew",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "brew",
        "正在检查可升级项 (brew outdated --greedy --json=v2)...",
        MsgKind::Info,
    ));

    let Some(root) = load_brew_outdated_json(&mut state.brew.check_failed, logs).await else {
        return;
    };

    state.brew.formula_list = brew_json_names(&root, "formulae");
    state.brew.cask_list = brew_json_names(&root, "casks");
    record_brew_updates(state, logs);
}

async fn load_brew_outdated_json(check_failed: &mut bool, logs: &mut Vec<String>) -> Option<Value> {
    let Ok((status, output)) = run_capture("brew", &["outdated", "--greedy", "--json=v2"]).await
    else {
        mark_check_failed(check_failed, "brew", "检查失败: 无法执行 brew 命令.", logs);
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "brew",
            &format!("检查失败 (brew outdated --greedy --json=v2, exit {status})."),
            logs,
        );
        return None;
    }
    let Some(json_text) = first_json_payload(&output) else {
        mark_check_failed(check_failed, "brew", "检查失败: 未找到 JSON 内容.", logs);
        return None;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        mark_check_failed(check_failed, "brew", "检查失败: JSON 解析失败.", logs);
        return None;
    };
    Some(root)
}

fn brew_json_names(root: &Value, key: &str) -> Vec<String> {
    root.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn record_brew_updates(state: &mut AppState, logs: &mut Vec<String>) {
    record_bucket_items(
        &mut state.brew.has_updates,
        logs,
        "brew",
        &state.brew.formula_list,
        "Formula: 已是最新.",
        "Formula 可升级:",
    );
    record_bucket_items(
        &mut state.brew.has_updates,
        logs,
        "brew",
        &state.brew.cask_list,
        "Cask: 已是最新.",
        "Cask 可升级:",
    );
}

async fn check_npm_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.npm,
        &mut state.npm.installed,
        "npm",
        "npm",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "npm",
        "正在检查全局包更新 (npm outdated --json --global)...",
        MsgKind::Info,
    ));
    let Some(items) = load_npm_outdated_items(&mut state.npm.check_failed, logs).await else {
        return;
    };
    state.npm.updatable_items = items;
    record_bucket_items(
        &mut state.npm.has_updates,
        logs,
        "npm",
        &state.npm.updatable_items,
        "全局包已是最新.",
        "以下全局包可升级:",
    );
}

async fn load_npm_outdated_items(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Ok((status, output)) = run_capture("npm", &["outdated", "--json", "--global"]).await else {
        mark_check_failed(check_failed, "npm", "检查失败: 无法执行 npm 命令.", logs);
        return None;
    };
    if status == 0 {
        return Some(Vec::new());
    }
    if status != 1 {
        mark_check_failed(
            check_failed,
            "npm",
            &format!("检查失败 (exit {status})."),
            logs,
        );
        return None;
    }
    let Some(json_text) = first_json_payload(&output) else {
        mark_check_failed(check_failed, "npm", "检查失败: JSON 解析失败.", logs);
        return None;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        mark_check_failed(check_failed, "npm", "检查失败: JSON 解析失败.", logs);
        return None;
    };
    let Some(obj) = root.as_object() else {
        return Some(Vec::new());
    };
    Some(obj.keys().cloned().collect())
}

pub async fn check_cargo_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.cargo,
        &mut state.cargo.installed,
        "cargo",
        "cargo",
        logs,
    )
    .await
    {
        return;
    }
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
    let Some(pkgs) = load_cargo_update_list(&mut state.cargo.check_failed, logs).await else {
        return;
    };
    state.cargo.updatable_packages = pkgs;
    record_bucket_items(
        &mut state.cargo.has_updates,
        logs,
        "cargo",
        &state.cargo.updatable_packages,
        "已安装 crate 已是最新.",
        "以下 crate 可升级:",
    );
}

async fn load_cargo_update_list(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Ok((status, output)) = run_cargo_install_update_capture(&["--list"]).await else {
        mark_check_failed(check_failed, "cargo", "检查失败: 命令执行失败.", logs);
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "cargo",
            &format!("检查失败 (exit {status})."),
            logs,
        );
        return None;
    }
    let Ok(pkgs) = parse_cargo_list(&output) else {
        mark_check_failed(check_failed, "cargo", "检查失败: 输出解析失败.", logs);
        return None;
    };
    Some(pkgs)
}

async fn check_rustup_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.rustup,
        &mut state.rustup.installed,
        "rustup",
        "rustup",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "rustup",
        "正在检查 toolchain 更新 (rustup check --no-self-update)...",
        MsgKind::Info,
    ));
    let Some((status, output)) =
        load_rustup_check_output(&mut state.rustup.check_failed, logs).await
    else {
        return;
    };
    record_rustup_status(state, logs, status, &output);
    recommend_windows_gnu_toolchain(state, logs).await;
}

async fn load_rustup_check_output(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<(i32, String)> {
    let Ok((status, output)) = run_capture("rustup", &["check", "--no-self-update"]).await else {
        mark_check_failed(
            check_failed,
            "rustup",
            "检查失败: 无法执行 rustup 命令.",
            logs,
        );
        return None;
    };
    Some((status, output))
}

fn record_rustup_status(state: &mut AppState, logs: &mut Vec<String>, status: i32, output: &str) {
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
            mark_check_failed(
                &mut state.rustup.check_failed,
                "rustup",
                &format!("检查失败 (exit {status})."),
                logs,
            );
        }
    }
}

async fn recommend_windows_gnu_toolchain(state: &mut AppState, logs: &mut Vec<String>) {
    if state.system_profile != SystemProfile::Windows {
        return;
    }
    logs.push(log_pkg_line(
        "rustup",
        "正在检查 Windows GNU toolchain (rustup toolchain list)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("rustup", &["toolchain", "list"]).await else {
        logs.push(log_pkg_line(
            "rustup",
            "建议检查失败: 无法执行 rustup toolchain list.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        logs.push(log_pkg_line(
            "rustup",
            &format!("建议检查失败 (rustup toolchain list, exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    if !rustup_toolchains_include_windows_gnu(&output) {
        logs.push(log_pkg_line(
            "rustup",
            "建议安装 GNU toolchain: rustup toolchain install stable-x86_64-pc-windows-gnu",
            MsgKind::Warn,
        ));
    }
}

fn rustup_toolchains_include_windows_gnu(output: &str) -> bool {
    output
        .lines()
        .map(str::trim)
        .any(|line| line.contains("windows-gnu"))
}

async fn check_fnm_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.fnm,
        &mut state.fnm.installed,
        "fnm",
        "fnm",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "fnm",
        "正在检查 Node.js 版本更新 (fnm list/list-remote)...",
        MsgKind::Info,
    ));

    let Some(installed_versions) =
        load_fnm_installed_versions(&mut state.fnm.check_failed, logs).await
    else {
        return;
    };
    let Some(latest_version) = load_fnm_remote_version(
        &mut state.fnm.check_failed,
        logs,
        &["list-remote", "--latest"],
        "latest",
    )
    .await
    else {
        return;
    };
    let Some(lts_version) = load_fnm_remote_version(
        &mut state.fnm.check_failed,
        logs,
        &["list-remote", "--lts", "--latest"],
        "LTS",
    )
    .await
    else {
        return;
    };

    record_fnm_updates(
        state,
        logs,
        &installed_versions,
        &latest_version,
        &lts_version,
    );
}

async fn load_fnm_installed_versions(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Ok((list_status, list_output)) = run_capture("fnm", &["list"]).await else {
        mark_check_failed(check_failed, "fnm", "检查失败: 无法执行 fnm list.", logs);
        return None;
    };
    if list_status != 0 {
        mark_check_failed(
            check_failed,
            "fnm",
            &format!("检查失败 (fnm list, exit {list_status})."),
            logs,
        );
        return None;
    }

    Some(
        list_output
            .lines()
            .filter_map(parse_fnm_version_token)
            .collect(),
    )
}

async fn load_fnm_remote_version(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
    args: &[&str],
    label: &str,
) -> Option<String> {
    let Ok((status, output)) = run_capture("fnm", args).await else {
        mark_check_failed(
            check_failed,
            "fnm",
            &format!("检查失败: 无法获取 {label} 远端版本."),
            logs,
        );
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "fnm",
            &format!("检查失败 (fnm {}, exit {status}).", args.join(" ")),
            logs,
        );
        return None;
    }
    parse_fnm_remote_version(check_failed, logs, &output, label)
}

fn parse_fnm_remote_version(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
    output: &str,
    label: &str,
) -> Option<String> {
    let version = output.lines().find_map(parse_fnm_version_token);
    if version.is_none() {
        mark_check_failed(
            check_failed,
            "fnm",
            &format!("检查失败: {label} 版本解析失败."),
            logs,
        );
    }
    version
}

fn record_fnm_updates(
    state: &mut AppState,
    logs: &mut Vec<String>,
    installed_versions: &[String],
    latest_version: &str,
    lts_version: &str,
) {
    if !installed_versions.iter().any(|v| v == latest_version) {
        state
            .fnm
            .updatable_items
            .push(format!("latest -> {latest_version}"));
    }
    if !installed_versions.iter().any(|v| v == lts_version) {
        state
            .fnm
            .updatable_items
            .push(format!("lts -> {lts_version}"));
    }

    if state.fnm.updatable_items.is_empty() {
        logs.push(log_pkg_line("fnm", "latest/LTS 已安装到最新.", MsgKind::Ok));
    } else {
        state.fnm.has_updates = true;
        log_item_list(
            logs,
            "fnm",
            "以下 Node.js 版本可安装/更新:",
            &state.fnm.updatable_items,
        );
    }
}

async fn check_scoop_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.scoop,
        &mut state.scoop.installed,
        "scoop",
        "scoop",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "scoop",
        "正在检查可升级项 (scoop status)...",
        MsgKind::Info,
    ));

    let Some(parsed) = load_scoop_status(&mut state.scoop.check_failed, logs).await else {
        return;
    };
    let parsed = refresh_scoop_status_if_needed(parsed, logs).await;
    record_scoop_status(state, logs, parsed);
}

async fn load_scoop_status(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<crate::parse::ScoopStatusOutput> {
    let Ok((status, output)) = run_capture("scoop", &["status"]).await else {
        mark_check_failed(
            check_failed,
            "scoop",
            "检查失败: 无法执行 scoop status.",
            logs,
        );
        return None;
    };

    if status != 0 {
        mark_check_failed(
            check_failed,
            "scoop",
            &format!("检查失败 (scoop status, exit {status})."),
            logs,
        );
        return None;
    }

    Some(parse_scoop_status_output(&output))
}

async fn refresh_scoop_status_if_needed(
    mut parsed: crate::parse::ScoopStatusOutput,
    logs: &mut Vec<String>,
) -> crate::parse::ScoopStatusOutput {
    if parsed.updatable_items.is_empty() && parsed.metadata_outdated {
        logs.push(log_pkg_line(
            "scoop",
            "Scoop/桶元数据过期, 正在刷新后重新检查 (scoop update --quiet)...",
            MsgKind::Warn,
        ));
        refresh_scoop_status_after_metadata_update(&mut parsed, logs).await;
    }
    parsed
}

async fn refresh_scoop_status_after_metadata_update(
    parsed: &mut crate::parse::ScoopStatusOutput,
    logs: &mut Vec<String>,
) {
    match run_capture("scoop", &["update", "--quiet"]).await {
        Ok((0, _)) => refresh_scoop_status_local(parsed, logs).await,
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

async fn refresh_scoop_status_local(
    parsed: &mut crate::parse::ScoopStatusOutput,
    logs: &mut Vec<String>,
) {
    match run_capture("scoop", &["status", "--local"]).await {
        Ok((0, refreshed_output)) => *parsed = parse_scoop_status_output(&refreshed_output),
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
    }
}

fn record_scoop_status(
    state: &mut AppState,
    logs: &mut Vec<String>,
    parsed: crate::parse::ScoopStatusOutput,
) {
    let metadata_outdated = parsed.metadata_outdated;
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
        log_item_list(logs, "scoop", "以下包可升级:", &state.scoop.updatable_items);
    }
}

async fn check_paru_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.paru,
        &mut state.paru.installed,
        "paru",
        "paru",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "paru",
        "正在检查 AUR 可升级项 (paru -Qua)...",
        MsgKind::Info,
    ));
    let Some(output) = load_paru_updates(&mut state.paru.check_failed, logs).await else {
        return;
    };
    state.paru.updatable_items = package_names_from_lines(&output);
    record_bucket_items(
        &mut state.paru.has_updates,
        logs,
        "paru",
        &state.paru.updatable_items,
        "AUR 包已是最新.",
        "以下 AUR 包可升级:",
    );
}

async fn load_paru_updates(check_failed: &mut bool, logs: &mut Vec<String>) -> Option<String> {
    let Ok((status, output)) = run_capture("paru", &["-Qua"]).await else {
        mark_check_failed(check_failed, "paru", "检查失败: 无法执行 paru 命令.", logs);
        return None;
    };
    let trimmed_output = output.trim();
    if status != 0 {
        if status == 1 && trimmed_output.is_empty() {
            logs.push(log_pkg_line("paru", "AUR 包已是最新.", MsgKind::Ok));
            return None;
        }
        mark_check_failed(
            check_failed,
            "paru",
            &format!("检查失败 (exit {status})."),
            logs,
        );
        return None;
    }
    Some(trimmed_output.to_string())
}

async fn check_flatpak_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.flatpak,
        &mut state.flatpak.installed,
        "flatpak",
        "flatpak",
        logs,
    )
    .await
    {
        return;
    }
    logs.push(log_pkg_line(
        "flatpak",
        "正在检查可升级项 (flatpak remote-ls --updates --columns=application)...",
        MsgKind::Info,
    ));
    let Some(items) = load_flatpak_updates(&mut state.flatpak.check_failed, logs).await else {
        return;
    };
    state.flatpak.updatable_items = items;
    record_bucket_items(
        &mut state.flatpak.has_updates,
        logs,
        "flatpak",
        &state.flatpak.updatable_items,
        "已是最新.",
        "以下应用可升级:",
    );
}

async fn load_flatpak_updates(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
) -> Option<Vec<String>> {
    let Ok((status, output)) = run_capture(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application"],
    )
    .await
    else {
        mark_check_failed(
            check_failed,
            "flatpak",
            "检查失败: 无法执行 flatpak 命令.",
            logs,
        );
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "flatpak",
            &format!("检查失败 (exit {status})."),
            logs,
        );
        return None;
    }
    Some(
        output
            .lines()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    )
}

async fn check_pacman_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.pacman,
        &mut state.pacman.installed,
        "pacman",
        "pacman",
        logs,
    )
    .await
    {
        return;
    }
    let use_checkupdates = command_exists("checkupdates").await;
    let (status, output) = load_pacman_updates(use_checkupdates).await;
    if status != 0 {
        record_pacman_nonzero(state, logs, status, &output, use_checkupdates);
        return;
    }
    state.pacman.updatable_items = package_names_from_lines(&output);
    record_bucket_items(
        &mut state.pacman.has_updates,
        logs,
        "pacman",
        &state.pacman.updatable_items,
        "已是最新.",
        "以下包可升级:",
    );
}

async fn load_pacman_updates(use_checkupdates: bool) -> (i32, String) {
    if use_checkupdates {
        run_capture("checkupdates", &[])
            .await
            .unwrap_or((-1, String::new()))
    } else {
        run_capture("pacman", &["-Qu"])
            .await
            .unwrap_or((-1, String::new()))
    }
}

fn record_pacman_nonzero(
    state: &mut AppState,
    logs: &mut Vec<String>,
    status: i32,
    output: &str,
    use_checkupdates: bool,
) {
    if pacman_status_means_no_updates(status, output, use_checkupdates) {
        logs.push(log_pkg_line("pacman", "已是最新.", MsgKind::Ok));
    } else {
        mark_check_failed(
            &mut state.pacman.check_failed,
            "pacman",
            &format!("检查失败 (exit {status})."),
            logs,
        );
    }
}

fn pacman_status_means_no_updates(status: i32, output: &str, use_checkupdates: bool) -> bool {
    if use_checkupdates {
        status == 2
    } else {
        status == 1 && output.trim().is_empty()
    }
}

async fn check_pkg_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_check_target(
        state.enable.pkg,
        &mut state.pkg.installed,
        "pkg",
        "pkg",
        logs,
    )
    .await
    {
        return;
    }

    logs.push(log_pkg_line(
        "pkg",
        "正在检查可升级项 (apt list --upgradable)...",
        MsgKind::Info,
    ));
    let Some(items) = load_pkg_updates(&mut state.pkg.check_failed, logs).await else {
        return;
    };
    state.pkg.updatable_items = items;
    record_bucket_items(
        &mut state.pkg.has_updates,
        logs,
        "pkg",
        &state.pkg.updatable_items,
        "已是最新.",
        "以下包可升级:",
    );
}

async fn load_pkg_updates(check_failed: &mut bool, logs: &mut Vec<String>) -> Option<Vec<String>> {
    let Ok((status, output)) = run_capture("apt", &["list", "--upgradable"]).await else {
        mark_check_failed(check_failed, "pkg", "检查失败: 无法执行 apt 命令.", logs);
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "pkg",
            &format!("检查失败 (exit {status})."),
            logs,
        );
        return None;
    }
    Some(pkg_updatable_names(&output))
}

fn pkg_updatable_names(output: &str) -> Vec<String> {
    output.lines().filter_map(pkg_updatable_name).collect()
}

fn pkg_updatable_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if pkg_output_noise(trimmed) {
        return None;
    }
    let token = trimmed.split_whitespace().next().unwrap_or_default();
    pkg_name_from_token(token)
}

fn pkg_output_noise(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("WARNING:")
        || line.eq_ignore_ascii_case("listing...")
        || line.eq_ignore_ascii_case("listing")
        || line.eq_ignore_ascii_case("done")
}

fn pkg_name_from_token(token: &str) -> Option<String> {
    if token.is_empty() || !token.contains('/') {
        return None;
    }
    let name = token.split('/').next().unwrap_or(token);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        NVIM_COUNT_PROBES, add_nvim_component_if_needed, nvim_count_from_output,
        package_names_from_lines, pacman_status_means_no_updates, pkg_updatable_name,
        pkg_updatable_names, rustup_toolchains_include_windows_gnu,
    };

    #[test]
    fn detects_windows_gnu_toolchain() {
        let output = "stable-x86_64-pc-windows-msvc (default)\nstable-x86_64-pc-windows-gnu\n";

        assert!(rustup_toolchains_include_windows_gnu(output));
    }

    #[test]
    fn reports_missing_windows_gnu_toolchain() {
        let output = "stable-x86_64-pc-windows-msvc (default)\nnightly-x86_64-pc-windows-msvc\n";

        assert!(!rustup_toolchains_include_windows_gnu(output));
    }

    #[test]
    fn extracts_package_names_from_first_column() {
        let output = "git 2.0 -> 2.1\n\nnodejs 20.0 -> 22.0\n";

        assert_eq!(
            package_names_from_lines(output),
            vec!["git".to_string(), "nodejs".to_string()]
        );
    }

    #[test]
    fn recognizes_pacman_no_update_statuses() {
        assert!(pacman_status_means_no_updates(2, "", true));
        assert!(pacman_status_means_no_updates(1, "", false));
        assert!(!pacman_status_means_no_updates(1, "pacman 1 -> 2", false));
        assert!(!pacman_status_means_no_updates(0, "", true));
    }

    #[test]
    fn parses_pkg_updatable_names_and_ignores_noise() {
        let output = "Listing...\nlibc/stable 1.0 aarch64 [upgradable from: 0.9]\nWARNING: apt does not have a stable CLI interface.\nvim/now 9.0 aarch64 [upgradable from: 8.0]\n";

        assert_eq!(
            pkg_updatable_names(output),
            vec!["libc".to_string(), "vim".to_string()]
        );
        assert_eq!(pkg_updatable_name("not-a-package"), None);
    }

    #[test]
    fn nvim_count_from_output_extracts_marker_or_marks_failed() {
        let probe = &NVIM_COUNT_PROBES[0];
        let mut failed = false;
        let mut logs = Vec::new();

        assert_eq!(
            nvim_count_from_output(&mut failed, &mut logs, probe, "noise\nUPDT_LAZY_COUNT=3\n"),
            Some(3)
        );
        assert!(!failed);

        assert_eq!(
            nvim_count_from_output(&mut failed, &mut logs, probe, "noise"),
            None
        );
        assert!(failed);
    }

    #[test]
    fn adds_nvim_component_only_when_count_is_positive() {
        let probe = &NVIM_COUNT_PROBES[1];
        let mut components = Vec::new();

        add_nvim_component_if_needed(&mut components, probe, 0);
        add_nvim_component_if_needed(&mut components, probe, 2);

        assert_eq!(components, vec!["Mason tools: 2 项可更新".to_string()]);
    }
}

struct NvimAvailabilityProbe {
    args: &'static [&'static str],
    ok_marker: &'static str,
    command_error: &'static str,
    exit_label: &'static str,
}

#[derive(Clone, Copy)]
enum NvimManager {
    Lazy,
    Mason,
}

struct NvimCountProbe {
    manager: NvimManager,
    args: &'static [&'static str],
    marker: &'static str,
    component_label: &'static str,
    command_error: &'static str,
    exit_label: &'static str,
    parse_error: &'static str,
}

const NVIM_LAZY_AVAILABILITY: NvimAvailabilityProbe = NvimAvailabilityProbe {
    args: &[
        "+lua local ok=pcall(require,'lazy'); print(ok and 'UPDT_LAZY_OK' or 'UPDT_LAZY_MISSING')",
        "+qa",
    ],
    ok_marker: "UPDT_LAZY_OK",
    command_error: "检查失败: 无法启动 nvim.",
    exit_label: "Lazy",
};

const NVIM_MASON_AVAILABILITY: NvimAvailabilityProbe = NvimAvailabilityProbe {
    args: &[
        "+lua local ok=pcall(require,'mason'); print(ok and 'UPDT_MASON_OK' or 'UPDT_MASON_MISSING')",
        "+qa",
    ],
    ok_marker: "UPDT_MASON_OK",
    command_error: "检查失败: Mason 探测命令失败.",
    exit_label: "Mason",
};

static NVIM_COUNT_PROBES: &[NvimCountProbe] = &[
    NvimCountProbe {
        manager: NvimManager::Lazy,
        args: &[
            "+lua local checker=require('lazy.manage.checker'); checker.check({show=false}); vim.wait(120000, function() return not checker.running end, 200); local n=0; for _ in pairs(checker.updated or {}) do n=n+1 end; print('UPDT_LAZY_COUNT='..n)",
            "+qa",
        ],
        marker: "UPDT_LAZY_COUNT=",
        component_label: "Lazy plugins",
        command_error: "检查失败: Lazy 更新计数命令失败.",
        exit_label: "Lazy",
        parse_error: "检查失败: Lazy 计数输出解析失败.",
    },
    NvimCountProbe {
        manager: NvimManager::Mason,
        args: &[
            "+lua local reg=require('mason-registry'); local ok,pkgs=pcall(reg.get_installed_packages); if not ok then print('UPDT_MASON_COUNT=0') return end; local n=0; for _,p in ipairs(pkgs) do local ok_i,iv=pcall(p.get_installed_version,p); local ok_l,lv=pcall(p.get_latest_version,p); if ok_i and ok_l and tostring(iv)~=tostring(lv) then n=n+1 end end; print('UPDT_MASON_COUNT='..n)",
            "+qa",
        ],
        marker: "UPDT_MASON_COUNT=",
        component_label: "Mason tools",
        command_error: "检查失败: Mason 更新计数命令失败.",
        exit_label: "Mason",
        parse_error: "检查失败: Mason 计数输出解析失败.",
    },
];

async fn check_nvim_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !ensure_nvim_checkable(state, logs).await {
        return;
    }
    logs.push(log_pkg_line(
        "nvim",
        "正在检查 Lazy/Mason 可用性...",
        MsgKind::Info,
    ));

    if !probe_nvim_managers(state, logs).await {
        return;
    }
    if !nvim_has_supported_manager(state, logs) {
        return;
    }
    if !record_nvim_counts(state, logs).await {
        return;
    }

    finish_nvim_check(state, logs);
}

async fn ensure_nvim_checkable(state: &mut AppState, logs: &mut Vec<String>) -> bool {
    ensure_check_target(
        state.enable.nvim,
        &mut state.nvim.installed,
        "nvim",
        "nvim",
        logs,
    )
    .await
}

async fn probe_nvim_managers(state: &mut AppState, logs: &mut Vec<String>) -> bool {
    let Some(lazy_available) =
        probe_nvim_availability(&mut state.nvim.check_failed, logs, &NVIM_LAZY_AVAILABILITY).await
    else {
        return false;
    };
    let Some(mason_available) =
        probe_nvim_availability(&mut state.nvim.check_failed, logs, &NVIM_MASON_AVAILABILITY).await
    else {
        return false;
    };
    state.nvim.lazy_available = lazy_available;
    state.nvim.mason_available = mason_available;
    true
}

async fn probe_nvim_availability(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
    probe: &NvimAvailabilityProbe,
) -> Option<bool> {
    let Ok((status, output)) = run_nvim_headless_capture(probe.args).await else {
        mark_check_failed(check_failed, "nvim", probe.command_error, logs);
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "nvim",
            &format!("检查失败: {} 探测退出码 {status}.", probe.exit_label),
            logs,
        );
        return None;
    }
    Some(output.contains(probe.ok_marker))
}

fn nvim_has_supported_manager(state: &AppState, logs: &mut Vec<String>) -> bool {
    if state.nvim.lazy_available || state.nvim.mason_available {
        return true;
    }
    logs.push(log_pkg_line(
        "nvim",
        "未检测到 Lazy 或 Mason, 跳过.",
        MsgKind::Warn,
    ));
    false
}

async fn record_nvim_counts(state: &mut AppState, logs: &mut Vec<String>) -> bool {
    for probe in NVIM_COUNT_PROBES {
        if !nvim_count_probe_enabled(state, probe) {
            continue;
        }
        if !record_nvim_count(
            &mut state.nvim.check_failed,
            &mut state.nvim.updatable_components,
            logs,
            probe,
        )
        .await
        {
            return false;
        }
    }
    true
}

fn nvim_count_probe_enabled(state: &AppState, probe: &NvimCountProbe) -> bool {
    match probe.manager {
        NvimManager::Lazy => state.nvim.lazy_available,
        NvimManager::Mason => state.nvim.mason_available,
    }
}

async fn record_nvim_count(
    check_failed: &mut bool,
    components: &mut Vec<String>,
    logs: &mut Vec<String>,
    probe: &NvimCountProbe,
) -> bool {
    let Some(count) = load_nvim_count(check_failed, logs, probe).await else {
        return false;
    };
    add_nvim_component_if_needed(components, probe, count);
    true
}

async fn load_nvim_count(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
    probe: &NvimCountProbe,
) -> Option<usize> {
    let Ok((status, output)) = run_nvim_headless_capture(probe.args).await else {
        mark_check_failed(check_failed, "nvim", probe.command_error, logs);
        return None;
    };
    if status != 0 {
        mark_check_failed(
            check_failed,
            "nvim",
            &format!("检查失败: {} 计数退出码 {status}.", probe.exit_label),
            logs,
        );
        return None;
    }
    nvim_count_from_output(check_failed, logs, probe, &output)
}

fn nvim_count_from_output(
    check_failed: &mut bool,
    logs: &mut Vec<String>,
    probe: &NvimCountProbe,
    output: &str,
) -> Option<usize> {
    let count = extract_marker_count(output, probe.marker);
    if count.is_none() {
        mark_check_failed(check_failed, "nvim", probe.parse_error, logs);
    }
    count
}

fn add_nvim_component_if_needed(
    components: &mut Vec<String>,
    probe: &NvimCountProbe,
    count: usize,
) {
    if count > 0 {
        components.push(format!("{}: {count} 项可更新", probe.component_label));
    }
}

fn finish_nvim_check(state: &mut AppState, logs: &mut Vec<String>) {
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
    if let Some(handler) = check_target_handler(target) {
        (handler.check)(&mut local, &mut logs).await;
    }
    CheckResult {
        target: target.to_string(),
        state: local,
        logs,
    }
}

pub fn merge_check_result(state: &mut AppState, target: &str, local: AppState) {
    if let Some(handler) = check_target_handler(target) {
        (handler.merge)(state, local);
    }
}
