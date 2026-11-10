use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

const DAYS_IN_YEAR: f64 = 365.0;
const DAYS_IN_MONTH: f64 = 28.0;
const DAYS_IN_WEEK: f64 = 7.0;

/// The largest rounded unit wins ("3d", "2mo", "now").
pub fn time_elapsed(then: DateTime<Utc>) -> String {
    let diff = Utc::now().signed_duration_since(then);
    let secs = diff.num_milliseconds() as f64 / 1000.0;
    let hours = secs / 3600.0;
    let day = (hours / 24.0).round();
    let year = (day / DAYS_IN_YEAR).round();
    let month = (day / DAYS_IN_MONTH).round();
    let week = (day / DAYS_IN_WEEK).round();
    let hour = hours.abs().round();
    let minute = (secs / 60.0).abs().round();
    let second = secs.abs().round();

    let parts: [(f64, &str); 7] = [
        (year, "y"),
        (month, "mo"),
        (week, "w"),
        (day, "d"),
        (hour, "h"),
        (minute, "m"),
        (second, "s"),
    ];
    for (value, unit) in parts {
        if value > 0.0 {
            return format!("{}{}", value as i64, unit);
        }
    }
    "now".to_string()
}

pub fn short_number(n: usize) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", n / 1000)
    } else {
        format!("{}m", n / 1_000_000)
    }
}

pub fn format_number(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Exact match, then `owner/*`, then `:owner/:repo`.
pub fn repo_local_path(repo_name: &str, paths: &HashMap<String, String>) -> Option<String> {
    if let Some(p) = paths.get(repo_name) {
        return Some(p.clone());
    }
    let parts: Vec<&str> = repo_name.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let (owner, repo) = (parts[0], parts[1]);
    if let Some(p) = paths.get(&format!("{owner}/*")) {
        return Some(format!("{}/{}", p.trim_end_matches("/*"), repo));
    }
    if let Some(t) = paths.get(":owner/:repo") {
        return Some(t.replace(":owner", owner).replace(":repo", repo));
    }
    None
}

pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        return format!("{}{}", home.display(), rest);
    }
    path.to_string()
}

/// Opens a URL as gh does: $GH_BROWSER, $BROWSER, then the OS opener.
pub fn open_browser(url: &str) -> anyhow::Result<()> {
    let launcher = std::env::var("GH_BROWSER")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("BROWSER").ok().filter(|s| !s.is_empty()));
    let mut cmd = match launcher {
        Some(l) => {
            let mut parts = l.split_whitespace();
            let mut c = Command::new(parts.next().unwrap_or("open"));
            c.args(parts);
            c
        }
        None if cfg!(target_os = "macos") => Command::new("open"),
        None if cfg!(target_os = "windows") => {
            let mut c = Command::new("cmd");
            c.args(["/c", "start"]);
            c
        }
        None => Command::new("xdg-open"),
    };
    let status = cmd
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("browser exited with {status}");
    }
    Ok(())
}

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    let mut last_err = anyhow::anyhow!("no clipboard utility found");
    for (bin, args) in candidates {
        match Command::new(bin)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.stdin.take();
                child.wait()?;
                return Ok(());
            }
            Err(e) => last_err = e.into(),
        }
    }
    Err(last_err)
}

/// `$SHELL -c`, falling back to `sh -c` (or cmd.exe on Windows).
pub fn shell_command(cmd: &str) -> Command {
    if let Ok(shell) = std::env::var("SHELL")
        && !shell.is_empty()
    {
        let mut c = Command::new(shell);
        c.arg("-c").arg(cmd);
        return c;
    }
    if cfg!(target_os = "windows") {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        let mut c = Command::new(comspec);
        c.arg("/C").arg(cmd);
        return c;
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

pub fn truncate_command(cmd: &str) -> String {
    let cmd = cmd.replace('\n', "");
    if cmd.chars().count() > 30 {
        format!("{}...", cmd.chars().take(30).collect::<String>())
    } else {
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_paths_match_exact_then_wildcards() {
        let mut paths = HashMap::new();
        paths.insert("user/repo".to_string(), "/path/to/user/repo".to_string());
        paths.insert("user_2/*".to_string(), "/path/to/user_2/*".to_string());
        assert_eq!(
            repo_local_path("user/repo", &paths).as_deref(),
            Some("/path/to/user/repo")
        );
        assert_eq!(
            repo_local_path("user_2/some_repo", &paths).as_deref(),
            Some("/path/to/user_2/some_repo")
        );
        assert_eq!(repo_local_path("user/other_repo", &paths), None);
        paths.insert(":owner/:repo".to_string(), "~/src/:owner/:repo".to_string());
        assert_eq!(repo_local_path("a/b", &paths).as_deref(), Some("~/src/a/b"));
    }

    #[test]
    fn elapsed_picks_largest_unit() {
        let t = Utc::now() - chrono::Duration::minutes(25);
        assert_eq!(time_elapsed(t), "25m");
        let t = Utc::now() - chrono::Duration::hours(30);
        assert_eq!(time_elapsed(t), "1d");
        let t = Utc::now() - chrono::Duration::days(9);
        assert_eq!(time_elapsed(t), "1w");
    }
}
