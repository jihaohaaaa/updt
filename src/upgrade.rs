use crate::command::{
    command_exists, run_capture, run_cargo_install_update_inherit, run_inherit,
    run_nvim_headless_inherit,
};
use crate::output::{err_text, ok_text, print_section};
use crate::parse::{
    ScoopBlockedProcessInfo, ScoopListItem, parse_scoop_blocked_process_output,
    parse_scoop_list_output, strip_ansi_control_sequences,
};
use crate::profile::{desktop_linux_session, interactive_terminal};
use crate::selection::{ScoopBlockedAction, prompt_scoop_blocked_action};
use crate::state::{AppState, target_label};
use std::collections::{HashMap, HashSet};
use std::{env, io, process};
use tokio::fs;

#[cfg(windows)]
use std::io::Write as _;
#[cfg(windows)]
use std::process::{Command, Stdio};

pub async fn upgrade_selected(state: &AppState, selected: &[String]) -> bool {
    print_section("执行升级");
    let mut run_fail = false;
    let self_pkg = env!("CARGO_PKG_NAME");
    let mut cargo_self_needs_update = false;
    let pacman_selected = selected.iter().any(|s| s == "pacman");
    let run_pacman_first = state.is_arch_linux && pacman_selected;

    if run_pacman_first {
        run_fail |= !run_pacman_upgrade(state).await;
    }

    if selected.iter().any(|s| s == "brew") {
        println!("[brew] 正在刷新索引: brew update --quiet");
        match run_inherit("brew", &["update", "--quiet"]).await {
            Ok(true) => {
                println!("[brew] 正在执行: brew upgrade --greedy");
                match run_inherit("brew", &["upgrade", "--greedy"]).await {
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
        match run_inherit("npm", &["update", "-g"]).await {
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
                "[cargo] 正在执行: cargo install-update --locked {}",
                targets.join(" ")
            );
            match run_cargo_install_update_inherit(&args).await {
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
                match run_nvim_headless_inherit(&["+Lazy! sync", "+qa"]).await {
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
                match run_nvim_headless_inherit(&["+Lazy load mason.nvim", "+MasonUpdate", "+qa"])
                    .await
                {
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
                ])
                .await
                {
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
        match run_inherit("rustup", &["update"]).await {
            Ok(true) => println!("[rustup] toolchain 升级完成."),
            _ => {
                println!("[rustup] toolchain 升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "fnm") {
        println!("[fnm] 正在执行: fnm install --latest");
        match run_inherit("fnm", &["install", "--latest"]).await {
            Ok(true) => println!("[fnm] latest Node.js 已安装/更新."),
            _ => {
                println!("[fnm] latest Node.js 更新失败.");
                run_fail = true;
            }
        }
        println!("[fnm] 正在执行: fnm install --lts");
        match run_inherit("fnm", &["install", "--lts"]).await {
            Ok(true) => println!("[fnm] LTS Node.js 已安装/更新."),
            _ => {
                println!("[fnm] LTS Node.js 更新失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "scoop") && !upgrade_scoop_packages(state).await {
        run_fail = true;
    }

    if selected.iter().any(|s| s == "paru") {
        println!("[paru] 正在执行: paru -Sua");
        match run_inherit("paru", &["-Sua"]).await {
            Ok(true) => println!("[paru] AUR 包升级完成."),
            _ => {
                println!("[paru] AUR 包升级失败.");
                run_fail = true;
            }
        }
    }

    if selected.iter().any(|s| s == "flatpak") {
        println!("[flatpak] 正在执行: flatpak update");
        match run_inherit("flatpak", &["update"]).await {
            Ok(true) => println!("[flatpak] 应用升级完成."),
            _ => {
                println!("[flatpak] 应用升级失败.");
                run_fail = true;
            }
        }
    }

    if pacman_selected && !run_pacman_first {
        run_fail |= !run_pacman_upgrade(state).await;
    }

    if selected.iter().any(|s| s == "pkg") {
        println!("[pkg] 正在执行: pkg update");
        match run_inherit("pkg", &["update"]).await {
            Ok(true) => {
                println!("[pkg] 正在执行: pkg upgrade");
                match run_inherit("pkg", &["upgrade"]).await {
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
                "[cargo] 即将单独升级 updt: 先退出当前 updt, 再执行 cargo install-update --locked updt"
            );
            match schedule_windows_self_update(self_pkg).await {
                Ok(()) => {
                    println!("[cargo] 已启动前台自更新窗口, 本次 updt 退出后会显示升级过程.");
                }
                Err(err) => {
                    println!("[cargo] 启动前台自更新窗口失败: {err}");
                    println!("[cargo] 可手动执行: cargo install-update --locked updt");
                    run_fail = true;
                }
            }
        }

        #[cfg(not(windows))]
        {
            println!("[cargo] 正在执行: cargo install-update --locked updt");
            match run_cargo_install_update_inherit(&[self_pkg]).await {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ScoopInstallScope {
    Local,
    Global,
}

impl ScoopInstallScope {
    fn suffix(self) -> &'static str {
        match self {
            Self::Local => "",
            Self::Global => " (global)",
        }
    }

    fn update_args(self, app: &str) -> Vec<String> {
        let mut args = vec!["update".to_string(), app.to_string()];
        if matches!(self, Self::Global) {
            args.push("--global".to_string());
        }
        args
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScoopUpdateTask {
    app: String,
    scope: ScoopInstallScope,
}

impl ScoopUpdateTask {
    fn display_name(&self) -> String {
        format!("{}{}", self.app, self.scope.suffix())
    }

    fn update_args(&self) -> Vec<String> {
        self.scope.update_args(&self.app)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScoopTaskOutcome {
    Updated,
    SkippedInUse,
    FailedOther,
    Aborted,
}

fn build_scoop_update_tasks(
    updatable_items: &[String],
    installed_items: &[ScoopListItem],
) -> Vec<ScoopUpdateTask> {
    let mut scopes_by_name = HashMap::<&str, Vec<ScoopInstallScope>>::new();
    for item in installed_items {
        let scope = if item.is_global {
            ScoopInstallScope::Global
        } else {
            ScoopInstallScope::Local
        };
        scopes_by_name
            .entry(item.name.as_str())
            .or_default()
            .push(scope);
    }

    let mut tasks = Vec::new();
    let mut seen = HashSet::<(String, ScoopInstallScope)>::new();
    let mut handled_apps = HashSet::<String>::new();

    for app in updatable_items {
        if !handled_apps.insert(app.clone()) {
            continue;
        }

        let scopes = scopes_by_name
            .get(app.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[ScoopInstallScope::Local]);

        for scope in [ScoopInstallScope::Local, ScoopInstallScope::Global] {
            if !scopes.contains(&scope) {
                continue;
            }
            if seen.insert((app.clone(), scope)) {
                tasks.push(ScoopUpdateTask {
                    app: app.clone(),
                    scope,
                });
            }
        }
    }

    tasks
}

async fn upgrade_scoop_packages(state: &AppState) -> bool {
    let metadata_args = vec!["update".to_string()];
    println!("[scoop] 正在执行: {}", scoop_command_label(&metadata_args));
    match run_scoop_inherit(&metadata_args).await {
        Ok(true) => {}
        Ok(false) => {
            println!(
                "[scoop] 升级失败: {} 失败.",
                scoop_command_label(&metadata_args)
            );
            return false;
        }
        Err(err) => {
            println!("[scoop] 升级失败: {err}");
            return false;
        }
    }

    let installed_items = match load_scoop_installed_items().await {
        Ok(items) => items,
        Err(err) => {
            println!("[scoop] 无法解析已安装包列表: {err}");
            return false;
        }
    };
    let tasks = build_scoop_update_tasks(&state.scoop.updatable_items, &installed_items);
    if tasks.is_empty() {
        println!("[scoop] 未解析到待升级包任务.");
        return false;
    }

    let allow_prompt = interactive_terminal();
    let mut updated = Vec::new();
    let mut skipped_in_use = Vec::new();
    let mut failed_other = Vec::new();
    let mut aborted = false;

    for task in tasks {
        match run_scoop_update_task(&task, allow_prompt).await {
            ScoopTaskOutcome::Updated => updated.push(task.display_name()),
            ScoopTaskOutcome::SkippedInUse => skipped_in_use.push(task.display_name()),
            ScoopTaskOutcome::FailedOther => failed_other.push(task.display_name()),
            ScoopTaskOutcome::Aborted => {
                aborted = true;
                break;
            }
        }
    }

    if !updated.is_empty() {
        println!("[scoop] 已更新 {} 个包.", updated.len());
    }
    if !skipped_in_use.is_empty() {
        println!("[scoop] 以下包因运行中进程未完成更新:");
        for item in &skipped_in_use {
            println!("  - {item}");
        }
    }
    if !failed_other.is_empty() {
        println!("[scoop] 以下包更新失败:");
        for item in &failed_other {
            println!("  - {item}");
        }
    }
    if aborted {
        println!("[scoop] 用户中止了后续 Scoop 包更新.");
    }

    !aborted && skipped_in_use.is_empty() && failed_other.is_empty()
}

async fn load_scoop_installed_items() -> io::Result<Vec<ScoopListItem>> {
    let (status, output) = run_capture("scoop", &["list"]).await?;
    if status != 0 {
        return Err(io::Error::other(format!("scoop list 失败 (exit {status})")));
    }
    parse_scoop_list_output(&output).map_err(|_| io::Error::other("scoop list 输出解析失败"))
}

async fn run_scoop_update_task(task: &ScoopUpdateTask, allow_prompt: bool) -> ScoopTaskOutcome {
    loop {
        let args = task.update_args();
        println!("[scoop] 正在执行: {}", scoop_command_label(&args));

        let (status, output) = match run_scoop_capture(&args).await {
            Ok(result) => result,
            Err(err) => {
                println!("[scoop] {} 更新失败: {err}", task.display_name());
                return ScoopTaskOutcome::FailedOther;
            }
        };

        print_captured_command_output(&output);

        if let Some(blocked) = parse_scoop_blocked_process_output(&output) {
            if !allow_prompt {
                println!("[scoop] {} 因运行中进程被跳过.", task.display_name());
                return ScoopTaskOutcome::SkippedInUse;
            }

            notify_scoop_blocked(task, &blocked).await;
            match prompt_scoop_blocked_action(&task.display_name(), &blocked.details).await {
                ScoopBlockedAction::KillAndRetry => {
                    if let Err(err) = kill_scoop_task_processes(task, &blocked).await {
                        println!("[scoop] 结束 {} 关联进程失败: {err}", task.display_name());
                    }
                    println!("[scoop] 正在重试 {}...", task.display_name());
                    continue;
                }
                ScoopBlockedAction::Skip => return ScoopTaskOutcome::SkippedInUse,
                ScoopBlockedAction::Abort => return ScoopTaskOutcome::Aborted,
            }
        }

        if status == 0 {
            return ScoopTaskOutcome::Updated;
        }

        println!("[scoop] {} 更新失败 (exit {status}).", task.display_name());
        return ScoopTaskOutcome::FailedOther;
    }
}

fn print_captured_command_output(output: &str) {
    if output.is_empty() {
        return;
    }
    print!("{output}");
    if !output.ends_with('\n') {
        println!();
    }
}

fn scoop_command_label(args: &[String]) -> String {
    #[cfg(windows)]
    let program = "gsudo scoop";
    #[cfg(not(windows))]
    let program = "scoop";

    if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

async fn run_scoop_inherit(args: &[String]) -> io::Result<bool> {
    #[cfg(windows)]
    {
        if !command_exists("gsudo").await {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Windows Scoop 更新需要 gsudo, 但未找到 gsudo",
            ));
        }

        let mut elevated_args = Vec::with_capacity(args.len() + 1);
        elevated_args.push("scoop".to_string());
        elevated_args.extend_from_slice(args);
        run_inherit("gsudo", &borrow_string_args(&elevated_args)).await
    }

    #[cfg(not(windows))]
    {
        run_inherit("scoop", &borrow_string_args(args)).await
    }
}

async fn run_scoop_capture(args: &[String]) -> io::Result<(i32, String)> {
    #[cfg(windows)]
    {
        if !command_exists("gsudo").await {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Windows Scoop 更新需要 gsudo, 但未找到 gsudo",
            ));
        }

        let mut elevated_args = Vec::with_capacity(args.len() + 1);
        elevated_args.push("scoop".to_string());
        elevated_args.extend_from_slice(args);
        run_capture("gsudo", &borrow_string_args(&elevated_args)).await
    }

    #[cfg(not(windows))]
    {
        run_capture("scoop", &borrow_string_args(args)).await
    }
}

fn borrow_string_args(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

#[cfg(windows)]
async fn kill_scoop_task_processes(
    task: &ScoopUpdateTask,
    _blocked: &ScoopBlockedProcessInfo,
) -> io::Result<()> {
    let app_dir = resolve_scoop_app_dir(task).await?;
    let shell = if command_exists("pwsh").await {
        "pwsh"
    } else {
        "powershell.exe"
    };
    let app_dir_literal = ps_single_quote(&app_dir);
    let script = format!(
        "$ErrorActionPreference='Stop'; \
$appDir = Convert-Path '{app_dir_literal}'; \
$prefix = $appDir.TrimEnd('\\') + '\\'; \
$targets = @(Get-Process | Where-Object {{ \
    $_.Path -and \
    ([System.IO.Path]::GetFullPath($_.Path)).StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) \
}} | Sort-Object Id -Unique); \
if ($targets.Count -eq 0) {{ \
    Write-Output 'No matching processes found.'; \
    exit 0; \
}}; \
$targets | ForEach-Object {{ Write-Output ('PID {{0}}: {{1}}' -f $_.Id, $_.Path) }}; \
$targets | Stop-Process -Force -ErrorAction Stop; \
Write-Output ('Stopped {{0}} process(es).' -f $targets.Count)"
    );
    let args = vec![
        shell.to_string(),
        "-NoLogo".to_string(),
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        script,
    ];

    let (status, output) = run_capture("gsudo", &borrow_string_args(&args)).await?;
    print_captured_command_output(&output);
    if status != 0 {
        return Err(io::Error::other(format!("结束进程失败 (exit {status})")));
    }
    Ok(())
}

#[cfg(not(windows))]
async fn kill_scoop_task_processes(
    _task: &ScoopUpdateTask,
    _blocked: &ScoopBlockedProcessInfo,
) -> io::Result<()> {
    Err(io::Error::other("仅支持 Windows Scoop 进程恢复"))
}

#[cfg(windows)]
async fn resolve_scoop_app_dir(task: &ScoopUpdateTask) -> io::Result<String> {
    let (status, scoop_dir_output) = run_capture("scoop", &["prefix", "scoop"]).await?;
    if status != 0 {
        return Err(io::Error::other(format!(
            "scoop prefix scoop 失败 (exit {status})"
        )));
    }
    let scoop_core_dir = first_nonempty_output_line(&scoop_dir_output)
        .ok_or_else(|| io::Error::other("未找到 Scoop core 目录"))?;
    let scoop_core_literal = ps_single_quote(&scoop_core_dir);
    let app_literal = ps_single_quote(&task.app);
    let shell = if command_exists("pwsh").await {
        "pwsh"
    } else {
        "powershell.exe"
    };
    let script = format!(
        "$ErrorActionPreference='Stop'; \
. '{scoop_core_literal}\\lib\\core.ps1'; \
. '{scoop_core_literal}\\lib\\versions.ps1'; \
$path = currentdir '{app_literal}' ${}; \
if (Test-Path $path) {{ Convert-Path $path }} else {{ exit 2 }}",
        if matches!(task.scope, ScoopInstallScope::Global) {
            "true"
        } else {
            "false"
        }
    );
    let args = [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ];
    let (status, output) = run_capture(shell, &args).await?;
    if status != 0 {
        return Err(io::Error::other(format!(
            "无法解析 {} 安装目录 (exit {status})",
            task.display_name()
        )));
    }

    first_nonempty_output_line(&output)
        .ok_or_else(|| io::Error::other(format!("无法解析 {} 安装目录", task.display_name())))
}

#[cfg(not(windows))]
async fn resolve_scoop_app_dir(_task: &ScoopUpdateTask) -> io::Result<String> {
    Err(io::Error::other("仅支持 Windows Scoop 目录解析"))
}

fn first_nonempty_output_line(output: &str) -> Option<String> {
    strip_ansi_control_sequences(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(windows)]
async fn notify_scoop_blocked(task: &ScoopUpdateTask, blocked: &ScoopBlockedProcessInfo) {
    ring_terminal_bell();

    let title = "updt";
    let body = if let Some(app_name) = &blocked.app_name {
        format!(
            "{app_name}{} is still running. Return to the terminal to choose Kill, Skip, or Abort.",
            task.scope.suffix()
        )
    } else {
        format!(
            "{} is waiting for your decision in the terminal.",
            task.display_name()
        )
    };

    let _ = spawn_windows_message_box(title, &body).await;
}

#[cfg(not(windows))]
async fn notify_scoop_blocked(_task: &ScoopUpdateTask, _blocked: &ScoopBlockedProcessInfo) {}

#[cfg(windows)]
fn ring_terminal_bell() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x07");
    let _ = stdout.flush();
}

#[cfg(windows)]
async fn spawn_windows_message_box(title: &str, body: &str) -> io::Result<()> {
    let shell = if command_exists("powershell.exe").await {
        "powershell.exe"
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "未找到可用的 PowerShell",
        ));
    };

    let title_literal = ps_single_quote(title);
    let body_literal = ps_single_quote(body);
    let script = format!(
        "Add-Type -AssemblyName PresentationFramework; \
[System.Windows.MessageBox]::Show('{body_literal}', '{title_literal}') | Out-Null"
    );
    tokio::task::block_in_place(|| {
        Command::new(shell)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-WindowStyle")
            .arg("Hidden")
            .arg("-Command")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    })
}

async fn run_pacman_upgrade(state: &AppState) -> bool {
    let (privilege_command, reason) = pacman_privilege_command(state).await;

    if privilege_command == "pkexec" && !command_exists(privilege_command).await {
        println!("[pacman] 未安装 pkexec, 无法使用 GUI 提权.");
        println!("[pacman] 包升级失败.");
        return false;
    }

    if let Some(reason) = reason {
        println!("[pacman] {reason}");
    }
    println!("[pacman] 正在执行: {privilege_command} pacman -Syu");
    match run_inherit(privilege_command, &["pacman", "-Syu"]).await {
        Ok(true) => {
            println!("[pacman] 包升级完成.");
            true
        }
        _ => {
            println!("[pacman] 包升级失败.");
            false
        }
    }
}

async fn pacman_privilege_command(state: &AppState) -> (&'static str, Option<&'static str>) {
    if !state.is_arch_linux || !desktop_linux_session() {
        return ("sudo", None);
    }

    match terminal_focus_state().await {
        TerminalFocusState::Focused => ("sudo", None),
        TerminalFocusState::NotFocused => {
            ("pkexec", Some("terminal 未处于桌面焦点, 使用 GUI 提权."))
        }
        TerminalFocusState::Unknown => (
            "pkexec",
            Some("无法确认 terminal 处于桌面焦点, 使用 GUI 提权."),
        ),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalFocusState {
    Focused,
    NotFocused,
    Unknown,
}

async fn terminal_focus_state() -> TerminalFocusState {
    if !interactive_terminal() {
        return TerminalFocusState::NotFocused;
    }

    if let Some(focused) = terminal_focused_by_x11_window_id().await {
        return if focused {
            TerminalFocusState::Focused
        } else {
            TerminalFocusState::NotFocused
        };
    }

    if let Some(pid) = active_window_pid().await {
        return if current_process_belongs_to_window(pid).await {
            TerminalFocusState::Focused
        } else {
            TerminalFocusState::NotFocused
        };
    }

    TerminalFocusState::Unknown
}

async fn terminal_focused_by_x11_window_id() -> Option<bool> {
    let terminal_window_id = env::var("WINDOWID").ok()?.trim().parse::<u64>().ok()?;
    if terminal_window_id == 0 || !command_exists("xdotool").await {
        return None;
    }

    let (status, output) = run_capture("xdotool", &["getactivewindow"]).await.ok()?;
    if status != 0 {
        return None;
    }
    let active_window_id = output.trim().parse::<u64>().ok()?;
    Some(active_window_id == terminal_window_id)
}

async fn active_window_pid() -> Option<u32> {
    if let Some(pid) = active_window_pid_from_hyprland().await {
        Some(pid)
    } else {
        active_window_pid_from_x11().await
    }
}

async fn active_window_pid_from_hyprland() -> Option<u32> {
    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() || !command_exists("hyprctl").await {
        return None;
    }

    let (status, output) = run_capture("hyprctl", &["activewindow", "-j"]).await.ok()?;
    if status != 0 {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(&output).ok()?;
    value
        .get("pid")?
        .as_u64()
        .and_then(|pid| pid.try_into().ok())
}

async fn active_window_pid_from_x11() -> Option<u32> {
    if env::var_os("DISPLAY").is_none() || !command_exists("xdotool").await {
        return None;
    }

    let (status, output) = run_capture("xdotool", &["getactivewindow", "getwindowpid"])
        .await
        .ok()?;
    if status != 0 {
        return None;
    }
    output.trim().parse::<u32>().ok()
}

async fn current_process_belongs_to_window(window_pid: u32) -> bool {
    let mut next = Some(process::id());
    for _ in 0..64 {
        let Some(pid) = next else {
            break;
        };
        if pid == window_pid {
            return true;
        }
        next = parent_pid(pid).await;
    }
    false
}

async fn parent_pid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).await.ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    let mut fields = after_comm.split_whitespace();
    fields.next()?;
    let parent = fields.next()?.parse::<u32>().ok()?;
    (parent != 0).then_some(parent)
}

#[cfg(windows)]
async fn schedule_windows_self_update(pkg: &str) -> io::Result<()> {
    let parent_pid = process::id();
    let script = format!(
        "$ErrorActionPreference='Continue'; \
$parentPid={parent_pid}; \
while (Get-Process -Id $parentPid -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; \
cargo install-update --locked {pkg}; \
Write-Host ''; \
Write-Host 'Self-update finished. Press Enter to close this window.'; \
[void](Read-Host); \
exit"
    );

    let shell = if command_exists("pwsh").await {
        "pwsh"
    } else {
        "powershell.exe"
    };

    let primary = tokio::task::block_in_place(|| {
        Command::new("cmd.exe")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(shell)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    });

    if primary.is_ok() {
        return Ok(());
    }

    tokio::task::block_in_place(|| {
        Command::new("cmd.exe")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::{ScoopInstallScope, ScoopUpdateTask, build_scoop_update_tasks};
    use crate::parse::ScoopListItem;

    #[test]
    fn builds_one_local_scoop_task() {
        let tasks = build_scoop_update_tasks(
            &[String::from("git")],
            &[ScoopListItem {
                name: "git".to_string(),
                is_global: false,
            }],
        );

        assert_eq!(
            tasks,
            vec![ScoopUpdateTask {
                app: "git".to_string(),
                scope: ScoopInstallScope::Local,
            }]
        );
    }

    #[test]
    fn builds_one_global_scoop_task() {
        let tasks = build_scoop_update_tasks(
            &[String::from("git")],
            &[ScoopListItem {
                name: "git".to_string(),
                is_global: true,
            }],
        );

        assert_eq!(
            tasks,
            vec![ScoopUpdateTask {
                app: "git".to_string(),
                scope: ScoopInstallScope::Global,
            }]
        );
    }

    #[test]
    fn builds_local_and_global_scoop_tasks_for_same_app() {
        let tasks = build_scoop_update_tasks(
            &[String::from("git")],
            &[
                ScoopListItem {
                    name: "git".to_string(),
                    is_global: false,
                },
                ScoopListItem {
                    name: "git".to_string(),
                    is_global: true,
                },
            ],
        );

        assert_eq!(
            tasks,
            vec![
                ScoopUpdateTask {
                    app: "git".to_string(),
                    scope: ScoopInstallScope::Local,
                },
                ScoopUpdateTask {
                    app: "git".to_string(),
                    scope: ScoopInstallScope::Global,
                },
            ]
        );
    }
}
