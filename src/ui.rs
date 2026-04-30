use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::io;
use std::time::Duration;

use crate::output::MsgKind;
use crate::parse::strip_ansi_control_sequences;
use crate::state::{
    AppState, profile_name, target_label, target_state_flags, target_update_summary,
    updatable_items_for_target,
};

pub type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self { active: true })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }
}

pub fn interrupted_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "user requested exit")
}

pub fn is_ctrl_exit_key(key: &KeyEvent) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
}

pub fn summarize_target_status(target: &str, state: &AppState) -> (MsgKind, &'static str) {
    let Some(flags) = target_state_flags(state, target) else {
        return (MsgKind::Warn, "未知状态");
    };
    if !flags.enabled || !flags.installed {
        (MsgKind::Warn, "已跳过")
    } else if flags.needs_cargo_updater {
        (MsgKind::Warn, "缺少 cargo-update")
    } else if flags.check_failed {
        (MsgKind::Warn, "检查失败")
    } else if flags.has_updates {
        (MsgKind::Warn, target_update_summary(target))
    } else {
        (MsgKind::Ok, "当前最新")
    }
}

fn target_row_index(upgradable_targets: &[String], target_idx: usize, state: &AppState) -> usize {
    upgradable_targets
        .iter()
        .take(target_idx)
        .map(|target| 1 + updatable_items_for_target(state, target).len())
        .sum()
}

pub fn select_targets_tui(
    terminal: &mut AppTerminal,
    state: &AppState,
    upgradable_targets: &[String],
) -> io::Result<Vec<String>> {
    select_targets_tui_with_checks(terminal, state, upgradable_targets, &[], "")
}

pub fn select_targets_tui_with_checks(
    terminal: &mut AppTerminal,
    state: &AppState,
    upgradable_targets: &[String],
    check_targets: &[String],
    start_time: &str,
) -> io::Result<Vec<String>> {
    if upgradable_targets.is_empty() {
        return Ok(Vec::new());
    }

    let mut cursor = 0usize;
    let mut selected = vec![true; upgradable_targets.len()];

    loop {
        terminal.draw(|frame| {
            let show_checks = !check_targets.is_empty() && !start_time.is_empty();
            if show_checks {
                let area = frame.area();
                let targets_height =
                    ((check_targets.len() as u16) + 2).clamp(3, area.height.saturating_sub(7));
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(5),
                        Constraint::Length(targets_height),
                        Constraint::Length(3),
                        Constraint::Min(1),
                    ])
                    .split(area);

                let header = Paragraph::new(format!(
                    "开始时间: {start_time}\n系统策略: {}\n进度: {}/{}",
                    profile_name(state.system_profile),
                    check_targets.len(),
                    check_targets.len()
                ))
                .block(Block::default().title("检查可升级项").borders(Borders::ALL));
                frame.render_widget(header, chunks[0]);

                let target_items: Vec<ListItem> = check_targets
                    .iter()
                    .map(|target| {
                        let (kind, summary) = summarize_target_status(target, state);
                        let style = match kind {
                            MsgKind::Info => Style::default().fg(Color::Cyan),
                            MsgKind::Ok => Style::default().fg(Color::Green),
                            MsgKind::Warn => Style::default().fg(Color::Yellow),
                        };
                        ListItem::new(format!("{:<10} {}", target_label(target), summary))
                            .style(style)
                    })
                    .collect();
                let target_list = List::new(target_items)
                    .block(Block::default().title("目标").borders(Borders::ALL));
                frame.render_widget(target_list, chunks[1]);

                let help =
                    Paragraph::new("Up/Down: move, Space: toggle, Enter: confirm, q/Esc: quit")
                        .block(Block::default().title("updt").borders(Borders::ALL));
                frame.render_widget(help, chunks[2]);

                let items: Vec<ListItem> = upgradable_targets
                    .iter()
                    .enumerate()
                    .flat_map(|(idx, item)| {
                        let mark = if selected[idx] { "[x]" } else { "[ ]" };
                        let mut rows = vec![
                            ListItem::new(format!("{mark} {}", target_label(item)))
                                .style(Style::default().add_modifier(Modifier::BOLD)),
                        ];
                        rows.extend(
                            updatable_items_for_target(state, item)
                                .into_iter()
                                .map(|pkg| ListItem::new(format!("    - {pkg}"))),
                        );
                        rows
                    })
                    .collect();

                let list = List::new(items)
                    .block(
                        Block::default()
                            .title("选择要升级的项目")
                            .borders(Borders::ALL),
                    )
                    .highlight_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol(">> ");

                let mut list_state = ListState::default();
                list_state.select(Some(target_row_index(upgradable_targets, cursor, state)));
                frame.render_stateful_widget(list, chunks[3], &mut list_state);
            } else {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(1)])
                    .split(frame.area());

                let help =
                    Paragraph::new("Up/Down: move, Space: toggle, Enter: confirm, q/Esc: quit")
                        .block(Block::default().title("updt").borders(Borders::ALL));
                frame.render_widget(help, chunks[0]);

                let items: Vec<ListItem> = upgradable_targets
                    .iter()
                    .enumerate()
                    .flat_map(|(idx, item)| {
                        let mark = if selected[idx] { "[x]" } else { "[ ]" };
                        let mut rows = vec![
                            ListItem::new(format!("{mark} {}", target_label(item)))
                                .style(Style::default().add_modifier(Modifier::BOLD)),
                        ];
                        rows.extend(
                            updatable_items_for_target(state, item)
                                .into_iter()
                                .map(|pkg| ListItem::new(format!("  - {pkg}"))),
                        );
                        rows
                    })
                    .collect();

                let list = List::new(items)
                    .block(
                        Block::default()
                            .title("选择要升级的项目")
                            .borders(Borders::ALL),
                    )
                    .highlight_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol(">> ");

                let mut list_state = ListState::default();
                list_state.select(Some(target_row_index(upgradable_targets, cursor, state)));
                frame.render_stateful_widget(list, chunks[1], &mut list_state);
            }
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if is_ctrl_exit_key(&key) {
            return Err(interrupted_error());
        }

        match key.code {
            KeyCode::Up => {
                cursor = cursor.saturating_sub(1);
            }
            KeyCode::Down if cursor + 1 < upgradable_targets.len() => {
                cursor += 1;
            }
            KeyCode::Char(' ') => {
                selected[cursor] = !selected[cursor];
            }
            KeyCode::Enter => {
                let chosen = upgradable_targets
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, item)| {
                        if selected[idx] {
                            Some(item.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                return Ok(chosen);
            }
            KeyCode::Esc | KeyCode::Char('q') => return Ok(Vec::new()),
            _ => {}
        }
    }
}

pub fn wait_tui_message(
    terminal: &mut AppTerminal,
    title: &str,
    lines: &[String],
) -> io::Result<bool> {
    let clean_lines = lines
        .iter()
        .map(|line| strip_ansi_control_sequences(line))
        .collect::<Vec<_>>();
    let body = clean_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  ");
    let status = format!("[{title}] {body}");
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let footer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area)[1];
            frame.render_widget(Clear, footer);
            frame.render_widget(
                Paragraph::new(status.as_str()).style(Style::default().fg(Color::Yellow)),
                footer,
            );
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if is_ctrl_exit_key(&key) {
            return Err(interrupted_error());
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
            KeyCode::Esc
            | KeyCode::Char('q')
            | KeyCode::Char('Q')
            | KeyCode::Char('n')
            | KeyCode::Char('N') => return Ok(false),
            _ => {}
        }
    }
}

pub fn wait_tui_message_on_checks(
    terminal: &mut AppTerminal,
    state: &AppState,
    targets: &[String],
    start_time: &str,
    title: &str,
    lines: &[String],
) -> io::Result<bool> {
    let clean_lines = lines
        .iter()
        .map(|line| strip_ansi_control_sequences(line))
        .collect::<Vec<_>>();
    let body = clean_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  ");
    let status = format!("[{title}] {body}");

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(frame.area());

            let header = Paragraph::new(format!(
                "开始时间: {start_time}\n系统策略: {}\n进度: {}/{}",
                profile_name(state.system_profile),
                targets.len(),
                targets.len()
            ))
            .block(Block::default().title("检查可升级项").borders(Borders::ALL));
            frame.render_widget(header, chunks[0]);

            let items: Vec<ListItem> = targets
                .iter()
                .map(|target| {
                    let (kind, summary) = summarize_target_status(target, state);
                    let style = match kind {
                        MsgKind::Info => Style::default().fg(Color::Cyan),
                        MsgKind::Ok => Style::default().fg(Color::Green),
                        MsgKind::Warn => Style::default().fg(Color::Yellow),
                    };
                    ListItem::new(format!("{:<10} {}", target_label(target), summary)).style(style)
                })
                .collect();
            let list = List::new(items).block(Block::default().title("目标").borders(Borders::ALL));
            frame.render_widget(list, chunks[1]);

            frame.render_widget(Clear, chunks[2]);
            frame.render_widget(
                Paragraph::new(status.as_str()).style(Style::default().fg(Color::Yellow)),
                chunks[2],
            );
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if is_ctrl_exit_key(&key) {
            return Err(interrupted_error());
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
            KeyCode::Esc
            | KeyCode::Char('q')
            | KeyCode::Char('Q')
            | KeyCode::Char('n')
            | KeyCode::Char('N') => return Ok(false),
            _ => {}
        }
    }
}

pub struct SelectionConfirmView<'a> {
    pub state: &'a AppState,
    pub check_targets: &'a [String],
    pub start_time: &'a str,
    pub upgradable_targets: &'a [String],
    pub selected_targets: &'a [String],
    pub title: &'a str,
    pub lines: &'a [String],
}

pub fn wait_tui_float_on_selection(
    terminal: &mut AppTerminal,
    view: &SelectionConfirmView<'_>,
) -> io::Result<bool> {
    let clean_lines = view
        .lines
        .iter()
        .map(|line| strip_ansi_control_sequences(line))
        .collect::<Vec<_>>();
    let mut confirm_selected = true;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let targets_height =
                ((view.check_targets.len() as u16) + 2).clamp(3, area.height.saturating_sub(7));
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(targets_height),
                    Constraint::Length(3),
                    Constraint::Min(1),
                ])
                .split(area);

            let header = Paragraph::new(format!(
                "开始时间: {}\n系统策略: {}\n进度: {}/{}",
                view.start_time,
                profile_name(view.state.system_profile),
                view.check_targets.len(),
                view.check_targets.len()
            ))
            .block(Block::default().title("检查可升级项").borders(Borders::ALL));
            frame.render_widget(header, chunks[0]);

            let target_items: Vec<ListItem> = view
                .check_targets
                .iter()
                .map(|target| {
                    let (kind, summary) = summarize_target_status(target, view.state);
                    let style = match kind {
                        MsgKind::Info => Style::default().fg(Color::Cyan),
                        MsgKind::Ok => Style::default().fg(Color::Green),
                        MsgKind::Warn => Style::default().fg(Color::Yellow),
                    };
                    ListItem::new(format!("{:<10} {}", target_label(target), summary)).style(style)
                })
                .collect();
            let target_list =
                List::new(target_items).block(Block::default().title("目标").borders(Borders::ALL));
            frame.render_widget(target_list, chunks[1]);

            let help = Paragraph::new("Up/Down: move, Space: toggle, Enter: confirm, q/Esc: quit")
                .block(Block::default().title("updt").borders(Borders::ALL));
            frame.render_widget(help, chunks[2]);

            let items: Vec<ListItem> = view
                .upgradable_targets
                .iter()
                .map(|item| {
                    let checked = view.selected_targets.iter().any(|t| t == item);
                    let mark = if checked { "[x]" } else { "[ ]" };
                    ListItem::new(format!("{mark} {}", target_label(item)))
                })
                .collect();
            let list = List::new(items).block(
                Block::default()
                    .title("选择要升级的项目")
                    .borders(Borders::ALL),
            );
            frame.render_widget(list, chunks[3]);

            let popup_height =
                ((view.lines.len() as u16) + 2).clamp(3, area.height.saturating_sub(2));
            let popup_width = area.width.saturating_mul(80) / 100;
            let v = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(area.height.saturating_sub(popup_height) / 2),
                    Constraint::Length(popup_height),
                    Constraint::Min(0),
                ])
                .split(area);
            let h = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(area.width.saturating_sub(popup_width) / 2),
                    Constraint::Length(popup_width),
                    Constraint::Min(0),
                ])
                .split(v[1]);
            let popup = h[1];

            frame.render_widget(Clear, popup);
            let block = Block::default().title(view.title).borders(Borders::ALL);
            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(inner);
            let body_text = clean_lines.join("\n");
            frame.render_widget(Paragraph::new(body_text), inner_chunks[0]);

            let (confirm_style, cancel_style) = if confirm_selected {
                (
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(Color::Gray),
                )
            } else {
                (
                    Style::default().fg(Color::Gray),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            };
            let buttons = Line::from(vec![
                Span::raw("  "),
                Span::styled("[ 确认 ]", confirm_style),
                Span::raw("    "),
                Span::styled("[ 取消 ]", cancel_style),
            ]);
            frame.render_widget(
                Paragraph::new(buttons).alignment(ratatui::layout::Alignment::Center),
                inner_chunks[1],
            );
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if is_ctrl_exit_key(&key) {
            return Err(interrupted_error());
        }
        match key.code {
            KeyCode::Left => confirm_selected = true,
            KeyCode::Right | KeyCode::Tab => confirm_selected = false,
            KeyCode::Enter => return Ok(confirm_selected),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(false),
            _ => {}
        }
    }
}
