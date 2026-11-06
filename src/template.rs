//! The subset of Go text/template used by ghtui: `{{ .Field }}` in custom
//! keybinding commands (missing fields are errors) and `{{ nowModify "-2w" }}`
//! in section filters.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use chrono::{Duration, Local};

pub fn render(tpl: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| anyhow!("unclosed action in template"))?;
        let expr = after[..end]
            .trim()
            .trim_start_matches('-')
            .trim_end_matches('-')
            .trim();
        out.push_str(&eval(expr, vars)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn eval(expr: &str, vars: &HashMap<String, String>) -> Result<String> {
    if let Some(field) = expr.strip_prefix('.') {
        return vars
            .get(field)
            .cloned()
            .ok_or_else(|| anyhow!("map has no entry for key \"{field}\""));
    }
    if let Some(arg) = expr.strip_prefix("nowModify") {
        let arg = arg.trim().trim_matches('"');
        let dur = parse_duration(arg)?;
        return Ok((Local::now() + dur).format("%Y-%m-%d").to_string());
    }
    bail!("unsupported template expression: {expr}")
}

/// "10d", "-1.5w", "3Y4M5d"; units d/D, w/W, mo/M, y/Y, plus Go's h/m/s.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let re = regex::Regex::new(r"(\d*\.\d+|\d+)([^\d.]*)").unwrap();
    let mut total_secs = 0f64;
    let mut matched = false;
    for cap in re.captures_iter(s) {
        matched = true;
        let n: f64 = cap[1].parse()?;
        let unit = &cap[2];
        let hours = if unit.contains("mo") || unit.contains('M') {
            30.0 * 24.0
        } else if unit.contains('d') || unit.contains('D') {
            24.0
        } else if unit.contains('w') || unit.contains('W') {
            7.0 * 24.0
        } else if unit.contains('y') || unit.contains('Y') {
            365.0 * 24.0
        } else {
            match unit {
                "h" => 1.0,
                "m" => 1.0 / 60.0,
                "s" => 1.0 / 3600.0,
                _ => bail!("time: missing unit in duration \"{s}\""),
            }
        };
        total_secs += n * hours * 3600.0;
    }
    if !matched {
        bail!("time: invalid duration \"{s}\"");
    }
    let d = Duration::milliseconds((total_secs * 1000.0) as i64);
    Ok(if neg { -d } else { d })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_fields_and_errors_on_missing() {
        let mut v = HashMap::new();
        v.insert("PrNumber".to_string(), "42".to_string());
        assert_eq!(
            render("gh pr view {{.PrNumber}}", &v).unwrap(),
            "gh pr view 42"
        );
        assert_eq!(render("x {{ .PrNumber }} y", &v).unwrap(), "x 42 y");
        assert!(render("cd {{.RepoPath}}", &v).is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("-2w").unwrap(), -Duration::days(14));
        assert_eq!(parse_duration("1mo").unwrap(), Duration::days(30));
        assert_eq!(parse_duration("3d").unwrap(), Duration::days(3));
    }

    #[test]
    fn now_modify_renders_a_date() {
        let out = render("updated:>={{ nowModify \"-2w\" }}", &HashMap::new()).unwrap();
        let want = (Local::now() - Duration::days(14))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(out, format!("updated:>={want}"));
    }
}
