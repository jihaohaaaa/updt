use chrono::Local;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SystemProfile {
    Unknown,
    Macos,
    Arch,
}

struct AppState {
    brew_installed: bool,
    brew_has_updates: bool,
    brew_check_failed: bool,
    brew_formula_list: Vec<String>,
    brew_cask_list: Vec<String>,
    npm_installed: bool,
    npm_has_updates: bool,
    npm_check_failed: bool,
    cargo_installed: bool,
    cargo_has_updates: bool,
    cargo_check_failed: bool,
    cargo_updater_installed: bool,
    cargo_updatable_packages: Vec<String>,
    rustup_installed: bool,
    rustup_has_updates: bool,
    rustup_check_failed: bool,
    paru_installed: bool,
    paru_has_updates: bool,
    paru_check_failed: bool,
    paru_updatable_packages: Vec<String>,
    flatpak_installed: bool,
    flatpak_has_updates: bool,
    flatpak_check_failed: bool,
    flatpak_updatable_refs: Vec<String>,
    pacman_installed: bool,
    pacman_has_updates: bool,
    pacman_check_failed: bool,
    pacman_updatable_packages: Vec<String>,
    is_arch_linux: bool,
    system_profile: SystemProfile,
    enable_brew: bool,
    enable_npm: bool,
    enable_cargo: bool,
    enable_rustup: bool,
    enable_paru: bool,
    enable_pacman: bool,
    enable_flatpak: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            brew_installed: false,
            brew_has_updates: false,
            brew_check_failed: false,
            brew_formula_list: vec![],
            brew_cask_list: vec![],
            npm_installed: false,
            npm_has_updates: false,
            npm_check_failed: false,
            cargo_installed: false,
            cargo_has_updates: false,
            cargo_check_failed: false,
            cargo_updater_installed: false,
            cargo_updatable_packages: vec![],
            rustup_installed: false,
            rustup_has_updates: false,
            rustup_check_failed: false,
            paru_installed: false,
            paru_has_updates: false,
            paru_check_failed: false,
            paru_updatable_packages: vec![],
            flatpak_installed: false,
            flatpak_has_updates: false,
            flatpak_check_failed: false,
            flatpak_updatable_refs: vec![],
            pacman_installed: false,
            pacman_has_updates: false,
            pacman_check_failed: false,
            pacman_updatable_packages: vec![],
            is_arch_linux: false,
            system_profile: SystemProfile::Unknown,
            enable_brew: false,
            enable_npm: false,
            enable_cargo: false,
            enable_rustup: false,
            enable_paru: false,
            enable_pacman: false,
            enable_flatpak: false,
        }
    }
}

fn print_section(title: &str) {
    println!();
    println!("==== {title} ====");
}

fn prompt_yes_no(message: &str) -> bool {
    print!("{message} [y/N]: ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return is_executable(Path::new(name));
    }
    let Some(path_env) = env::var_os("PATH") else {
        return false;
    };
    for dir in env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return true;
        }
    }
    false
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        return meta.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn run_capture(program: &str, args: &[&str]) -> io::Result<(i32, String)> {
    let output = Command::new(program).args(args).output()?;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((code, text))
}

fn run_inherit(program: &str, args: &[&str]) -> io::Result<bool> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.success())
}

fn run_cargo_install_update_capture(args: &[&str]) -> io::Result<(i32, String)> {
    let mut proxy_args = vec!["install-update"];
    proxy_args.extend_from_slice(args);
    run_capture("cargo-install-update", &proxy_args)
}

fn run_cargo_install_update_inherit(args: &[&str]) -> io::Result<bool> {
    let mut proxy_args = vec!["install-update"];
    proxy_args.extend_from_slice(args);
    run_inherit("cargo-install-update", &proxy_args)
}

fn first_json_payload(output: &str) -> Option<&str> {
    let start = output.find('{')?;
    let bytes = output.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match *b {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match *b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return output.get(start..=idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_token(line: &str) -> Option<String> {
    line.split_whitespace().next().map(ToOwned::to_owned)
}

fn parse_profile(state: &mut AppState) {
    state.is_arch_linux = PathBuf::from("/etc/arch-release").is_file();
    if env::consts::OS == "macos" {
        state.system_profile = SystemProfile::Macos;
        state.enable_brew = true;
        state.enable_npm = true;
        state.enable_cargo = true;
        state.enable_rustup = true;
    } else if state.is_arch_linux {
        state.system_profile = SystemProfile::Arch;
        state.enable_npm = true;
        state.enable_cargo = true;
        state.enable_rustup = true;
        state.enable_paru = true;
        state.enable_pacman = true;
        state.enable_flatpak = true;
    }
}

fn profile_name(profile: SystemProfile) -> &'static str {
    match profile {
        SystemProfile::Unknown => "unknown",
        SystemProfile::Macos => "macos",
        SystemProfile::Arch => "arch",
    }
}

fn check_brew(state: &mut AppState) {
    print_section("Homebrew");
    if !state.enable_brew {
        println!("[brew] 按系统策略跳过.");
        return;
    }
    if !command_exists("brew") {
        println!("[brew] 未安装, 跳过.");
        return;
    }

    state.brew_installed = true;
    println!("[brew] 正在检查可升级项 (brew outdated --greedy --json=v2)...");
    let Ok((status, output)) = run_capture("brew", &["outdated", "--greedy", "--json=v2"]) else {
        state.brew_check_failed = true;
        println!("[brew] 检查失败: 无法执行 brew 命令.");
        return;
    };

    if status != 0 {
        state.brew_check_failed = true;
        println!("[brew] 检查失败 (brew outdated --greedy --json=v2, exit {status}):");
        print!("{output}");
        return;
    }

    let Some(json_text) = first_json_payload(&output) else {
        state.brew_check_failed = true;
        println!("[brew] 检查失败: 未在输出中找到 JSON 内容.");
        println!("[brew] 原始输出:");
        print!("{output}");
        return;
    };

    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        state.brew_check_failed = true;
        println!("[brew] 检查失败: 解析 brew JSON 输出失败.");
        println!("[brew] 原始输出:");
        print!("{output}");
        return;
    };

    state.brew_formula_list = root
        .get("formulae")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    state.brew_cask_list = root
        .get("casks")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|arr| arr.iter())
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();

    if !state.brew_formula_list.is_empty() {
        state.brew_has_updates = true;
        println!("[brew] Formula 可升级:");
        for pkg in &state.brew_formula_list {
            println!("  - {pkg}");
        }
    } else {
        println!("[brew] Formula: 已是最新.");
    }

    if !state.brew_cask_list.is_empty() {
        state.brew_has_updates = true;
        println!("[brew] Cask 可升级:");
        for pkg in &state.brew_cask_list {
            println!("  - {pkg}");
        }
    } else {
        println!("[brew] Cask: 已是最新.");
    }
}

fn check_npm(state: &mut AppState) {
    print_section("npm (global)");
    if !state.enable_npm {
        println!("[npm] 按系统策略跳过.");
        return;
    }
    if !command_exists("npm") {
        println!("[npm] 未安装, 跳过.");
        return;
    }

    state.npm_installed = true;
    println!("[npm] 正在检查全局包更新 (npm outdated --json --global)...");
    let Ok((status, output)) = run_capture("npm", &["outdated", "--json", "--global"]) else {
        state.npm_check_failed = true;
        println!("[npm] 检查失败: 无法执行 npm 命令.");
        return;
    };

    if status == 0 {
        println!("[npm] 全局包已是最新.");
        return;
    }

    if status == 1 {
        let Some(json_text) = first_json_payload(&output) else {
            state.npm_check_failed = true;
            println!("[npm] 检查失败: npm JSON 解析失败.");
            print!("{output}");
            return;
        };
        let Ok(root) = serde_json::from_str::<Value>(json_text) else {
            state.npm_check_failed = true;
            println!("[npm] 检查失败: npm JSON 解析失败.");
            print!("{output}");
            return;
        };
        let Some(obj) = root.as_object() else {
            println!("[npm] 全局包已是最新.");
            return;
        };
        if obj.is_empty() {
            println!("[npm] 全局包已是最新.");
            return;
        }
        state.npm_has_updates = true;
        println!("[npm] 以下全局包可升级:");
        for name in obj.keys() {
            println!("  - {name}");
        }
        return;
    }

    state.npm_check_failed = true;
    println!("[npm] 检查失败 (exit {status}):");
    print!("{output}");
}

fn parse_cargo_list(output: &str) -> Result<Vec<String>, ()> {
    let mut pkgs = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Polling registry ") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.as_slice() == ["Package", "Installed", "Latest", "Needs", "update"] {
            continue;
        }
        if parts.len() == 4 && parts[1].starts_with('v') && parts[2].starts_with('v') {
            match parts[3] {
                "Yes" => {
                    pkgs.push(parts[0].to_string());
                    continue;
                }
                "No" => continue,
                _ => {}
            }
        }
        return Err(());
    }
    Ok(pkgs)
}

fn check_cargo(state: &mut AppState) {
    print_section("cargo");
    if !state.enable_cargo {
        println!("[cargo] 按系统策略跳过.");
        return;
    }
    if !command_exists("cargo") {
        println!("[cargo] 未安装, 跳过.");
        return;
    }

    state.cargo_installed = true;
    if !command_exists("cargo-install-update") {
        println!("[cargo] 未安装 cargo-install-update, 无法检查已安装 crate 更新.");
        println!("[cargo] 可先执行: cargo install cargo-update");
        return;
    }
    let updater_exists = run_cargo_install_update_capture(&["--help"])
        .map(|(code, _)| code == 0)
        .unwrap_or(false);
    if !updater_exists {
        println!("[cargo] 未安装 cargo-install-update, 无法检查已安装 crate 更新.");
        println!("[cargo] 可先执行: cargo install cargo-update");
        return;
    }
    state.cargo_updater_installed = true;
    println!("[cargo] 正在检查已安装 crate 更新 (cargo install-update --list)...");

    let Ok((status, output)) = run_cargo_install_update_capture(&["--list"]) else {
        state.cargo_check_failed = true;
        println!("[cargo] 检查失败: 无法执行 cargo install-update 命令.");
        return;
    };

    if status != 0 {
        state.cargo_check_failed = true;
        println!("[cargo] 检查失败 (exit {status}):");
        print!("{output}");
        return;
    }

    let Ok(pkgs) = parse_cargo_list(&output) else {
        state.cargo_check_failed = true;
        println!("[cargo] 检查失败: 无法按表格格式解析 cargo install-update --list 输出.");
        println!("[cargo] 原始输出如下:");
        print!("{output}");
        return;
    };
    state.cargo_updatable_packages = pkgs;
    if !state.cargo_updatable_packages.is_empty() {
        state.cargo_has_updates = true;
        println!("[cargo] 以下 crate 可升级:");
        for pkg in &state.cargo_updatable_packages {
            println!("  - {pkg}");
        }
    } else {
        println!("[cargo] 已安装 crate 已是最新.");
    }
}

fn check_rustup(state: &mut AppState) {
    print_section("rustup");
    if !state.enable_rustup {
        println!("[rustup] 按系统策略跳过.");
        return;
    }
    if !command_exists("rustup") {
        println!("[rustup] 未安装, 跳过.");
        return;
    }

    state.rustup_installed = true;
    println!("[rustup] 正在检查 toolchain 更新 (rustup check --no-self-update)...");
    let Ok((status, output)) = run_capture("rustup", &["check", "--no-self-update"]) else {
        state.rustup_check_failed = true;
        println!("[rustup] 检查失败: 无法执行 rustup 命令.");
        return;
    };
    match status {
        0 => println!("[rustup] toolchain 已是最新."),
        100 => {
            state.rustup_has_updates = true;
            println!("[rustup] 以下 toolchain 可升级:");
            for line in output
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                println!("  - {line}");
            }
        }
        _ => {
            state.rustup_check_failed = true;
            println!("[rustup] 检查失败 (exit {status}):");
            print!("{output}");
        }
    }
}

fn check_paru(state: &mut AppState) {
    print_section("paru (AUR)");
    if !state.enable_paru {
        println!("[paru] 按系统策略跳过.");
        return;
    }
    if !command_exists("paru") {
        println!("[paru] 未安装, 跳过.");
        return;
    }

    state.paru_installed = true;
    if !state.is_arch_linux {
        println!("[paru] 检测到非 Arch Linux 环境, 将按可用命令尝试检查.");
    }
    println!("[paru] 正在检查 AUR 可升级项 (paru -Qua)...");
    let Ok((status, output)) = run_capture("paru", &["-Qua"]) else {
        state.paru_check_failed = true;
        println!("[paru] 检查失败: 无法执行 paru 命令.");
        return;
    };

    if status != 0 {
        state.paru_check_failed = true;
        println!("[paru] 检查失败 (exit {status}):");
        print!("{output}");
        return;
    }

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(name) = first_token(line) {
            state.paru_updatable_packages.push(name);
        }
    }
    if !state.paru_updatable_packages.is_empty() {
        state.paru_has_updates = true;
        println!("[paru] 以下 AUR 包可升级:");
        for pkg in &state.paru_updatable_packages {
            println!("  - {pkg}");
        }
    } else {
        println!("[paru] AUR 包已是最新.");
    }
}

fn check_flatpak(state: &mut AppState) {
    print_section("flatpak");
    if !state.enable_flatpak {
        println!("[flatpak] 按系统策略跳过.");
        return;
    }
    if !command_exists("flatpak") {
        println!("[flatpak] 未安装, 跳过.");
        return;
    }

    state.flatpak_installed = true;
    println!("[flatpak] 正在检查可升级项 (flatpak remote-ls --updates --columns=application)...");
    let Ok((status, output)) = run_capture(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application"],
    ) else {
        state.flatpak_check_failed = true;
        println!("[flatpak] 检查失败: 无法执行 flatpak 命令.");
        return;
    };

    if status != 0 {
        state.flatpak_check_failed = true;
        println!("[flatpak] 检查失败 (exit {status}):");
        print!("{output}");
        return;
    }

    state.flatpak_updatable_refs = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if !state.flatpak_updatable_refs.is_empty() {
        state.flatpak_has_updates = true;
        println!("[flatpak] 以下应用可升级:");
        for app in &state.flatpak_updatable_refs {
            println!("  - {app}");
        }
    } else {
        println!("[flatpak] 已是最新.");
    }
}

fn check_pacman(state: &mut AppState) {
    print_section("pacman");
    if !state.enable_pacman {
        println!("[pacman] 按系统策略跳过.");
        return;
    }
    if !command_exists("pacman") {
        println!("[pacman] 未安装, 跳过.");
        return;
    }

    state.pacman_installed = true;
    if !state.is_arch_linux {
        println!("[pacman] 检测到非 Arch Linux 环境, 将按可用命令尝试检查.");
    }

    let (status, output) = if command_exists("checkupdates") {
        println!("[pacman] 正在检查可升级项 (checkupdates)...");
        match run_capture("checkupdates", &[]) {
            Ok(v) => v,
            Err(_) => {
                state.pacman_check_failed = true;
                println!("[pacman] 检查失败: 无法执行 checkupdates 命令.");
                return;
            }
        }
    } else {
        println!("[pacman] 未安装 checkupdates, 回退到 pacman -Qu.");
        match run_capture("pacman", &["-Qu"]) {
            Ok(v) => v,
            Err(_) => {
                state.pacman_check_failed = true;
                println!("[pacman] 检查失败: 无法执行 pacman 命令.");
                return;
            }
        }
    };

    if status != 0 {
        if status == 2 {
            println!("[pacman] 已是最新.");
        } else {
            state.pacman_check_failed = true;
            println!("[pacman] 检查失败 (exit {status}):");
            print!("{output}");
        }
        return;
    }

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(name) = first_token(line) {
            state.pacman_updatable_packages.push(name);
        }
    }

    if !state.pacman_updatable_packages.is_empty() {
        state.pacman_has_updates = true;
        println!("[pacman] 以下包可升级:");
        for pkg in &state.pacman_updatable_packages {
            println!("  - {pkg}");
        }
    } else {
        println!("[pacman] 已是最新.");
    }
}

fn upgrade_selected(selected: &[String]) -> bool {
    print_section("执行升级");
    let mut run_fail = false;

    if selected.iter().any(|s| s == "Homebrew") {
        println!("[brew] 正在刷新索引: brew update --quiet");
        match run_inherit("brew", &["update", "--quiet"]) {
            Ok(true) => {
                println!("[brew] 正在执行: brew upgrade --greedy");
                match run_inherit("brew", &["upgrade", "--greedy"]) {
                    Ok(true) => println!("[brew] 升级完成."),
                    _ => {
                        println!("[brew] 升级失败.");
                        run_fail = true;
                    }
                }
            }
            _ => {
                println!("[brew] 升级失败: brew update 失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "npm") {
        println!("[npm] 正在执行: npm update -g");
        match run_inherit("npm", &["update", "-g"]) {
            Ok(true) => println!("[npm] 全局包升级完成."),
            _ => {
                println!("[npm] 全局包升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "cargo") {
        println!("[cargo] 正在执行: cargo install-update -a");
        match run_cargo_install_update_inherit(&["-a"]) {
            Ok(true) => println!("[cargo] 已安装 crate 升级完成."),
            _ => {
                println!("[cargo] 已安装 crate 升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "rustup") {
        println!("[rustup] 正在执行: rustup update");
        match run_inherit("rustup", &["update"]) {
            Ok(true) => println!("[rustup] toolchain 升级完成."),
            _ => {
                println!("[rustup] toolchain 升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "paru") {
        println!("[paru] 正在执行: paru -Sua");
        match run_inherit("paru", &["-Sua"]) {
            Ok(true) => println!("[paru] AUR 包升级完成."),
            _ => {
                println!("[paru] AUR 包升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "flatpak") {
        println!("[flatpak] 正在执行: flatpak update");
        match run_inherit("flatpak", &["update"]) {
            Ok(true) => println!("[flatpak] 应用升级完成."),
            _ => {
                println!("[flatpak] 应用升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "pacman") {
        println!("[pacman] 正在执行: sudo pacman -Syu");
        match run_inherit("sudo", &["pacman", "-Syu"]) {
            Ok(true) => println!("[pacman] 包升级完成."),
            _ => {
                println!("[pacman] 包升级失败.");
                run_fail = true;
            }
        }
    }

    print_section("汇总");
    println!("已选择升级项: {}", selected.join(", "));
    if run_fail {
        println!("存在升级失败项.");
        return false;
    }
    println!("所有已选升级项执行完成.");
    true
}

fn main() {
    let mut state = AppState::default();
    parse_profile(&mut state);

    print_section("检查可升级项");
    println!("开始时间: {}", Local::now().format("%Y-%m-%d %H:%M:%S"));
    println!("系统策略: {}", profile_name(state.system_profile));

    check_brew(&mut state);
    check_npm(&mut state);
    check_cargo(&mut state);
    check_rustup(&mut state);
    check_paru(&mut state);
    check_flatpak(&mut state);
    check_pacman(&mut state);

    let mut upgradable_targets = Vec::<String>::new();
    if state.brew_has_updates {
        upgradable_targets.push("Homebrew".to_string());
    }
    if state.npm_has_updates {
        upgradable_targets.push("npm".to_string());
    }
    if state.cargo_has_updates {
        upgradable_targets.push("cargo".to_string());
    }
    if state.rustup_has_updates {
        upgradable_targets.push("rustup".to_string());
    }
    if state.paru_has_updates {
        upgradable_targets.push("paru".to_string());
    }
    if state.flatpak_has_updates {
        upgradable_targets.push("flatpak".to_string());
    }
    if state.pacman_has_updates {
        upgradable_targets.push("pacman".to_string());
    }

    if upgradable_targets.is_empty() {
        print_section("汇总");
        println!("没有可升级项.");
        if state.brew_check_failed
            || state.npm_check_failed
            || state.cargo_check_failed
            || state.rustup_check_failed
            || state.paru_check_failed
            || state.flatpak_check_failed
            || state.pacman_check_failed
        {
            println!("但有检查失败, 请根据上方日志排查.");
            process::exit(1);
        }
        process::exit(0);
    }

    print_section("选择要升级的项目");
    let mut selected_targets = Vec::<String>::new();
    println!("逐项确认待升级项目.");

    for target in &upgradable_targets {
        let message = match target.as_str() {
            "Homebrew" => "是否升级 Homebrew",
            "npm" => "是否升级 npm 全局包",
            "cargo" => "是否升级 cargo 已安装 crate",
            "rustup" => "是否升级 rustup toolchain",
            "paru" => "是否升级 paru (AUR)",
            "flatpak" => "是否升级 flatpak 应用",
            "pacman" => "是否升级 pacman 包 (sudo pacman -Syu)",
            _ => continue,
        };
        if prompt_yes_no(message) {
            selected_targets.push(target.clone());
        }
    }

    if selected_targets.is_empty() {
        println!("未选择任何升级项, 已退出.");
        process::exit(0);
    }

    if upgrade_selected(&selected_targets) {
        process::exit(0);
    }
    process::exit(1);
}
