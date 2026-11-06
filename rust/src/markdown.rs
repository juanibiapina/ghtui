//! Markdown to styled lines. Stands in for glamour with the Tokyo Night style
//! from internal/tui/markdown/theme.go (no margins, no heading prefixes,
//! orange inline code on a dark chip, links shown as text plus a link glyph).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::icons;
use crate::text::{Block, str_width, wrap_spans};

const BLUE: Color = Color::Rgb(0x82, 0xaa, 0xff);
const PURPLE: Color = Color::Rgb(0xc0, 0x99, 0xff);
const CYAN: Color = Color::Rgb(0x86, 0xe1, 0xfc);
const FG: Color = Color::Rgb(0xc8, 0xd3, 0xf5);
const QUOTE: Color = Color::Rgb(0x63, 0x6d, 0xa6);
const RULE: Color = Color::Rgb(0x3b, 0x42, 0x61);
const CODE_FG: Color = Color::Rgb(0xff, 0x96, 0x6c);
const CODE_BG: Color = Color::Rgb(0x2f, 0x33, 0x4d);

struct ListCtx {
    next: Option<u64>,
}

struct Renderer {
    width: usize,
    out: Block,
    inline: Vec<Span<'static>>,
    styles: Vec<Style>,
    lists: Vec<ListCtx>,
    quote_depth: usize,
    item_marker: Option<String>,
    in_code_block: bool,
    code_buf: String,
    link_stack: usize,
    blank_pending: bool,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, f: impl Fn(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn quote_prefix(&self) -> Vec<Span<'static>> {
        (0..self.quote_depth)
            .map(|_| Span::styled("│ ", Style::default().fg(QUOTE)))
            .collect()
    }

    fn list_indent(&self) -> usize {
        self.lists.len().saturating_sub(1) * 2
    }

    fn start_block(&mut self) {
        if self.blank_pending && !self.out.is_empty() {
            let mut l = Line::from(self.quote_prefix());
            if l.spans.is_empty() {
                l = Line::default();
            }
            self.out.push(l);
        }
        self.blank_pending = false;
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() && self.item_marker.is_none() {
            return;
        }
        let spans = std::mem::take(&mut self.inline);
        let quote = self.quote_prefix();
        let quote_w: usize = quote.iter().map(|s| str_width(&s.content)).sum();
        let indent = if self.lists.is_empty() {
            0
        } else {
            self.list_indent()
        };
        let marker = self.item_marker.take();
        let marker_w = marker
            .as_ref()
            .map(|m| str_width(m))
            .unwrap_or(if self.lists.is_empty() { 0 } else { 2 });
        let avail = self
            .width
            .saturating_sub(quote_w + indent + marker_w)
            .max(1);
        let wrapped = wrap_spans(&spans, avail);
        for (i, l) in wrapped.into_iter().enumerate() {
            let mut line_spans = quote.clone();
            if indent > 0 {
                line_spans.push(Span::raw(" ".repeat(indent)));
            }
            if i == 0 {
                if let Some(m) = &marker {
                    line_spans.push(Span::raw(m.clone()));
                } else if marker_w > 0 {
                    line_spans.push(Span::raw(" ".repeat(marker_w)));
                }
            } else if marker_w > 0 {
                line_spans.push(Span::raw(" ".repeat(marker_w)));
            }
            line_spans.extend(l.spans);
            self.out.push(Line::from(line_spans));
        }
    }

    fn text(&mut self, t: &str) {
        if self.in_code_block {
            self.code_buf.push_str(t);
            return;
        }
        self.inline.push(Span::styled(t.to_string(), self.style()));
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let s = Style::default().fg(CODE_FG).bg(CODE_BG);
                self.inline.push(Span::styled(format!(" {t} "), s));
            }
            Event::SoftBreak => self.inline.push(Span::styled(" ", self.style())),
            Event::HardBreak => self.inline.push(Span::raw("\n")),
            Event::Rule => {
                self.flush_inline();
                self.start_block();
                self.out.push(Line::from(Span::styled(
                    "--------",
                    Style::default().fg(RULE),
                )));
                self.blank_pending = true;
            }
            Event::TaskListMarker(checked) => {
                let m = if checked { "[✓] " } else { "[ ] " };
                self.inline.push(Span::styled(m, self.style()));
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(r) => self.inline.push(Span::raw(format!("[{r}]"))),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if (self.lists.is_empty() || self.item_marker.is_none())
                    && self.item_marker.is_none()
                {
                    self.flush_inline();
                    self.start_block();
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.start_block();
                let color = match level {
                    HeadingLevel::H1 => PURPLE,
                    HeadingLevel::H2 => BLUE,
                    HeadingLevel::H3 => CYAN,
                    _ => FG,
                };
                self.push_style(|s| s.fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.start_block();
                self.quote_depth += 1;
                self.push_style(|s| s.fg(QUOTE));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                self.start_block();
                self.in_code_block = true;
                self.code_buf.clear();
                let _ = matches!(kind, CodeBlockKind::Fenced(_));
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.start_block();
                }
                self.lists.push(ListCtx { next: start });
            }
            Tag::Item => {
                self.flush_inline();
                let marker = match self.lists.last_mut() {
                    Some(ListCtx { next: Some(n) }) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.item_marker = Some(marker);
            }
            Tag::Emphasis => self.push_style(|s| s.add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(|s| s.add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => {
                self.link_stack += 1;
                self.push_style(|s| s.fg(BLUE).add_modifier(Modifier::BOLD));
            }
            Tag::Image { .. } => {
                self.inline.push(Span::styled(
                    format!("{} ", icons::IMAGE),
                    Style::default().fg(BLUE),
                ));
                self.push_style(|s| s.fg(BLUE).add_modifier(Modifier::BOLD));
            }
            Tag::Table(_) | Tag::TableHead | Tag::TableRow => self.flush_inline(),
            Tag::TableCell if !self.inline.is_empty() => {
                self.inline
                    .push(Span::styled(" │ ", Style::default().fg(RULE)));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                self.blank_pending = true;
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_inline();
                self.blank_pending = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.pop_style();
                self.blank_pending = true;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                let code = std::mem::take(&mut self.code_buf);
                let style = Style::default().fg(Color::Indexed(244));
                let quote = self.quote_prefix();
                for l in code.trim_end_matches('\n').split('\n') {
                    let mut spans = quote.clone();
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(l.replace('\t', "    "), style));
                    self.out.push(Line::from(spans));
                }
                self.blank_pending = true;
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank_pending = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                self.blank_pending = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                self.link_stack = self.link_stack.saturating_sub(1);
                self.inline.push(Span::styled(
                    format!(" {}", icons::LINK),
                    Style::default().fg(BLUE),
                ));
            }
            TagEnd::Image => self.pop_style(),
            TagEnd::TableRow | TagEnd::TableHead => self.flush_inline(),
            TagEnd::Table => {
                self.flush_inline();
                self.blank_pending = true;
            }
            _ => {}
        }
    }
}

pub fn render(md: &str, width: usize) -> Block {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_TABLES);
    let mut r = Renderer {
        width: width.max(1),
        out: vec![],
        inline: vec![],
        styles: vec![Style::default()],
        lists: vec![],
        quote_depth: 0,
        item_marker: None,
        in_code_block: false,
        code_buf: String::new(),
        link_stack: 0,
        blank_pending: false,
    };
    for ev in Parser::new_ext(md, opts) {
        r.handle(ev);
    }
    r.flush_inline();
    // glamour ends its output with a newline, which shows up as one blank line.
    if !r.out.is_empty() {
        r.out.push(Line::default());
    }
    r.out
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
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
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
