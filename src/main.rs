use chrono::Local;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    style::Color as TermColor,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::collections::HashMap;
use std::env;
use std::io;
use std::process;
#[cfg(windows)]
use std::process::Stdio;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;

mod checks;
mod cli;
mod cmd;
mod completion;
mod output;
mod profile;
mod selection;
mod state;
mod ui;

use crate::checks::{CheckResult, check_cargo_quiet, merge_check_result, run_single_check};
use crate::cli::{CliCommand, parse_cli};
use crate::cmd::{
    command_exists, run_cargo_install_update_inherit, run_inherit, run_nvim_headless_inherit,
};
use crate::completion::install_fish_completion;
use crate::output::{
    MsgKind, color_bold, err_text, log_pkg_line, ok_text, print_exit_signal_message, print_section,
    warn_text,
};
use crate::profile::{interactive_terminal, parse_profile};
use crate::selection::{
    confirm_default_yes, resolve_cli_selection, resolve_cli_selection_quiet, select_targets,
    select_targets_prompt,
};
use crate::state::{
    AppState, TARGET_IDS, profile_name, section_title, target_enabled, target_label,
    target_state_flags,
};
use crate::ui::{
    AppTerminal, SelectionConfirmView, TerminalGuard, interrupted_error, is_ctrl_exit_key,
    select_targets_tui_with_checks, summarize_target_status, wait_tui_float_on_selection,
    wait_tui_message, wait_tui_message_on_checks,
};

struct CheckRow {
    target: String,
    text: String,
    kind: MsgKind,
    done: bool,
}

enum CheckEvent {
    Started(String),
    Finished {
        target: String,
        kind: MsgKind,
        summary: &'static str,
    },
}

fn resolve_check_targets(state: &AppState, requested: &[String]) -> Vec<String> {
    if requested.is_empty() {
        TARGET_IDS
            .iter()
            .filter(|target| target_enabled(state, target))
            .map(|x| x.to_string())
            .collect()
    } else {
        let mut uniq = Vec::new();
        for t in requested {
            if !uniq.iter().any(|x: &String| x == t) {
                uniq.push(t.clone());
            }
        }
        uniq
    }
}

fn spawn_check_workers(
    targets: &[String],
) -> (
    Vec<thread::JoinHandle<CheckResult>>,
    mpsc::Receiver<CheckEvent>,
) {
    let mut handles = Vec::new();
    let (tx, rx) = mpsc::channel::<CheckEvent>();
    for target in targets {
        let t = target.clone();
        let tx_thread = tx.clone();
        handles.push(thread::spawn(move || {
            let _ = tx_thread.send(CheckEvent::Started(t.clone()));
            let result = run_single_check(&t);
            let (kind, summary) = summarize_target_status(&t, &result.state);
            let _ = tx_thread.send(CheckEvent::Finished {
                target: t,
                kind,
                summary,
            });
            result
        }));
    }
    drop(tx);
    (handles, rx)
}

fn finish_check_workers(
    state: &mut AppState,
    handles: Vec<thread::JoinHandle<CheckResult>>,
) -> HashMap<String, Vec<String>> {
    let mut logs_map: HashMap<String, Vec<String>> = HashMap::new();
    for h in handles {
        if let Ok(result) = h.join() {
            merge_check_result(state, &result.target, result.state);
            logs_map.insert(result.target, result.logs);
        }
    }
    logs_map
}

fn run_checks_plain(state: &mut AppState, targets: &[String]) {
    let mut handles = Vec::new();
    let (tx, rx) = mpsc::channel::<String>();
    println!(
        "{} {}",
        color_bold("[check]", TermColor::Yellow),
        targets
            .iter()
            .map(|t| target_label(t))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for target in targets {
        let t = target.clone();
        let tx_thread = tx.clone();
        handles.push(thread::spawn(move || {
            let _ = tx_thread.send(log_pkg_line(&t, "开始检查...", MsgKind::Info));
            let result = run_single_check(&t);
            let (kind, summary) = summarize_target_status(&t, &result.state);
            let _ = tx_thread.send(log_pkg_line(&t, &format!("检查完成: {summary}"), kind));
            result
        }));
    }
    drop(tx);

    while let Ok(line) = rx.recv() {
        println!("{line}");
    }

    let logs_map = finish_check_workers(state, handles);

    for target in targets {
        print_section(section_title(target));
        if let Some(lines) = logs_map.get(target) {
            for line in lines {
                println!("{line}");
            }
        }
    }
}

fn run_checks_tui(
    terminal: &mut AppTerminal,
    state: &mut AppState,
    targets: &[String],
    start_time: &str,
) -> io::Result<()> {
    let mut rows: Vec<CheckRow> = targets
        .iter()
        .map(|target| CheckRow {
            target: target.clone(),
            text: "等待检查".to_string(),
            kind: MsgKind::Info,
            done: false,
        })
        .collect();
    let mut done_count = 0usize;
    let (handles, rx) = spawn_check_workers(targets);

    loop {
        loop {
            match rx.try_recv() {
                Ok(CheckEvent::Started(target)) => {
                    if let Some(row) = rows.iter_mut().find(|row| row.target == target) {
                        row.text = "检查中".to_string();
                        row.kind = MsgKind::Info;
                    }
                }
                Ok(CheckEvent::Finished {
                    target,
                    kind,
                    summary,
                }) => {
                    if let Some(row) = rows.iter_mut().find(|row| row.target == target) {
                        row.text = summary.to_string();
                        row.kind = kind;
                        if !row.done {
                            row.done = true;
                            done_count += 1;
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    for row in rows.iter_mut().filter(|row| !row.done) {
                        row.text = "检查失败".to_string();
                        row.kind = MsgKind::Warn;
                        row.done = true;
                        done_count += 1;
                    }
                    break;
                }
            }
        }

        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(5), Constraint::Min(1)])
                .split(frame.area());
            let header = Paragraph::new(format!(
                "开始时间: {start_time}\n系统策略: {}\n进度: {done_count}/{}",
                profile_name(state.system_profile),
                rows.len()
            ))
            .block(Block::default().title("检查可升级项").borders(Borders::ALL));
            frame.render_widget(header, chunks[0]);

            let items: Vec<ListItem> = rows
                .iter()
                .map(|row| {
                    let style = match row.kind {
                        MsgKind::Info => Style::default().fg(Color::Cyan),
                        MsgKind::Ok => Style::default().fg(Color::Green),
                        MsgKind::Warn => Style::default().fg(Color::Yellow),
                    };
                    ListItem::new(format!("{:<10} {}", target_label(&row.target), row.text))
                        .style(style)
                })
                .collect();
            let list = List::new(items).block(Block::default().title("目标").borders(Borders::ALL));
            frame.render_widget(list, chunks[1]);
        })?;

        if done_count >= rows.len() {
            break;
        }
        if event::poll(Duration::from_millis(20))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && is_ctrl_exit_key(&key)
        {
            return Err(interrupted_error());
        }
        thread::sleep(Duration::from_millis(80));
    }

    let _ = finish_check_workers(state, handles);
    thread::sleep(Duration::from_millis(500));
    Ok(())
}

fn run_checks(state: &mut AppState, requested: &[String], start_time: &str) {
    let targets = resolve_check_targets(state, requested);
    if interactive_terminal() {
        let tui_result = (|| {
            let _guard = TerminalGuard::enter()?;
            let backend = CrosstermBackend::new(io::stdout());
            let mut terminal = Terminal::new(backend)?;
            run_checks_tui(&mut terminal, state, &targets, start_time)
        })();
        if let Err(err) = tui_result {
            if err.kind() == io::ErrorKind::Interrupted {
                print_exit_signal_message();
                process::exit(0);
            }
            eprintln!("[ui] TUI 初始化失败, 自动回退文本输出: {err}");
            run_checks_plain(state, &targets);
        }
    } else {
        run_checks_plain(state, &targets);
    }
}

fn upgrade_selected(state: &AppState, selected: &[String]) -> bool {
    print_section("执行升级");
    let mut run_fail = false;
    let self_pkg = env!("CARGO_PKG_NAME");
    let mut cargo_self_needs_update = false;

    if selected.iter().any(|s| s == "brew") {
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
        cargo_self_needs_update = state
            .cargo
            .updatable_packages
            .iter()
            .any(|pkg| pkg.as_str() == self_pkg);
        let targets: Vec<String> = state
            .cargo
            .updatable_packages
            .iter()
            .filter(|pkg| pkg.as_str() != self_pkg)
            .cloned()
            .collect();

        if targets.is_empty() {
            if cargo_self_needs_update {
                println!("[cargo] 检测到 updt 自身可升级, 将在最后单独升级.");
            } else {
                println!("[cargo] 无可升级 crate, 跳过.");
            }
        } else {
            let mut args = Vec::with_capacity(targets.len());
            for pkg in &targets {
                args.push(pkg.as_str());
            }
            println!(
                "[cargo] 正在执行: cargo install-update {}",
                targets.join(" ")
            );
            match run_cargo_install_update_inherit(&args) {
                Ok(true) => println!("[cargo] 其他已安装 crate 升级完成."),
                _ => {
                    println!("[cargo] 已安装 crate 升级失败.");
                    run_fail = true;
                }
            }
            if cargo_self_needs_update {
                println!("[cargo] updt 自身将放到最后单独升级.");
            }
        }
    }

    if selected.iter().any(|s| s == "nvim") {
        if !state.nvim.installed {
            println!("[nvim] 未安装 nvim, 跳过.");
        } else {
            if state.nvim.lazy_available {
                println!("[nvim] 正在执行: nvim --headless \"+Lazy! sync\" +qa");
                match run_nvim_headless_inherit(&["+Lazy! sync", "+qa"]) {
                    Ok(true) => println!("[nvim] Lazy 插件更新完成."),
                    _ => {
                        println!("[nvim] Lazy 插件更新失败.");
                        run_fail = true;
                    }
                }
            } else {
                println!("[nvim] 未检测到 Lazy 插件管理器, 跳过插件更新.");
            }

            if state.nvim.mason_available {
                println!(
                    "[nvim] 正在执行: nvim --headless \"+Lazy load mason.nvim\" \"+MasonUpdate\" +qa"
                );
                match run_nvim_headless_inherit(&["+Lazy load mason.nvim", "+MasonUpdate", "+qa"]) {
                    Ok(true) => println!("[nvim] Mason registry 更新完成."),
                    _ => {
                        println!("[nvim] Mason registry 更新失败.");
                        run_fail = true;
                    }
                }

                println!(
                    "[nvim] 正在执行: nvim --headless \"+Lazy load mason.nvim\" \"+lua ... MasonInstall <installed>\" +qa"
                );
                match run_nvim_headless_inherit(&[
                    "+Lazy load mason.nvim",
                    "+lua local root=vim.fn.stdpath('data')..'/mason/packages'; local ok,dir=pcall(vim.fs.dir,root); if not ok or not dir then return end; local pkgs={}; for name,t in dir do if t=='directory' then table.insert(pkgs,name) end end; table.sort(pkgs); if #pkgs>0 then vim.cmd('MasonInstall '..table.concat(pkgs,' ')) end",
                    "+qa",
                ]) {
                    Ok(true) => println!("[nvim] Mason 已安装工具更新完成."),
                    _ => {
                        println!("[nvim] Mason 已安装工具更新失败.");
                        run_fail = true;
                    }
                }
            } else {
                println!("[nvim] 未检测到 mason.nvim, 跳过 Mason 更新.");
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

    if selected.iter().any(|s| s == "fnm") {
        println!("[fnm] 正在执行: fnm install --latest");
        match run_inherit("fnm", &["install", "--latest"]) {
            Ok(true) => println!("[fnm] latest Node.js 已安装/更新."),
            _ => {
                println!("[fnm] latest Node.js 更新失败.");
                run_fail = true;
            }
        }
        println!("[fnm] 正在执行: fnm install --lts");
        match run_inherit("fnm", &["install", "--lts"]) {
            Ok(true) => println!("[fnm] LTS Node.js 已安装/更新."),
            _ => {
                println!("[fnm] LTS Node.js 更新失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "scoop") {
        println!("[scoop] 正在执行: scoop update");
        match run_inherit("scoop", &["update"]) {
            Ok(true) => {
                println!("[scoop] 正在执行: scoop update *");
                match run_inherit("scoop", &["update", "*"]) {
                    Ok(true) => println!("[scoop] 包升级完成."),
                    _ => {
                        println!("[scoop] 包升级失败.");
                        run_fail = true;
                    }
                }
            }
            _ => {
                println!("[scoop] 升级失败: scoop update 失败.");
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

    if selected.iter().any(|s| s == "pkg") {
        println!("[pkg] 正在执行: pkg update");
        match run_inherit("pkg", &["update"]) {
            Ok(true) => {
                println!("[pkg] 正在执行: pkg upgrade");
                match run_inherit("pkg", &["upgrade"]) {
                    Ok(true) => println!("[pkg] 包升级完成."),
                    _ => {
                        println!("[pkg] 包升级失败.");
                        run_fail = true;
                    }
                }
            }
            _ => {
                println!("[pkg] 升级失败: pkg update 失败.");
                run_fail = true;
            }
        }
    }

    if cargo_self_needs_update {
        #[cfg(windows)]
        {
            println!(
                "[cargo] 即将单独升级 updt: 先退出当前 updt, 再执行 cargo install-update updt"
            );
            match schedule_windows_self_update(self_pkg) {
                Ok(()) => {
                    println!("[cargo] 已启动前台自更新窗口, 本次 updt 退出后会显示升级过程.");
                }
                Err(err) => {
                    println!("[cargo] 启动前台自更新窗口失败: {err}");
                    println!("[cargo] 可手动执行: cargo install-update updt");
                    run_fail = true;
                }
            }
        }

        #[cfg(not(windows))]
        {
            println!("[cargo] 正在执行: cargo install-update updt");
            match run_cargo_install_update_inherit(&[self_pkg]) {
                Ok(true) => println!("[cargo] updt 自身升级完成."),
                _ => {
                    println!("[cargo] updt 自身升级失败.");
                    run_fail = true;
                }
            }
        }
    }

    print_section("汇总");
    println!(
        "已选择升级项: {}",
        selected
            .iter()
            .map(|id| target_label(id))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if run_fail {
        println!("{}", err_text("存在升级失败项."));
        return false;
    }
    println!("{}", ok_text("所有已选升级项执行完成."));
    true
}

fn build_upgradable_targets(state: &AppState) -> Vec<String> {
    TARGET_IDS
        .iter()
        .filter(|target| {
            target_state_flags(state, target)
                .map(|flags| flags.has_updates)
                .unwrap_or(false)
        })
        .map(|target| target.to_string())
        .collect()
}

fn any_check_failed(state: &AppState) -> bool {
    TARGET_IDS.iter().any(|target| {
        target_state_flags(state, target)
            .map(|flags| flags.check_failed)
            .unwrap_or(false)
    })
}

fn cargo_update_missing(state: &AppState) -> bool {
    state.enable.cargo
        && state.cargo.installed
        && !state.cargo.updater_installed
        && command_exists("cargo")
        && !command_exists("cargo-install-update")
}

fn run_inherit_outside_tui(
    terminal: &mut AppTerminal,
    program: &str,
    args: &[&str],
) -> io::Result<bool> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    let command_result = run_inherit(program, args);

    let mut restore_error = None;
    if let Err(err) = enable_raw_mode() {
        restore_error = Some(err);
    }
    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen)
        && restore_error.is_none()
    {
        restore_error = Some(err);
    }
    if restore_error.is_none() {
        let _ = terminal.clear();
    }

    if let Some(err) = restore_error {
        return Err(err);
    }
    command_result
}

#[cfg(windows)]
fn schedule_windows_self_update(pkg: &str) -> io::Result<()> {
    let parent_pid = process::id();
    let script = format!(
        "$ErrorActionPreference='Continue'; \
$parentPid={parent_pid}; \
while (Get-Process -Id $parentPid -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; \
cargo install-update {pkg}; \
Write-Host ''; \
Write-Host 'Self-update finished. Press Enter to close this window.'; \
[void](Read-Host)"
    );

    let shell = if command_exists("pwsh") {
        "pwsh"
    } else {
        "powershell.exe"
    };

    let primary = Command::new("cmd.exe")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(shell)
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NoExit")
        .arg("-Command")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ());

    if primary.is_ok() {
        return Ok(());
    }

    Command::new("cmd.exe")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg("powershell.exe")
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NoExit")
        .arg("-Command")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

fn offer_install_cargo_update_tui(
    terminal: &mut AppTerminal,
    state: &mut AppState,
) -> io::Result<()> {
    if !cargo_update_missing(state) {
        return Ok(());
    }

    let install = wait_tui_message(
        terminal,
        "cargo-update",
        &[
            "未安装 cargo-install-update, 无法检查已安装 crate 更新.".to_string(),
            "是否执行 cargo install cargo-update? 默认: Yes".to_string(),
            "".to_string(),
            "Enter/Y: 直连终端执行    N/q/Esc: 跳过".to_string(),
        ],
    )?;
    if !install {
        return Ok(());
    }

    let install_result = run_inherit_outside_tui(terminal, "cargo", &["install", "cargo-update"]);
    match install_result {
        Ok(true) => {
            state.cargo.has_updates = false;
            state.cargo.check_failed = false;
            state.cargo.updater_installed = false;
            state.cargo.updatable_packages.clear();
            let mut logs = Vec::new();
            let mut local = AppState::default();
            parse_profile(&mut local);
            check_cargo_quiet(&mut local, &mut logs);
            merge_check_result(state, "cargo", local);

            let mut lines = vec!["cargo-update 安装完成, 已重新检查 cargo.".to_string()];
            lines.extend(logs);
            lines.push("".to_string());
            lines.push("Enter: 继续    q/Esc: 继续".to_string());
            let _ = wait_tui_message(terminal, "cargo-update", &lines)?;
        }
        Ok(false) => {
            state.cargo.check_failed = true;
            let lines = vec![
                "cargo-update 安装失败 (退出码非 0).".to_string(),
                "".to_string(),
                "Enter: 继续    q/Esc: 继续".to_string(),
            ];
            let _ = wait_tui_message(terminal, "cargo-update", &lines)?;
        }
        Err(err) => {
            state.cargo.check_failed = true;
            let lines = vec![
                format!("cargo-update 安装失败: {err}"),
                "".to_string(),
                "Enter: 继续    q/Esc: 继续".to_string(),
            ];
            let _ = wait_tui_message(terminal, "cargo-update", &lines)?;
        }
    }
    Ok(())
}

fn offer_install_cargo_update(state: &mut AppState) {
    if !interactive_terminal() || !cargo_update_missing(state) {
        return;
    }

    print_section("cargo-update");
    println!("未安装 cargo-install-update, 无法检查已安装 crate 更新.");
    match confirm_default_yes("是否执行 cargo install cargo-update") {
        Some(true) => {}
        Some(false) => {
            println!("{}", warn_text("已跳过 cargo-update 安装."));
            return;
        }
        None => {
            print_exit_signal_message();
            process::exit(0);
        }
    }

    println!("[cargo] 正在执行: cargo install cargo-update");
    match run_inherit("cargo", &["install", "cargo-update"]) {
        Ok(true) => {
            println!("[cargo] cargo-update 安装完成, 正在重新检查 cargo.");
            state.cargo.has_updates = false;
            state.cargo.check_failed = false;
            state.cargo.updater_installed = false;
            state.cargo.updatable_packages.clear();
            let mut logs = Vec::new();
            let mut local = AppState::default();
            parse_profile(&mut local);
            check_cargo_quiet(&mut local, &mut logs);
            merge_check_result(state, "cargo", local);
            print_section(section_title("cargo"));
            for line in logs {
                println!("{line}");
            }
        }
        _ => {
            state.cargo.check_failed = true;
            println!("{}", err_text("[cargo] cargo-update 安装失败."));
        }
    }
}

enum InteractiveResult {
    Exit(i32),
    RunUpgrade(Vec<String>),
}

fn run_interactive_flow(
    state: &mut AppState,
    requested_updates: &[String],
    start_time: &str,
) -> io::Result<InteractiveResult> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let targets = resolve_check_targets(state, requested_updates);
    run_checks_tui(&mut terminal, state, &targets, start_time)?;
    offer_install_cargo_update_tui(&mut terminal, state)?;

    let upgradable_targets = build_upgradable_targets(state);
    if upgradable_targets.is_empty() {
        let mut lines = vec!["没有可升级项.".to_string()];
        let exit_code = if any_check_failed(state) {
            lines.push("但有检查失败, 请根据检查结果排查.".to_string());
            1
        } else {
            0
        };
        lines.push("".to_string());
        lines.push("Enter/q/Esc: 退出".to_string());
        let _ =
            wait_tui_message_on_checks(&mut terminal, state, &targets, start_time, "汇总", &lines)?;
        return Ok(InteractiveResult::Exit(exit_code));
    }

    loop {
        let selected_targets = if requested_updates.is_empty() {
            select_targets_tui_with_checks(
                &mut terminal,
                state,
                &upgradable_targets,
                &targets,
                start_time,
            )?
        } else {
            let (selected, skipped) =
                resolve_cli_selection_quiet(requested_updates, &upgradable_targets);
            if !skipped.is_empty() {
                let mut lines = vec!["以下请求目标当前没有可升级项:".to_string()];
                lines.extend(
                    skipped
                        .iter()
                        .map(|target| format!("  - {}", target_label(target))),
                );
                lines.push("".to_string());
                lines.push("Enter: 继续    q/Esc: 继续".to_string());
                let _ = wait_tui_message(&mut terminal, "CLI 选择", &lines)?;
            }
            selected
        };

        if selected_targets.is_empty() {
            let _ = wait_tui_message(
                &mut terminal,
                "汇总",
                &[
                    "未选择任何升级项, 已退出.".to_string(),
                    "".to_string(),
                    "Enter/q/Esc: 退出".to_string(),
                ],
            )?;
            return Ok(InteractiveResult::Exit(0));
        }

        let mut lines = vec!["已选择升级项:".to_string()];
        lines.extend(
            selected_targets
                .iter()
                .map(|target| format!("  - {}", target_label(target))),
        );
        lines.push("".to_string());
        lines.push("左右键: 选择按钮    Enter: 确认".to_string());

        let confirm_view = SelectionConfirmView {
            state,
            check_targets: &targets,
            start_time,
            upgradable_targets: &upgradable_targets,
            selected_targets: &selected_targets,
            title: "执行升级",
            lines: &lines,
        };
        if wait_tui_float_on_selection(&mut terminal, &confirm_view)? {
            return Ok(InteractiveResult::RunUpgrade(selected_targets));
        }

        if !requested_updates.is_empty() {
            return Ok(InteractiveResult::Exit(0));
        }
    }
}

fn main() {
    let cli = parse_cli();

    if matches!(cli, CliCommand::Fish) {
        match install_fish_completion() {
            Ok(path) => {
                println!("fish completion 已写入: {}", path.display());
                process::exit(0);
            }
            Err(err) => {
                eprintln!("[fish] 写入失败: {err}");
                process::exit(1);
            }
        }
    }

    let mut state = AppState::default();
    parse_profile(&mut state);
    let start_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let requested_updates = match &cli {
        CliCommand::Update(v) => v.clone(),
        _ => Vec::new(),
    };

    let mut force_text_flow = false;
    if interactive_terminal() {
        match run_interactive_flow(&mut state, &requested_updates, &start_time) {
            Ok(InteractiveResult::Exit(code)) => process::exit(code),
            Ok(InteractiveResult::RunUpgrade(selected_targets)) => {
                if upgrade_selected(&state, &selected_targets) {
                    process::exit(0);
                }
                process::exit(1);
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::Interrupted {
                    print_exit_signal_message();
                    process::exit(0);
                }
                eprintln!("[ui] TUI 运行失败, 自动回退文本流程: {err}");
                force_text_flow = true;
            }
        }
    }

    if !interactive_terminal() || force_text_flow {
        print_section("检查可升级项");
        println!(
            "{}: {}",
            color_bold("开始时间", TermColor::Blue),
            start_time
        );
        println!(
            "{}: {}",
            color_bold("系统策略", TermColor::Blue),
            profile_name(state.system_profile)
        );
    }

    if force_text_flow {
        let targets = resolve_check_targets(&state, &requested_updates);
        run_checks_plain(&mut state, &targets);
    } else {
        run_checks(&mut state, &requested_updates, &start_time);
    }
    offer_install_cargo_update(&mut state);

    let upgradable_targets = build_upgradable_targets(&state);

    if upgradable_targets.is_empty() {
        print_section("汇总");
        println!("{}", ok_text("没有可升级项."));
        if any_check_failed(&state) {
            println!("{}", warn_text("但有检查失败, 请根据上方日志排查."));
            process::exit(1);
        }
        process::exit(0);
    }

    if !interactive_terminal() {
        print_section("选择要升级的项目");
    }
    let selected_targets = if requested_updates.is_empty() {
        if force_text_flow {
            select_targets_prompt(&state, &upgradable_targets)
        } else {
            select_targets(&state, &upgradable_targets)
        }
    } else {
        resolve_cli_selection(&requested_updates, &upgradable_targets)
    };

    if selected_targets.is_empty() {
        println!("{}", warn_text("未选择任何升级项, 已退出."));
        process::exit(0);
    }

    if upgrade_selected(&state, &selected_targets) {
        process::exit(0);
    }
    process::exit(1);
}
