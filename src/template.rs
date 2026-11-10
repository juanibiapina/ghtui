//! `{{ … }}` expressions in custom keybinding commands and section filters:
//! `{{ prNumber }}` inserts a variable, and `{{ now }}`, `{{ now-2w }}` or
//! `{{ now+1mo2d }}` insert a date as `YYYY-MM-DD`.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use chrono::{Days, Local, Months, NaiveDate};

pub fn render(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    render_on(template, vars, Local::now().date_naive())
}

fn render_on(template: &str, vars: &HashMap<String, String>, today: NaiveDate) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| anyhow!("'{{{{' without a closing '}}}}' in: {template}"))?;
        out.push_str(&eval(after[..end].trim(), vars, today)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn eval(expr: &str, vars: &HashMap<String, String>, today: NaiveDate) -> Result<String> {
    if let Some(offset) = expr.strip_prefix("now") {
        let date = shift(today, offset.trim()).ok_or_else(|| {
            anyhow!("invalid date '{{{{ {expr} }}}}': write now, now-2w or now+1mo2d")
        })?;
        return Ok(date.format("%Y-%m-%d").to_string());
    }
    match vars.get(expr) {
        Some(value) => Ok(value.clone()),
        None if vars.is_empty() => bail!("unknown expression '{{{{ {expr} }}}}'"),
        None => {
            let mut names: Vec<&str> = vars.keys().map(String::as_str).collect();
            names.sort();
            bail!("unknown variable '{expr}': use one of {}", names.join(", "))
        }
    }
}

/// Moves `date` by an offset like `-2w` or `+1mo2d`, in calendar units: d, w, mo, y.
fn shift(date: NaiveDate, offset: &str) -> Option<NaiveDate> {
    if offset.is_empty() {
        return Some(date);
    }
    let (back, mut rest) = match offset.split_at(1) {
        ("-", r) => (true, r),
        ("+", r) => (false, r),
        _ => return None,
    };
    if rest.is_empty() {
        return None;
    }
    let mut date = date;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let n: u32 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let (unit, tail) = ["mo", "d", "w", "y"]
            .iter()
            .find_map(|u| rest.strip_prefix(u).map(|t| (*u, t)))?;
        rest = tail;
        date = match (unit, back) {
            ("d", true) => date.checked_sub_days(Days::new(n.into()))?,
            ("d", false) => date.checked_add_days(Days::new(n.into()))?,
            ("w", true) => date.checked_sub_days(Days::new(7 * u64::from(n)))?,
            ("w", false) => date.checked_add_days(Days::new(7 * u64::from(n)))?,
            ("mo", true) => date.checked_sub_months(Months::new(n))?,
            ("mo", false) => date.checked_add_months(Months::new(n))?,
            (_, true) => date.checked_sub_months(Months::new(n.checked_mul(12)?))?,
            (_, false) => date.checked_add_months(Months::new(n.checked_mul(12)?))?,
        };
    }
    Some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn render_at(template: &str, today: &str) -> Result<String> {
        render_on(template, &HashMap::new(), day(today))
    }

    #[test]
    fn substitutes_variables() {
        let vars = HashMap::from([("prNumber".to_string(), "42".to_string())]);
        assert_eq!(
            render("gh pr view {{prNumber}} && echo {{ prNumber }}", &vars).unwrap(),
            "gh pr view 42 && echo 42"
        );
    }

    #[test]
    fn an_unknown_variable_names_itself_and_the_known_ones() {
        let vars = HashMap::from([("prNumber".to_string(), "42".to_string())]);
        let e = render("cd {{ repoPath }}", &vars).unwrap_err().to_string();
        assert!(e.contains("'repoPath'") && e.contains("prNumber"), "{e}");
    }

    #[test]
    fn now_renders_dates_with_calendar_offsets() {
        assert_eq!(render_at("{{ now }}", "2025-02-02").unwrap(), "2025-02-02");
        assert_eq!(
            render_at("{{ now-1mo }}", "2025-02-02").unwrap(),
            "2025-01-02"
        );
        assert_eq!(
            render_at("{{ now-2w }}", "2025-02-02").unwrap(),
            "2025-01-19"
        );
        assert_eq!(
            render_at("{{now+1y1d}}", "2024-02-28").unwrap(),
            "2025-03-01"
        );
    }

    #[test]
    fn a_month_back_from_the_31st_lands_on_the_last_day() {
        assert_eq!(
            render_at("{{ now-1mo }}", "2025-03-31").unwrap(),
            "2025-02-28"
        );
    }

    #[test]
    fn invalid_offsets_are_errors() {
        for t in [
            "{{ now-2 }}",
            "{{ now-1.5w }}",
            "{{ now-2h }}",
            "{{ now2w }}",
            "{{ now- }}",
        ] {
            assert!(render_at(t, "2025-02-02").is_err(), "{t}");
        }
    }

    #[test]
    fn go_template_syntax_is_an_error() {
        assert!(render_at("{{ nowModify \"-2w\" }}", "2025-02-02").is_err());
        assert!(render_at("{{ .PrNumber }}", "2025-02-02").is_err());
    }
}
