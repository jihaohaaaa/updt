use crossterm::style::Color as TermColor;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::collections::HashMap;
use std::io;
use std::process;
use std::time::Duration;
use tokio::sync::mpsc::{self, error::TryRecvError};
use tokio::task::{JoinSet, block_in_place};
use tokio::time::sleep;

use crate::checks::{CheckResult, merge_check_result, run_single_check};
use crate::command::command_exists;
use crate::output::{MsgKind, color_bold, log_pkg_line, print_exit_signal_message, print_section};
use crate::profile::interactive_terminal;
use crate::state::{
    AppState, TARGET_IDS, profile_name, section_title, target_enabled, target_label,
    target_state_flags,
};
use crate::ui::{
    AppTerminal, TerminalGuard, interrupted_error, is_ctrl_exit_key, summarize_target_status,
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

pub fn resolve_check_targets(state: &AppState, requested: &[String]) -> Vec<String> {
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
) -> (JoinSet<CheckResult>, mpsc::UnboundedReceiver<CheckEvent>) {
    let mut set = JoinSet::new();
    let (tx, rx) = mpsc::unbounded_channel::<CheckEvent>();
    for target in targets {
        let t = target.clone();
        let tx_task = tx.clone();
        set.spawn(async move {
            let _ = tx_task.send(CheckEvent::Started(t.clone()));
            let result = run_single_check(&t).await;
            let (kind, summary) = summarize_target_status(&t, &result.state);
            let _ = tx_task.send(CheckEvent::Finished {
                target: t,
                kind,
                summary,
            });
            result
        });
    }
    drop(tx);
    (set, rx)
}

async fn finish_check_workers(
    state: &mut AppState,
    mut handles: JoinSet<CheckResult>,
) -> HashMap<String, Vec<String>> {
    let mut logs_map: HashMap<String, Vec<String>> = HashMap::new();
    while let Some(joined) = handles.join_next().await {
        if let Ok(result) = joined {
            merge_check_result(state, &result.target, result.state);
            logs_map.insert(result.target, result.logs);
        }
    }
    logs_map
}

pub async fn run_checks_plain(state: &mut AppState, targets: &[String]) {
    let (handles, mut rx) = spawn_check_workers(targets);
    println!(
        "{} {}",
        color_bold("[check]", TermColor::Yellow),
        targets
            .iter()
            .map(|t| target_label(t))
            .collect::<Vec<_>>()
            .join(", ")
    );

    while let Some(line) = rx.recv().await {
        match line {
            CheckEvent::Started(target) => {
                println!("{}", log_pkg_line(&target, "开始检查...", MsgKind::Info));
            }
            CheckEvent::Finished {
                target,
                kind,
                summary,
            } => {
                println!(
                    "{}",
                    log_pkg_line(&target, &format!("检查完成: {summary}"), kind)
                );
            }
        }
    }

    let logs_map = finish_check_workers(state, handles).await;

    for target in targets {
        print_section(section_title(target));
        if let Some(lines) = logs_map.get(target) {
            for line in lines {
                println!("{line}");
            }
        }
    }
}

pub async fn run_checks_tui(
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
    let (handles, mut rx) = spawn_check_workers(targets);

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

        tokio::task::block_in_place(|| {
            terminal.draw(|frame| {
                let chunks = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([
                        ratatui::layout::Constraint::Length(5),
                        ratatui::layout::Constraint::Min(1),
                    ])
                    .split(frame.area());
                let header = ratatui::widgets::Paragraph::new(format!(
                    "开始时间: {start_time}\n系统策略: {}\n进度: {done_count}/{}",
                    profile_name(state.system_profile),
                    rows.len()
                ))
                .block(
                    ratatui::widgets::Block::default()
                        .title("检查可升级项")
                        .borders(ratatui::widgets::Borders::ALL),
                );
                frame.render_widget(header, chunks[0]);

                let items: Vec<ratatui::widgets::ListItem> =
                    rows.iter()
                        .map(|row| {
                            let style = match row.kind {
                                MsgKind::Info => {
                                    ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)
                                }
                                MsgKind::Ok => ratatui::style::Style::default()
                                    .fg(ratatui::style::Color::Green),
                                MsgKind::Warn => ratatui::style::Style::default()
                                    .fg(ratatui::style::Color::Yellow),
                            };
                            ratatui::widgets::ListItem::new(format!(
                                "{:<10} {}",
                                target_label(&row.target),
                                row.text
                            ))
                            .style(style)
                        })
                        .collect();
                let list = ratatui::widgets::List::new(items).block(
                    ratatui::widgets::Block::default()
                        .title("目标")
                        .borders(ratatui::widgets::Borders::ALL),
                );
                frame.render_widget(list, chunks[1]);
            })
        })?;

        if done_count >= rows.len() {
            break;
        }
        let key = block_in_place(|| {
            if crossterm::event::poll(Duration::from_millis(20))?
                && let crossterm::event::Event::Key(key) = crossterm::event::read()?
            {
                return Ok::<_, io::Error>(Some(key));
            }
            Ok(None)
        })?;
        if let Some(key) = key
            && key.kind == crossterm::event::KeyEventKind::Press
            && is_ctrl_exit_key(&key)
        {
            return Err(interrupted_error());
        }
        sleep(Duration::from_millis(80)).await;
    }

    let _ = finish_check_workers(state, handles).await;
    sleep(Duration::from_millis(500)).await;
    Ok(())
}

pub async fn run_checks(state: &mut AppState, requested: &[String], start_time: &str) {
    let targets = resolve_check_targets(state, requested);
    if interactive_terminal() {
        let tui_result = async {
            let _guard = TerminalGuard::enter().await?;
            let backend = CrosstermBackend::new(io::stdout());
            let mut terminal = Terminal::new(backend)?;
            run_checks_tui(&mut terminal, state, &targets, start_time).await
        }
        .await;
        if let Err(err) = tui_result {
            if err.kind() == io::ErrorKind::Interrupted {
                print_exit_signal_message();
                process::exit(0);
            }
            eprintln!("[ui] TUI 初始化失败, 自动回退文本输出: {err}");
            run_checks_plain(state, &targets).await;
        }
    } else {
        run_checks_plain(state, &targets).await;
    }
}

pub fn build_upgradable_targets(state: &AppState) -> Vec<String> {
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

pub fn any_check_failed(state: &AppState) -> bool {
    TARGET_IDS.iter().any(|target| {
        target_state_flags(state, target)
            .map(|flags| flags.check_failed)
            .unwrap_or(false)
    })
}

pub async fn cargo_update_missing(state: &AppState) -> bool {
    state.enable.cargo
        && state.cargo.installed
        && !state.cargo.updater_installed
        && command_exists("cargo").await
        && !command_exists("cargo-install-update").await
}
