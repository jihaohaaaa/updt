use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, IsTerminal, Write};

use crate::output::print_exit_signal_message;
use crate::state::{AppState, target_label, updatable_items_for_target};
use crate::ui::{TerminalGuard, select_targets_tui};

fn print_target_updatable_items(state: &AppState, target: &str) {
    for item in updatable_items_for_target(state, target) {
        println!("  - {item}");
    }
}

pub fn select_targets_prompt(state: &AppState, upgradable_targets: &[String]) -> Vec<String> {
    let mut selected_targets = Vec::<String>::new();
    println!("逐项确认待升级项目.");

    for target in upgradable_targets {
        println!("{}", target_label(target));
        print_target_updatable_items(state, target);
        let message = format!("是否升级 {}", target_label(target));
        print!("{message} [Y/n]: ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        match io::stdin().read_line(&mut answer) {
            Ok(0) => {
                print_exit_signal_message();
                return Vec::new();
            }
            Ok(_) => {
                if matches!(
                    answer.trim().to_ascii_lowercase().as_str(),
                    "" | "y" | "yes"
                ) {
                    selected_targets.push(target.clone());
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                print_exit_signal_message();
                return Vec::new();
            }
            Err(_) => return Vec::new(),
        }
    }
    selected_targets
}

pub fn select_targets(state: &AppState, upgradable_targets: &[String]) -> Vec<String> {
    if io::stdout().is_terminal() && io::stdin().is_terminal() {
        let tui_result = (|| {
            let _guard = TerminalGuard::enter()?;
            let backend = CrosstermBackend::new(io::stdout());
            let mut terminal = Terminal::new(backend)?;
            select_targets_tui(&mut terminal, state, upgradable_targets)
        })();
        match tui_result {
            Ok(chosen) => return chosen,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                print_exit_signal_message();
                return Vec::new();
            }
            Err(err) => {
                eprintln!("[ui] TUI 初始化失败, 自动回退文本交互: {err}");
            }
        }
    }
    select_targets_prompt(state, upgradable_targets)
}

pub fn resolve_cli_selection(requested: &[String], upgradable_targets: &[String]) -> Vec<String> {
    let mut selected = Vec::<String>::new();
    for req in requested {
        if selected.iter().any(|x| x == req) {
            continue;
        }
        if upgradable_targets.iter().any(|x| x == req) {
            selected.push(req.clone());
        } else {
            println!("[cli] {} 当前没有可升级项, 跳过.", target_label(req));
        }
    }
    selected
}

pub fn resolve_cli_selection_quiet(
    requested: &[String],
    upgradable_targets: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut selected = Vec::<String>::new();
    let mut skipped = Vec::<String>::new();
    for req in requested {
        if selected.iter().any(|x| x == req) || skipped.iter().any(|x| x == req) {
            continue;
        }
        if upgradable_targets.iter().any(|x| x == req) {
            selected.push(req.clone());
        } else {
            skipped.push(req.clone());
        }
    }
    (selected, skipped)
}

pub fn confirm_default_yes(prompt: &str) -> Option<bool> {
    print!("{prompt} [Y/n]: ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    match io::stdin().read_line(&mut answer) {
        Ok(0) => return None,
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::Interrupted => return None,
        Err(_) => return None,
    }
    Some(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    ))
}
