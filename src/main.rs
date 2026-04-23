use chrono::Local;
use clap::{Arg, Command as ClapCommand, builder::PossibleValuesParser};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::{Attribute, Color as TermColor, Stylize},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Duration;

type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

const TARGET_IDS: [&str; 8] = [
    "brew", "npm", "cargo", "rustup", "paru", "flatpak", "pacman", "pkg",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SystemProfile {
    Unknown,
    Windows,
    Macos,
    Arch,
    Termux,
}

enum CliCommand {
    Default,
    Update(Vec<String>),
    Fish,
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
    npm_updatable_packages: Vec<String>,
    cargo_installed: bool,
    cargo_has_updates: bool,
    cargo_check_failed: bool,
    cargo_updater_installed: bool,
    cargo_updatable_packages: Vec<String>,
    rustup_installed: bool,
    rustup_has_updates: bool,
    rustup_check_failed: bool,
    rustup_updatable_toolchains: Vec<String>,
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
    pkg_installed: bool,
    pkg_has_updates: bool,
    pkg_check_failed: bool,
    pkg_updatable_packages: Vec<String>,
    is_arch_linux: bool,
    is_termux: bool,
    system_profile: SystemProfile,
    enable_brew: bool,
    enable_npm: bool,
    enable_cargo: bool,
    enable_rustup: bool,
    enable_paru: bool,
    enable_pacman: bool,
    enable_flatpak: bool,
    enable_pkg: bool,
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
            npm_updatable_packages: vec![],
            cargo_installed: false,
            cargo_has_updates: false,
            cargo_check_failed: false,
            cargo_updater_installed: false,
            cargo_updatable_packages: vec![],
            rustup_installed: false,
            rustup_has_updates: false,
            rustup_check_failed: false,
            rustup_updatable_toolchains: vec![],
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
            pkg_installed: false,
            pkg_has_updates: false,
            pkg_check_failed: false,
            pkg_updatable_packages: vec![],
            is_arch_linux: false,
            is_termux: false,
            system_profile: SystemProfile::Unknown,
            enable_brew: false,
            enable_npm: false,
            enable_cargo: false,
            enable_rustup: false,
            enable_paru: false,
            enable_pacman: false,
            enable_flatpak: false,
            enable_pkg: false,
        }
    }
}

fn target_label(id: &str) -> &'static str {
    match id {
        "brew" => "Homebrew",
        "npm" => "npm",
        "cargo" => "cargo",
        "rustup" => "rustup",
        "paru" => "paru",
        "flatpak" => "flatpak",
        "pacman" => "pacman",
        "pkg" => "pkg",
        _ => "unknown",
    }
}

fn parse_cli() -> CliCommand {
    let matches = ClapCommand::new("updt")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(
            ClapCommand::new("update")
                .about("Update selected targets")
                .arg(
                    Arg::new("targets")
                        .num_args(1..)
                        .required(true)
                        .value_delimiter(',')
                        .value_parser(PossibleValuesParser::new(TARGET_IDS))
                        .help("Targets to update, for example: npm,cargo"),
                ),
        )
        .subcommand(
            ClapCommand::new("fish")
                .about("Install fish completion to ~/.config/fish/completions/updt.fish"),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("update", sub)) => CliCommand::Update(
            sub.get_many::<String>("targets")
                .map(|v| v.cloned().collect())
                .unwrap_or_default(),
        ),
        Some(("fish", _)) => CliCommand::Fish,
        _ => CliCommand::Default,
    }
}

fn print_section(title: &str) {
    println!();
    println!(
        "{}",
        color_bold(&format!("==== {title} ===="), TermColor::Cyan)
    );
}

fn color_enabled() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| io::stdout().is_terminal())
}

fn color(text: &str, c: TermColor) -> String {
    if color_enabled() {
        format!("{}", text.with(c))
    } else {
        text.to_string()
    }
}

fn color_bold(text: &str, c: TermColor) -> String {
    if color_enabled() {
        format!("{}", text.with(c).attribute(Attribute::Bold))
    } else {
        text.to_string()
    }
}

fn ok_text(text: &str) -> String {
    color(text, TermColor::Green)
}

fn warn_text(text: &str) -> String {
    color(text, TermColor::Yellow)
}

fn err_text(text: &str) -> String {
    color(text, TermColor::Red)
}

#[derive(Clone, Copy)]
enum MsgKind {
    Info,
    Ok,
    Warn,
}

fn pkg_color(pkg: &str) -> TermColor {
    let _ = pkg;
    TermColor::Cyan
}

fn log_pkg_line(pkg: &str, msg: &str, kind: MsgKind) -> String {
    let prefix = color_bold(&format!("[{pkg}]"), pkg_color(pkg));
    let body = match kind {
        MsgKind::Info => color(msg, TermColor::White),
        MsgKind::Ok => ok_text(msg),
        MsgKind::Warn => warn_text(msg),
    };
    format!("{prefix} {body}")
}

fn section_title(target: &str) -> &'static str {
    match target {
        "brew" => "Homebrew",
        "npm" => "npm (global)",
        "cargo" => "cargo",
        "rustup" => "rustup",
        "paru" => "paru (AUR)",
        "flatpak" => "flatpak",
        "pacman" => "pacman",
        "pkg" => "pkg (Termux)",
        _ => "unknown",
    }
}

fn summarize_target_status(target: &str, state: &AppState) -> (MsgKind, &'static str) {
    match target {
        "brew" => {
            if !state.enable_brew || !state.brew_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.brew_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.brew_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "npm" => {
            if !state.enable_npm || !state.npm_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.npm_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.npm_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "cargo" => {
            if !state.enable_cargo || !state.cargo_installed {
                (MsgKind::Warn, "已跳过")
            } else if !state.cargo_updater_installed {
                (MsgKind::Warn, "缺少 cargo-update")
            } else if state.cargo_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.cargo_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "rustup" => {
            if !state.enable_rustup || !state.rustup_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.rustup_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.rustup_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "paru" => {
            if !state.enable_paru || !state.paru_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.paru_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.paru_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "flatpak" => {
            if !state.enable_flatpak || !state.flatpak_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.flatpak_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.flatpak_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "pacman" => {
            if !state.enable_pacman || !state.pacman_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.pacman_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.pacman_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        "pkg" => {
            if !state.enable_pkg || !state.pkg_installed {
                (MsgKind::Warn, "已跳过")
            } else if state.pkg_check_failed {
                (MsgKind::Warn, "检查失败")
            } else if state.pkg_has_updates {
                (MsgKind::Warn, "发现可升级项")
            } else {
                (MsgKind::Ok, "当前最新")
            }
        }
        _ => (MsgKind::Warn, "未知状态"),
    }
}

fn command_exists(name: &str) -> bool {
    resolve_command_path(name).is_some()
}

fn resolve_command_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
        let path = Path::new(name);
        return is_executable(path).then(|| path.to_path_buf());
    }

    let candidates = command_name_candidates(name);
    for dir in env::split_paths(&env::var_os("PATH")?) {
        for candidate_name in &candidates {
            let candidate = dir.join(candidate_name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn command_name_candidates(name: &str) -> Vec<String> {
    let path = Path::new(name);
    if path.extension().is_some() {
        return vec![name.to_string()];
    }

    let pathext = env::var_os("PATHEXT")
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    let mut candidates = Vec::new();
    for ext in pathext
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
    {
        candidates.push(format!("{name}{ext}"));
    }
    candidates.push(name.to_string());
    candidates
}

#[cfg(not(windows))]
fn command_name_candidates(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

fn command_program(program: &str) -> PathBuf {
    resolve_command_path(program).unwrap_or_else(|| PathBuf::from(program))
}

fn command(program: &str) -> Command {
    let program_path = command_program(program);
    #[cfg(windows)]
    {
        if program_path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/D").arg("/C").arg("call").arg(program_path);
            return cmd;
        }
    }
    Command::new(program_path)
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
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn run_capture(program: &str, args: &[&str]) -> io::Result<(i32, String)> {
    let output = command(program).args(args).output()?;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((code, text))
}

fn run_inherit(program: &str, args: &[&str]) -> io::Result<bool> {
    let status = command(program)
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

fn updatable_items_for_target(state: &AppState, target: &str) -> Vec<String> {
    match target {
        "brew" => state
            .brew_formula_list
            .iter()
            .chain(state.brew_cask_list.iter())
            .cloned()
            .collect(),
        "npm" => state.npm_updatable_packages.clone(),
        "cargo" => state.cargo_updatable_packages.clone(),
        "rustup" => state.rustup_updatable_toolchains.clone(),
        "paru" => state.paru_updatable_packages.clone(),
        "flatpak" => state.flatpak_updatable_refs.clone(),
        "pacman" => state.pacman_updatable_packages.clone(),
        "pkg" => state.pkg_updatable_packages.clone(),
        _ => Vec::new(),
    }
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
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

fn print_target_updatable_items(state: &AppState, target: &str) {
    for item in updatable_items_for_target(state, target) {
        println!("  - {item}");
    }
}

fn select_targets_prompt(state: &AppState, upgradable_targets: &[String]) -> Vec<String> {
    let mut selected_targets = Vec::<String>::new();
    println!("逐项确认待升级项目.");

    for target in upgradable_targets {
        println!("{}", target_label(target));
        print_target_updatable_items(state, target);
        let message = format!("是否升级 {}", target_label(target));
        print!("{message} [y/N]: ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_ok()
            && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        {
            selected_targets.push(target.clone());
        }
    }
    selected_targets
}

fn target_row_index(upgradable_targets: &[String], target_idx: usize, state: &AppState) -> usize {
    upgradable_targets
        .iter()
        .take(target_idx)
        .map(|target| 1 + updatable_items_for_target(state, target).len())
        .sum()
}

fn select_targets_tui(
    terminal: &mut AppTerminal,
    state: &AppState,
    upgradable_targets: &[String],
) -> io::Result<Vec<String>> {
    if upgradable_targets.is_empty() {
        return Ok(Vec::new());
    }

    let mut cursor = 0usize;
    let mut selected = vec![false; upgradable_targets.len()];

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(frame.area());

            let help = Paragraph::new("Up/Down: move, Space: toggle, Enter: confirm, q/Esc: quit")
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

fn select_targets(state: &AppState, upgradable_targets: &[String]) -> Vec<String> {
    if io::stdout().is_terminal() && io::stdin().is_terminal() {
        let tui_result = (|| {
            let _guard = TerminalGuard::enter()?;
            let backend = CrosstermBackend::new(io::stdout());
            let mut terminal = Terminal::new(backend)?;
            select_targets_tui(&mut terminal, state, upgradable_targets)
        })();
        match tui_result {
            Ok(chosen) => return chosen,
            Err(err) => {
                eprintln!("[ui] TUI 初始化失败, 自动回退文本交互: {err}");
            }
        }
    }
    select_targets_prompt(state, upgradable_targets)
}

fn install_fish_completion() -> io::Result<PathBuf> {
    let Some(home) = env::var_os("HOME") else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "HOME env is not set",
        ));
    };

    let path = PathBuf::from(home)
        .join(".config")
        .join("fish")
        .join("completions")
        .join("updt.fish");

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid completion path"))?;
    fs::create_dir_all(parent)?;

    let script = r#"set -l __updt_targets brew npm cargo rustup paru flatpak pacman pkg

complete -c updt -f
complete -c updt -s h -l help -d 'Print help'
complete -c updt -s V -l version -d 'Print version'
complete -c updt -n '__fish_use_subcommand' -a 'update fish'
complete -c updt -n '__fish_seen_subcommand_from update' -x -a "$__updt_targets"
complete -c updt -n '__fish_seen_subcommand_from update' -s h -l help -d 'Print help'
"#;

    fs::write(&path, script)?;
    Ok(path)
}

fn parse_profile(state: &mut AppState) {
    let prefix = env::var("PREFIX").unwrap_or_default();
    state.is_termux = prefix.contains("com.termux")
        || Path::new("/data/data/com.termux/files/usr/bin/pkg").exists();
    state.is_arch_linux = PathBuf::from("/etc/arch-release").is_file();
    if state.is_termux {
        state.system_profile = SystemProfile::Termux;
        state.enable_pkg = true;
        state.enable_npm = true;
        state.enable_cargo = true;
        state.enable_rustup = false;
    } else if env::consts::OS == "windows" {
        state.system_profile = SystemProfile::Windows;
        state.enable_npm = true;
        state.enable_cargo = true;
        state.enable_rustup = true;
    } else if env::consts::OS == "macos" {
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
        SystemProfile::Windows => "windows",
        SystemProfile::Macos => "macos",
        SystemProfile::Arch => "arch",
        SystemProfile::Termux => "termux",
    }
}

fn target_enabled(state: &AppState, target: &str) -> bool {
    match target {
        "brew" => state.enable_brew,
        "npm" => state.enable_npm,
        "cargo" => state.enable_cargo,
        "rustup" => state.enable_rustup,
        "paru" => state.enable_paru,
        "flatpak" => state.enable_flatpak,
        "pacman" => state.enable_pacman,
        "pkg" => state.enable_pkg,
        _ => false,
    }
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

struct CheckResult {
    target: String,
    state: AppState,
    logs: Vec<String>,
}

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

fn check_brew_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_brew {
        logs.push(log_pkg_line("brew", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("brew") {
        logs.push(log_pkg_line("brew", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.brew_installed = true;
    logs.push(log_pkg_line(
        "brew",
        "正在检查可升级项 (brew outdated --greedy --json=v2)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("brew", &["outdated", "--greedy", "--json=v2"]) else {
        state.brew_check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: 无法执行 brew 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.brew_check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            &format!("检查失败 (brew outdated --greedy --json=v2, exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Some(json_text) = first_json_payload(&output) else {
        state.brew_check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: 未找到 JSON 内容.",
            MsgKind::Warn,
        ));
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        state.brew_check_failed = true;
        logs.push(log_pkg_line(
            "brew",
            "检查失败: JSON 解析失败.",
            MsgKind::Warn,
        ));
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
    if state.brew_formula_list.is_empty() {
        logs.push(log_pkg_line("brew", "Formula: 已是最新.", MsgKind::Ok));
    } else {
        state.brew_has_updates = true;
        logs.push(log_pkg_line("brew", "Formula 可升级:", MsgKind::Info));
        for p in &state.brew_formula_list {
            logs.push(format!("  - {p}"));
        }
    }
    if state.brew_cask_list.is_empty() {
        logs.push(log_pkg_line("brew", "Cask: 已是最新.", MsgKind::Ok));
    } else {
        state.brew_has_updates = true;
        logs.push(log_pkg_line("brew", "Cask 可升级:", MsgKind::Info));
        for p in &state.brew_cask_list {
            logs.push(format!("  - {p}"));
        }
    }
}

fn check_npm_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_npm {
        logs.push(log_pkg_line("npm", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("npm") {
        logs.push(log_pkg_line("npm", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.npm_installed = true;
    logs.push(log_pkg_line(
        "npm",
        "正在检查全局包更新 (npm outdated --json --global)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("npm", &["outdated", "--json", "--global"]) else {
        state.npm_check_failed = true;
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
        state.npm_check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Some(json_text) = first_json_payload(&output) else {
        state.npm_check_failed = true;
        logs.push(log_pkg_line(
            "npm",
            "检查失败: JSON 解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(json_text) else {
        state.npm_check_failed = true;
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
    state.npm_has_updates = true;
    state.npm_updatable_packages = obj.keys().cloned().collect();
    logs.push(log_pkg_line("npm", "以下全局包可升级:", MsgKind::Info));
    for name in &state.npm_updatable_packages {
        logs.push(format!("  - {name}"));
    }
}

fn check_cargo_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_cargo {
        logs.push(log_pkg_line("cargo", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("cargo") {
        logs.push(log_pkg_line("cargo", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.cargo_installed = true;
    if !command_exists("cargo-install-update") {
        logs.push(log_pkg_line(
            "cargo",
            "未安装 cargo-install-update, 跳过.",
            MsgKind::Warn,
        ));
        return;
    }
    state.cargo_updater_installed = true;
    logs.push(log_pkg_line(
        "cargo",
        "正在检查已安装 crate 更新 (cargo install-update --list)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_cargo_install_update_capture(&["--list"]) else {
        state.cargo_check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            "检查失败: 命令执行失败.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.cargo_check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    let Ok(pkgs) = parse_cargo_list(&output) else {
        state.cargo_check_failed = true;
        logs.push(log_pkg_line(
            "cargo",
            "检查失败: 输出解析失败.",
            MsgKind::Warn,
        ));
        return;
    };
    state.cargo_updatable_packages = pkgs;
    if state.cargo_updatable_packages.is_empty() {
        logs.push(log_pkg_line("cargo", "已安装 crate 已是最新.", MsgKind::Ok));
    } else {
        state.cargo_has_updates = true;
        logs.push(log_pkg_line("cargo", "以下 crate 可升级:", MsgKind::Info));
        for p in &state.cargo_updatable_packages {
            logs.push(format!("  - {p}"));
        }
    }
}

fn check_rustup_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_rustup {
        logs.push(log_pkg_line("rustup", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("rustup") {
        logs.push(log_pkg_line("rustup", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.rustup_installed = true;
    logs.push(log_pkg_line(
        "rustup",
        "正在检查 toolchain 更新 (rustup check --no-self-update)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("rustup", &["check", "--no-self-update"]) else {
        state.rustup_check_failed = true;
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
            state.rustup_has_updates = true;
            logs.push(log_pkg_line(
                "rustup",
                "以下 toolchain 可升级:",
                MsgKind::Info,
            ));
            for line in output.lines().map(str::trim).filter(|x| !x.is_empty()) {
                state.rustup_updatable_toolchains.push(line.to_string());
                logs.push(format!("  - {line}"));
            }
        }
        _ => {
            state.rustup_check_failed = true;
            logs.push(log_pkg_line(
                "rustup",
                &format!("检查失败 (exit {status})."),
                MsgKind::Warn,
            ));
        }
    }
}

fn check_paru_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_paru {
        logs.push(log_pkg_line("paru", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("paru") {
        logs.push(log_pkg_line("paru", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.paru_installed = true;
    logs.push(log_pkg_line(
        "paru",
        "正在检查 AUR 可升级项 (paru -Qua)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("paru", &["-Qua"]) else {
        state.paru_check_failed = true;
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
            // paru -Qua 在无更新时返回 1 且无输出, 这不是失败.
            logs.push(log_pkg_line("paru", "AUR 包已是最新.", MsgKind::Ok));
            return;
        }
        state.paru_check_failed = true;
        logs.push(log_pkg_line(
            "paru",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    for line in trimmed_output.lines().map(str::trim).filter(|x| !x.is_empty()) {
        if let Some(name) = first_token(line) {
            state.paru_updatable_packages.push(name);
        }
    }
    if state.paru_updatable_packages.is_empty() {
        logs.push(log_pkg_line("paru", "AUR 包已是最新.", MsgKind::Ok));
    } else {
        state.paru_has_updates = true;
        logs.push(log_pkg_line("paru", "以下 AUR 包可升级:", MsgKind::Info));
        for p in &state.paru_updatable_packages {
            logs.push(format!("  - {p}"));
        }
    }
}

fn check_flatpak_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_flatpak {
        logs.push(log_pkg_line("flatpak", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("flatpak") {
        logs.push(log_pkg_line("flatpak", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.flatpak_installed = true;
    logs.push(log_pkg_line(
        "flatpak",
        "正在检查可升级项 (flatpak remote-ls --updates --columns=application)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture(
        "flatpak",
        &["remote-ls", "--updates", "--columns=application"],
    ) else {
        state.flatpak_check_failed = true;
        logs.push(log_pkg_line(
            "flatpak",
            "检查失败: 无法执行 flatpak 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.flatpak_check_failed = true;
        logs.push(log_pkg_line(
            "flatpak",
            &format!("检查失败 (exit {status})."),
            MsgKind::Warn,
        ));
        return;
    }
    state.flatpak_updatable_refs = output
        .lines()
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if state.flatpak_updatable_refs.is_empty() {
        logs.push(log_pkg_line("flatpak", "已是最新.", MsgKind::Ok));
    } else {
        state.flatpak_has_updates = true;
        logs.push(log_pkg_line("flatpak", "以下应用可升级:", MsgKind::Info));
        for p in &state.flatpak_updatable_refs {
            logs.push(format!("  - {p}"));
        }
    }
}

fn check_pacman_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_pacman {
        logs.push(log_pkg_line("pacman", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("pacman") {
        logs.push(log_pkg_line("pacman", "未安装, 跳过.", MsgKind::Warn));
        return;
    }
    state.pacman_installed = true;
    let (status, output) = if command_exists("checkupdates") {
        run_capture("checkupdates", &[]).unwrap_or((-1, String::new()))
    } else {
        run_capture("pacman", &["-Qu"]).unwrap_or((-1, String::new()))
    };
    if status != 0 {
        if status == 2 {
            logs.push(log_pkg_line("pacman", "已是最新.", MsgKind::Ok));
        } else {
            state.pacman_check_failed = true;
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
            state.pacman_updatable_packages.push(name);
        }
    }
    if state.pacman_updatable_packages.is_empty() {
        logs.push(log_pkg_line("pacman", "已是最新.", MsgKind::Ok));
    } else {
        state.pacman_has_updates = true;
        logs.push(log_pkg_line("pacman", "以下包可升级:", MsgKind::Info));
        for p in &state.pacman_updatable_packages {
            logs.push(format!("  - {p}"));
        }
    }
}

fn check_pkg_quiet(state: &mut AppState, logs: &mut Vec<String>) {
    if !state.enable_pkg {
        logs.push(log_pkg_line("pkg", "按系统策略跳过.", MsgKind::Warn));
        return;
    }
    if !command_exists("pkg") {
        logs.push(log_pkg_line("pkg", "未安装, 跳过.", MsgKind::Warn));
        return;
    }

    state.pkg_installed = true;
    logs.push(log_pkg_line(
        "pkg",
        "正在检查可升级项 (apt list --upgradable)...",
        MsgKind::Info,
    ));
    let Ok((status, output)) = run_capture("apt", &["list", "--upgradable"]) else {
        state.pkg_check_failed = true;
        logs.push(log_pkg_line(
            "pkg",
            "检查失败: 无法执行 apt 命令.",
            MsgKind::Warn,
        ));
        return;
    };
    if status != 0 {
        state.pkg_check_failed = true;
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
            state.pkg_updatable_packages.push(name);
        }
    }

    if state.pkg_updatable_packages.is_empty() {
        logs.push(log_pkg_line("pkg", "已是最新.", MsgKind::Ok));
    } else {
        state.pkg_has_updates = true;
        logs.push(log_pkg_line("pkg", "以下包可升级:", MsgKind::Info));
        for p in &state.pkg_updatable_packages {
            logs.push(format!("  - {p}"));
        }
    }
}

fn run_single_check(target: &str) -> CheckResult {
    let mut local = AppState::default();
    let mut logs = Vec::new();
    parse_profile(&mut local);
    match target {
        "brew" => check_brew_quiet(&mut local, &mut logs),
        "npm" => check_npm_quiet(&mut local, &mut logs),
        "cargo" => check_cargo_quiet(&mut local, &mut logs),
        "rustup" => check_rustup_quiet(&mut local, &mut logs),
        "paru" => check_paru_quiet(&mut local, &mut logs),
        "flatpak" => check_flatpak_quiet(&mut local, &mut logs),
        "pacman" => check_pacman_quiet(&mut local, &mut logs),
        "pkg" => check_pkg_quiet(&mut local, &mut logs),
        _ => {}
    }
    CheckResult {
        target: target.to_string(),
        state: local,
        logs,
    }
}

fn merge_check_result(state: &mut AppState, target: &str, local: AppState) {
    match target {
        "brew" => {
            state.brew_installed = local.brew_installed;
            state.brew_has_updates = local.brew_has_updates;
            state.brew_check_failed = local.brew_check_failed;
            state.brew_formula_list = local.brew_formula_list;
            state.brew_cask_list = local.brew_cask_list;
        }
        "npm" => {
            state.npm_installed = local.npm_installed;
            state.npm_has_updates = local.npm_has_updates;
            state.npm_check_failed = local.npm_check_failed;
            state.npm_updatable_packages = local.npm_updatable_packages;
        }
        "cargo" => {
            state.cargo_installed = local.cargo_installed;
            state.cargo_has_updates = local.cargo_has_updates;
            state.cargo_check_failed = local.cargo_check_failed;
            state.cargo_updater_installed = local.cargo_updater_installed;
            state.cargo_updatable_packages = local.cargo_updatable_packages;
        }
        "rustup" => {
            state.rustup_installed = local.rustup_installed;
            state.rustup_has_updates = local.rustup_has_updates;
            state.rustup_check_failed = local.rustup_check_failed;
            state.rustup_updatable_toolchains = local.rustup_updatable_toolchains;
        }
        "paru" => {
            state.paru_installed = local.paru_installed;
            state.paru_has_updates = local.paru_has_updates;
            state.paru_check_failed = local.paru_check_failed;
            state.paru_updatable_packages = local.paru_updatable_packages;
        }
        "flatpak" => {
            state.flatpak_installed = local.flatpak_installed;
            state.flatpak_has_updates = local.flatpak_has_updates;
            state.flatpak_check_failed = local.flatpak_check_failed;
            state.flatpak_updatable_refs = local.flatpak_updatable_refs;
        }
        "pacman" => {
            state.pacman_installed = local.pacman_installed;
            state.pacman_has_updates = local.pacman_has_updates;
            state.pacman_check_failed = local.pacman_check_failed;
            state.pacman_updatable_packages = local.pacman_updatable_packages;
        }
        "pkg" => {
            state.pkg_installed = local.pkg_installed;
            state.pkg_has_updates = local.pkg_has_updates;
            state.pkg_check_failed = local.pkg_check_failed;
            state.pkg_updatable_packages = local.pkg_updatable_packages;
        }
        _ => {}
    }
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
            .cargo_updatable_packages
            .iter()
            .any(|pkg| pkg.as_str() == self_pkg);
        let targets: Vec<String> = state
            .cargo_updatable_packages
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
            println!("[cargo] 正在执行: cargo install-update {}", targets.join(" "));
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
            println!("[cargo] 即将单独升级 updt: 先退出当前 updt, 再执行 cargo install-update updt");
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
    let mut upgradable_targets = Vec::<String>::new();
    if state.brew_has_updates {
        upgradable_targets.push("brew".to_string());
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
    if state.pkg_has_updates {
        upgradable_targets.push("pkg".to_string());
    }
    upgradable_targets
}

fn resolve_cli_selection(requested: &[String], upgradable_targets: &[String]) -> Vec<String> {
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

fn any_check_failed(state: &AppState) -> bool {
    state.brew_check_failed
        || state.npm_check_failed
        || state.cargo_check_failed
        || state.rustup_check_failed
        || state.paru_check_failed
        || state.flatpak_check_failed
        || state.pacman_check_failed
        || state.pkg_check_failed
}

fn cargo_update_missing(state: &AppState) -> bool {
    state.enable_cargo
        && state.cargo_installed
        && !state.cargo_updater_installed
        && command_exists("cargo")
        && !command_exists("cargo-install-update")
}

fn confirm_default_yes(prompt: &str) -> bool {
    print!("{prompt} [Y/n]: ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

fn strip_ansi_control_sequences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek().is_some_and(|next| *next == '[') {
                let _ = chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn wait_tui_message(terminal: &mut AppTerminal, title: &str, lines: &[String]) -> io::Result<bool> {
    let clean_text = lines
        .iter()
        .map(|line| strip_ansi_control_sequences(line))
        .collect::<Vec<_>>()
        .join("\n");
    loop {
        terminal.draw(|frame| {
            let block = Paragraph::new(clean_text.as_str())
                .block(Block::default().title(title).borders(Borders::ALL));
            frame.render_widget(block, frame.area());
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

    Command::new("cmd.exe")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg("powershell.exe")
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NoExit")
        .arg("-Command")
        .arg(script)
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
            state.cargo_has_updates = false;
            state.cargo_check_failed = false;
            state.cargo_updater_installed = false;
            state.cargo_updatable_packages.clear();
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
            state.cargo_check_failed = true;
            let lines = vec![
                "cargo-update 安装失败 (退出码非 0).".to_string(),
                "".to_string(),
                "Enter: 继续    q/Esc: 继续".to_string(),
            ];
            let _ = wait_tui_message(terminal, "cargo-update", &lines)?;
        }
        Err(err) => {
            state.cargo_check_failed = true;
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
    if !confirm_default_yes("是否执行 cargo install cargo-update") {
        println!("{}", warn_text("已跳过 cargo-update 安装."));
        return;
    }

    println!("[cargo] 正在执行: cargo install cargo-update");
    match run_inherit("cargo", &["install", "cargo-update"]) {
        Ok(true) => {
            println!("[cargo] cargo-update 安装完成, 正在重新检查 cargo.");
            state.cargo_has_updates = false;
            state.cargo_check_failed = false;
            state.cargo_updater_installed = false;
            state.cargo_updatable_packages.clear();
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
            state.cargo_check_failed = true;
            println!("{}", err_text("[cargo] cargo-update 安装失败."));
        }
    }
}

fn interactive_terminal() -> bool {
    io::stdout().is_terminal() && io::stdin().is_terminal()
}

enum InteractiveResult {
    Exit(i32),
    RunUpgrade(Vec<String>),
}

fn resolve_cli_selection_quiet(
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
        let _ = wait_tui_message(&mut terminal, "汇总", &lines)?;
        return Ok(InteractiveResult::Exit(exit_code));
    }

    let selected_targets = if requested_updates.is_empty() {
        select_targets_tui(&mut terminal, state, &upgradable_targets)?
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
    lines.push("Enter: 离开 TUI 并执行升级".to_string());
    lines.push("q/Esc: 取消并退出".to_string());
    if wait_tui_message(&mut terminal, "执行升级", &lines)? {
        Ok(InteractiveResult::RunUpgrade(selected_targets))
    } else {
        Ok(InteractiveResult::Exit(0))
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
