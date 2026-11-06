//! Markdown for the preview pane: body cleanup and
//! rendering through the glamour port.

use crate::text::Block;

pub fn render(md: &str, width: usize) -> Block {
    crate::glamour::render(md, width)
}

/// Removes HTML comments and table-like lines before rendering.
pub fn clean_body(body: &str) -> String {
    static HTML_COMMENT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static TABLE_LINES: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let c = HTML_COMMENT.get_or_init(|| regex::Regex::new(r"(?sU)<!--.*-->").unwrap());
    let t = TABLE_LINES
        .get_or_init(|| regex::Regex::new(r"((\n)+|^)([^\r\n]*\|[^\r\n]*(\n)?)+").unwrap());
    let body = c.replace_all(body, "");
    t.replace_all(&body, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(b: &Block) -> Vec<String> {
        b.iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn paragraphs_and_lists() {
        let out = plain(&render("Hello **world**\n\n- one\n- two\n\nend", 40));
        assert_eq!(
            out,
            vec!["Hello world", "", "• one", "• two", "", "end", ""]
        );
    }

    #[test]
    fn wraps_long_paragraphs() {
        let out = plain(&render("aaa bbb ccc ddd", 7));
        assert_eq!(out, vec!["aaa bbb", "ccc ddd", ""]);
    }

    #[test]
    fn strips_comments_and_tables() {
        let b = clean_body("a <!-- hidden --> b\n| x | y |\n|---|---|\nafter");
        assert!(!b.contains("hidden"));
        assert!(!b.contains('|'));
        assert!(b.contains("after"));
    }
}
