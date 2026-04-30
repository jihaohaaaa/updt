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
