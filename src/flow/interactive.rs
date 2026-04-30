use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use crate::checks::{check_cargo_quiet, merge_check_result};
use crate::command::run_inherit;
use crate::flow::check::{
    any_check_failed, build_upgradable_targets, cargo_update_missing, resolve_check_targets,
    run_checks_tui,
};
use crate::output::{err_text, print_exit_signal_message, print_section, warn_text};
use crate::profile::{interactive_terminal, parse_profile};
use crate::selection::{confirm_default_yes, resolve_cli_selection_quiet};
use crate::state::{AppState, section_title, target_label};
use crate::ui::{
    AppTerminal, SelectionConfirmView, TerminalGuard, select_targets_tui_with_checks,
    wait_tui_float_on_selection, wait_tui_message, wait_tui_message_on_checks,
};

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

pub fn offer_install_cargo_update_tui(
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

pub fn offer_install_cargo_update(state: &mut AppState) {
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
            std::process::exit(0);
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

pub enum InteractiveResult {
    Exit(i32),
    RunUpgrade(Vec<String>),
}

pub fn run_interactive_flow(
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
