use std::env;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::fs;
use tokio::process::Command;

pub async fn command_exists(name: &str) -> bool {
    resolve_command_path(name).await.is_some()
}

pub async fn resolve_command_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
        let path = Path::new(name);
        return is_executable(path).await.then(|| path.to_path_buf());
    }

    let candidates = command_name_candidates(name);
    for dir in env::split_paths(&env::var_os("PATH")?) {
        for candidate_name in &candidates {
            let candidate = dir.join(candidate_name);
            if is_executable(&candidate).await {
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

async fn command_program(program: &str) -> PathBuf {
    resolve_command_path(program)
        .await
        .unwrap_or_else(|| PathBuf::from(program))
}

async fn command(program: &str) -> Command {
    let program_path = command_program(program).await;
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

async fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path).await else {
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

pub async fn run_capture(program: &str, args: &[&str]) -> io::Result<(i32, String)> {
    let output = command(program).await.args(args).output().await?;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((code, text))
}

pub async fn run_inherit(program: &str, args: &[&str]) -> io::Result<bool> {
    let status = command(program)
        .await
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    Ok(status.success())
}

pub async fn run_cargo_install_update_capture(args: &[&str]) -> io::Result<(i32, String)> {
    let mut proxy_args = vec!["install-update", "--locked"];
    proxy_args.extend_from_slice(args);
    run_capture("cargo-install-update", &proxy_args).await
}

pub async fn run_cargo_install_update_inherit(args: &[&str]) -> io::Result<bool> {
    let mut proxy_args = vec!["install-update", "--locked"];
    proxy_args.extend_from_slice(args);
    run_inherit("cargo-install-update", &proxy_args).await
}

pub async fn run_nvim_headless_capture(args: &[&str]) -> io::Result<(i32, String)> {
    let mut all_args = vec!["--headless"];
    all_args.extend_from_slice(args);
    run_capture("nvim", &all_args).await
}

pub async fn run_nvim_headless_inherit(args: &[&str]) -> io::Result<bool> {
    let mut all_args = vec!["--headless"];
    all_args.extend_from_slice(args);
    run_inherit("nvim", &all_args).await
}
