use std::env;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn command_exists(name: &str) -> bool {
    resolve_command_path(name).is_some()
}

pub fn resolve_command_path(name: &str) -> Option<PathBuf> {
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

pub fn run_capture(program: &str, args: &[&str]) -> io::Result<(i32, String)> {
    let output = command(program).args(args).output()?;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((code, text))
}

pub fn run_inherit(program: &str, args: &[&str]) -> io::Result<bool> {
    let status = command(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.success())
}

pub fn run_cargo_install_update_capture(args: &[&str]) -> io::Result<(i32, String)> {
    let mut proxy_args = vec!["install-update"];
    proxy_args.extend_from_slice(args);
    run_capture("cargo-install-update", &proxy_args)
}

pub fn run_cargo_install_update_inherit(args: &[&str]) -> io::Result<bool> {
    let mut proxy_args = vec!["install-update"];
    proxy_args.extend_from_slice(args);
    run_inherit("cargo-install-update", &proxy_args)
}

pub fn run_nvim_headless_capture(args: &[&str]) -> io::Result<(i32, String)> {
    let mut all_args = vec!["--headless"];
    all_args.extend_from_slice(args);
    run_capture("nvim", &all_args)
}

pub fn run_nvim_headless_inherit(args: &[&str]) -> io::Result<bool> {
    let mut all_args = vec!["--headless"];
    all_args.extend_from_slice(args);
    run_inherit("nvim", &all_args)
}

pub fn first_json_payload(output: &str) -> Option<&str> {
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

pub fn first_token(line: &str) -> Option<String> {
    line.split_whitespace().next().map(ToOwned::to_owned)
}

pub fn parse_cargo_list(output: &str) -> Result<Vec<String>, ()> {
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

pub fn parse_fnm_version_token(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches('*').trim();
    let token = trimmed.split_whitespace().next()?;
    if token.starts_with('v') {
        return Some(token.to_string());
    }
    None
}

pub fn extract_marker_count(output: &str, marker: &str) -> Option<usize> {
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(raw) = trimmed.strip_prefix(marker)
            && let Ok(value) = raw.trim().parse::<usize>()
        {
            return Some(value);
        }
    }
    None
}

pub fn strip_ansi_control_sequences(text: &str) -> String {
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
