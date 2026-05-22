use crate::command::{
    command_exists, run_capture, run_cargo_install_update_inherit, run_inherit,
    run_nvim_headless_inherit,
};
use crate::output::{err_text, ok_text, print_section};
use crate::profile::{desktop_linux_session, interactive_terminal};
use crate::state::{AppState, target_label};
use std::{env, io, process};
use tokio::fs;

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

    if selected.iter().any(|s| s == "scoop") {
        let metadata_args = ["update"];
        println!("[scoop] 正在执行: {}", scoop_command_label(&metadata_args));
        match run_scoop_inherit(&metadata_args).await {
            Ok(true) => {
                let app_args = scoop_app_update_args();
                println!("[scoop] 正在执行: {}", scoop_command_label(&app_args));
                match run_scoop_inherit(&app_args).await {
                    Ok(true) => println!("[scoop] 包升级完成."),
                    Ok(false) => {
                        println!("[scoop] 包升级失败.");
                        run_fail = true;
                    }
                    Err(err) => {
                        println!("[scoop] 包升级失败: {err}");
                        run_fail = true;
                    }
                }
            }
            Ok(false) => {
                println!(
                    "[scoop] 升级失败: {} 失败.",
                    scoop_command_label(&metadata_args)
                );
                run_fail = true;
            }
            Err(err) => {
                println!("[scoop] 升级失败: {err}");
                run_fail = true;
            }
        }
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

fn scoop_app_update_args() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["update", "*", "--global"]
    }
    #[cfg(not(windows))]
    {
        vec!["update", "*"]
    }
}

fn scoop_command_label(args: &[&str]) -> String {
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

async fn run_scoop_inherit(args: &[&str]) -> io::Result<bool> {
    #[cfg(windows)]
    {
        if !command_exists("gsudo").await {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Windows Scoop 更新需要 gsudo, 但未找到 gsudo",
            ));
        }

        let mut elevated_args = Vec::with_capacity(args.len() + 1);
        elevated_args.push("scoop");
        elevated_args.extend_from_slice(args);
        run_inherit("gsudo", &elevated_args).await
    }

    #[cfg(not(windows))]
    {
        run_inherit("scoop", args).await
    }
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
