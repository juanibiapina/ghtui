//! Formats times with Go reference layouts ("2006-01-02 15:04"), used by
//! `defaults.dateFormat`.

use chrono::{DateTime, Local, Utc};

const TOKENS: &[(&str, &str)] = &[
    ("January", "%B"),
    ("Monday", "%A"),
    ("Z07:00", "%:z"),
    ("-07:00", "%:z"),
    ("-0700", "%z"),
    ("2006", "%Y"),
    ("Jan", "%b"),
    ("Mon", "%a"),
    ("MST", "%Z"),
    ("_2", "%e"),
    ("06", "%y"),
    ("01", "%m"),
    ("02", "%d"),
    ("15", "%H"),
    ("03", "%I"),
    ("04", "%M"),
    ("05", "%S"),
    ("PM", "%p"),
    ("pm", "%P"),
    ("1", "%-m"),
    ("2", "%-d"),
    ("3", "%-I"),
    ("4", "%-M"),
    ("5", "%-S"),
];

pub fn go_layout_to_strftime(layout: &str) -> String {
    let mut out = String::new();
    let mut rest = layout;
    'outer: while !rest.is_empty() {
        for (go, cf) in TOKENS {
            if let Some(r) = rest.strip_prefix(go) {
                out.push_str(cf);
                rest = r;
                continue 'outer;
            }
        }
        let ch = rest.chars().next().unwrap();
        if ch == '%' {
            out.push_str("%%");
        } else {
            out.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
}

pub fn go_format(t: DateTime<Utc>, layout: &str) -> String {
    t.with_timezone(&Local)
        .format(&go_layout_to_strftime(layout))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_reference_layouts() {
        assert_eq!(go_layout_to_strftime("2006-01-02 15:04"), "%Y-%m-%d %H:%M");
        assert_eq!(
            go_layout_to_strftime("Jan 2, 2006 3:04 PM"),
            "%b %-d, %Y %-I:%M %p"
        );
        assert_eq!(go_layout_to_strftime("01/02 15:04:05"), "%m/%d %H:%M:%S");
    }
}
