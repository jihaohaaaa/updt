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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScoopStatusOutput {
    pub updatable_items: Vec<String>,
    pub metadata_outdated: bool,
}

#[derive(Clone, Copy)]
struct ScoopStatusColumns {
    installed_start: usize,
    latest_start: usize,
    latest_end: usize,
}

impl ScoopStatusColumns {
    fn from_header(line: &str) -> Option<Self> {
        let installed_start = line.find("Installed Version")?;
        let latest_start = line.find("Latest Version")?;
        if !line[..installed_start].contains("Name") {
            return None;
        }

        let latest_end = line
            .find("Missing Dependencies")
            .or_else(|| line.find("Info"))
            .unwrap_or(line.len());
        Some(Self {
            installed_start,
            latest_start,
            latest_end,
        })
    }

    fn outdated_name(self, line: &str) -> Option<String> {
        let name = slice_column(line, 0, self.installed_start).trim();
        let latest = slice_column(line, self.latest_start, self.latest_end).trim();
        (!name.is_empty() && !latest.is_empty()).then(|| name.to_string())
    }
}

fn slice_column(line: &str, start: usize, end: usize) -> &str {
    let len = line.len();
    if start >= len || start >= end {
        return "";
    }
    let end = end.min(len);
    line.get(start..end).unwrap_or("")
}

fn is_scoop_status_separator(line: &str) -> bool {
    line.chars().all(|ch| ch == '-' || ch == ' ' || ch == '\t')
}

fn is_scoop_status_noise(line: &str) -> bool {
    matches!(
        line,
        "Everything is ok!" | "Scoop is up to date." | "Scoop was updated successfully!"
    )
}

pub fn parse_scoop_status_output(output: &str) -> ScoopStatusOutput {
    let cleaned = strip_ansi_control_sequences(output);
    let mut parsed = ScoopStatusOutput::default();
    let mut columns = None;

    for raw in cleaned.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || is_scoop_status_noise(trimmed) {
            continue;
        }

        if trimmed.starts_with("WARN") {
            if trimmed.contains("Scoop out of date") || trimmed.contains("bucket(s) out of date") {
                parsed.metadata_outdated = true;
            }
            continue;
        }

        if let Some(header) = ScoopStatusColumns::from_header(line) {
            columns = Some(header);
            continue;
        }
        if is_scoop_status_separator(trimmed) {
            continue;
        }

        if let Some(columns) = columns {
            if let Some(name) = columns.outdated_name(line)
                && !parsed.updatable_items.iter().any(|item| item == &name)
            {
                parsed.updatable_items.push(name);
            }
            continue;
        }

        if let Some(name) = first_token(trimmed)
            && !matches!(name.as_str(), "Name" | "----")
            && !parsed.updatable_items.iter().any(|item| item == &name)
        {
            parsed.updatable_items.push(name);
        }
    }

    parsed
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

#[cfg(test)]
mod tests {
    use super::{ScoopStatusOutput, parse_scoop_status_output};

    #[test]
    fn parses_scoop_status_table_outdated_items() {
        let output = "\
Scoop is up to date.\n\
\n\
Name          Installed Version Latest Version Missing Dependencies Info\n\
----          ----------------- -------------- -------------------- ----\n\
7zip          24.09             25.01\n\
git           2.45.2.windows.1  2.46.0.windows.1\n\
dependency    1.0                              innounp\n";

        assert_eq!(
            parse_scoop_status_output(output),
            ScoopStatusOutput {
                updatable_items: vec!["7zip".to_string(), "git".to_string()],
                metadata_outdated: false,
            }
        );
    }

    #[test]
    fn detects_scoop_metadata_outdated_warning() {
        let output =
            "WARN  Scoop bucket(s) out of date. Run 'scoop update' to get the latest changes.\n";

        assert_eq!(
            parse_scoop_status_output(output),
            ScoopStatusOutput {
                updatable_items: Vec::new(),
                metadata_outdated: true,
            }
        );
    }

    #[test]
    fn supports_legacy_name_only_scoop_status_output() {
        let output = "7zip\ngit\n";

        assert_eq!(
            parse_scoop_status_output(output),
            ScoopStatusOutput {
                updatable_items: vec!["7zip".to_string(), "git".to_string()],
                metadata_outdated: false,
            }
        );
    }
}
