use std::process::Command;

pub fn format_duration(hours: f64) -> String {
    if hours < 1.0 {
        let mins = (hours * 60.0).round() as i32;
        format!("{mins} minutes")
    } else if hours == 1.0 {
        "1 hour".to_string()
    } else if hours < 24.0 {
        format!("{} hours", hours as i32)
    } else {
        let days = (hours / 24.0).round() as i32;
        if days == 1 {
            "1 day".to_string()
        } else {
            format!("{days} days")
        }
    }
}

pub fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let val: f64 = parts[0].parse().ok()?;
    match parts[1] {
        "minutes" | "minute" | "mins" | "min" => Some(val / 60.0),
        "hour" | "hours" | "h" => Some(val),
        "day" | "days" | "d" => Some(val * 24.0),
        _ => None,
    }
}

pub fn input_dialog(title: &str, prompt: &str, default: &str) -> Option<String> {
    let output = Command::new("zenity")
        .arg("--entry")
        .arg("--title")
        .arg(title)
        .arg("--text")
        .arg(prompt)
        .arg("--entry-text")
        .arg(default)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn open_file_dialog(title: &str, filename: &str) -> Option<String> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--file-selection").arg("--title").arg(title);

    if !filename.is_empty() {
        cmd.arg("--filename").arg(filename);
    }

    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn open_files_dialog(title: &str) -> Option<Vec<String>> {
    let output = Command::new("zenity")
        .arg("--file-selection")
        .arg("--title")
        .arg(title)
        .arg("--multiple")
        .arg("--separator")
        .arg("\n")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let files: Vec<String> = s.lines().map(|l| l.to_string()).filter(|l| !l.is_empty()).collect();

    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

pub fn select_list(
    title: &str,
    text: &str,
    options: &[String],
    _selected: Option<&str>,
) -> Option<String> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--list")
        .arg("--title")
        .arg(title)
        .arg("--column")
        .arg("")
        .arg("--hide-header")
        .arg("--print-column")
        .arg("1");

    if !text.is_empty() {
        cmd.arg("--text").arg(text);
    }

    for opt in options {
        cmd.arg(opt);
    }

    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
