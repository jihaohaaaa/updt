use crate::command::{run_cargo_install_update_inherit, run_inherit, run_nvim_headless_inherit};
use crate::output::{err_text, ok_text, print_section};
use crate::state::{AppState, target_label};

#[cfg(windows)]
use crate::command::command_exists;
#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::process::{self, Command, Stdio};

pub fn upgrade_selected(state: &AppState, selected: &[String]) -> bool {
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
                "[cargo] 正在执行: cargo install-update --locked {}",
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
                "[cargo] 即将单独升级 updt: 先退出当前 updt, 再执行 cargo install-update --locked updt"
            );
            match schedule_windows_self_update(self_pkg) {
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

#[cfg(windows)]
fn schedule_windows_self_update(pkg: &str) -> io::Result<()> {
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
        .arg("-Command")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}
