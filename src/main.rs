use chrono::Local;
use crossterm::style::Color as TermColor;
use std::io;
use std::process;

mod checks;
mod cli;
mod command;
mod completion;
mod flow;
mod output;
mod parse;
mod profile;
mod selection;
mod state;
mod ui;
mod upgrade;

use crate::cli::{CliCommand, parse_cli};
use crate::completion::install_fish_completion;
use crate::flow::check::{
    any_check_failed, build_upgradable_targets, resolve_check_targets, run_checks, run_checks_plain,
};
use crate::flow::interactive::{
    InteractiveResult, offer_install_cargo_update, run_interactive_flow,
};
use crate::output::{color_bold, ok_text, print_exit_signal_message, print_section, warn_text};
use crate::profile::{interactive_terminal, parse_profile};
use crate::selection::{resolve_cli_selection, select_targets, select_targets_prompt};
use crate::state::{AppState, profile_name};
use crate::upgrade::upgrade_selected;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let cli = parse_cli();

    if matches!(cli, CliCommand::Fish) {
        match install_fish_completion().await {
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
    parse_profile(&mut state).await;
    let start_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let requested_updates = match &cli {
        CliCommand::Update(v) => v.clone(),
        _ => Vec::new(),
    };

    let mut force_text_flow = false;
    if interactive_terminal() {
        match run_interactive_flow(&mut state, &requested_updates, &start_time).await {
            Ok(InteractiveResult::Exit(code)) => process::exit(code),
            Ok(InteractiveResult::RunUpgrade(selected_targets)) => {
                if upgrade_selected(&state, &selected_targets).await {
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
        run_checks_plain(&mut state, &targets).await;
    } else {
        run_checks(&mut state, &requested_updates, &start_time).await;
    }
    offer_install_cargo_update(&mut state).await;

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
            select_targets_prompt(&state, &upgradable_targets).await
        } else {
            select_targets(&state, &upgradable_targets).await
        }
    } else {
        resolve_cli_selection(&requested_updates, &upgradable_targets)
    };

    if selected_targets.is_empty() {
        println!("{}", warn_text("未选择任何升级项, 已退出."));
        process::exit(0);
    }

    if upgrade_selected(&state, &selected_targets).await {
        process::exit(0);
    }
    process::exit(1);
}
