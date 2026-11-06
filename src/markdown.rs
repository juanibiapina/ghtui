//! Markdown for the preview pane: body cleanup as prview/issueview do it, and
//! rendering through the glamour port.

use crate::text::Block;

pub fn render(md: &str, width: usize) -> Block {
    crate::glamour::render(md, width)
}

/// Removes HTML comments and table-like lines, as prview/issueview do before rendering.
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string())
            .collect()
    }

    /// Prints a rendering of $MD_IN for comparing against glamour by hand.
    #[test]
    #[ignore]
    fn print_md_in() {
        let md = std::fs::read_to_string(std::env::var("MD_IN").unwrap()).unwrap();
        let backend = ratatui::backend::TestBackend::new(60, 200);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let block = render(&md, 60);
        term.draw(|f| { let a = f.area(); crate::text::draw_block(f.buffer_mut(), a, &block) }).unwrap();
        let buf = term.backend().buffer().clone();
        for y in 0..block.len() as u16 {
            let line: String = (0..60).map(|x| buf[(x, y)].symbol().to_string()).collect();
            println!("MD|{}", line.trim_end());
        }
        for l in &block {
            let cells: Vec<String> = l
                .spans
                .iter()
                .flat_map(|s| {
                    let fg = match s.style.fg {
                        Some(ratatui::style::Color::Indexed(n)) => format!("i{n}"),
                        Some(ratatui::style::Color::Rgb(r, g, b)) => format!("r{r},{g},{b}"),
                        _ => String::new(),
                    };
                    let bold = s.style.add_modifier.contains(ratatui::style::Modifier::BOLD);
                    s.content.chars().map(move |ch| format!("{ch}\u{1}{fg}\u{1}{}", if bold { "b" } else { "" })).collect::<Vec<_>>()
                })
                .collect();
            println!("MS|{}", cells.join("\u{2}"));
        }
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
