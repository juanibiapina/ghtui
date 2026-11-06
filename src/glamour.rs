//! A port of glamour v2's ANSI renderer (charm.land/glamour/v2/ansi) with the
//! gh-dash Tokyo Night style. Markdown is parsed with pulldown-cmark into a
//! tree shaped like goldmark's AST (including GFM linkify), then rendered with
//! glamour's block stack, margin/indent/padding writers and wrapping passes, so
//! previews lay out the same way as the Go version.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use crate::highlight;
use crate::icons;
use crate::text::{Block, ansi_wrap_with, cells_to_line, str_width};
use crate::theme::parse_color;

type Cell = (char, Style);

// ---------------- styles ----------------

#[derive(Clone, Default, Debug)]
struct Prim {
    color: Option<Color>,
    bg: Option<Color>,
    underline: Option<bool>,
    bold: Option<bool>,
    italic: Option<bool>,
    crossed: Option<bool>,
    block_prefix: String,
    block_suffix: String,
    prefix: String,
    suffix: String,
    format: String,
}

#[derive(Clone, Default, Debug)]
struct Blk {
    prim: Prim,
    indent: Option<usize>,
    margin: Option<usize>,
    indent_token: Option<String>,
}

/// ansi.cascadeStylePrimitive
fn cascade_prim(parent: &Prim, child: &Prim, to_block: bool) -> Prim {
    let mut s = child.clone();
    s.color = child.color.or(parent.color);
    s.bg = child.bg.or(parent.bg);
    s.underline = child.underline.or(parent.underline);
    s.bold = child.bold.or(parent.bold);
    s.italic = child.italic.or(parent.italic);
    s.crossed = child.crossed.or(parent.crossed);
    if to_block {
        s.block_prefix = parent.block_prefix.clone();
        s.block_suffix = parent.block_suffix.clone();
        s.prefix = parent.prefix.clone();
        s.suffix = parent.suffix.clone();
    }
    for (dst, src) in [
        (&mut s.block_prefix, &child.block_prefix),
        (&mut s.block_suffix, &child.block_suffix),
        (&mut s.prefix, &child.prefix),
        (&mut s.suffix, &child.suffix),
    ] {
        if !src.is_empty() {
            *dst = src.clone();
        }
    }
    s
}

/// ansi.cascadeStylePrimitives
fn cascade_prims(list: &[&Prim]) -> Prim {
    let mut r = Prim::default();
    for p in list {
        r = cascade_prim(&r, p, true);
    }
    r
}

/// ansi.cascadeStyle
fn cascade_blk(parent: &Blk, child: &Blk, to_block: bool) -> Blk {
    let mut s = child.clone();
    s.prim = cascade_prim(&parent.prim, &child.prim, to_block);
    if to_block {
        s.indent = parent.indent;
        s.margin = parent.margin;
    }
    if child.indent.is_some() {
        s.indent = child.indent;
    }
    s
}

fn to_style(p: &Prim) -> Style {
    let mut s = Style::default();
    if let Some(c) = p.color {
        s = s.fg(c);
    }
    if let Some(c) = p.bg {
        s = s.bg(c);
    }
    if p.underline == Some(true) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    if p.bold == Some(true) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if p.italic == Some(true) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if p.crossed == Some(true) {
        s = s.add_modifier(Modifier::CROSSED_OUT);
    }
    s
}

fn col(s: &str) -> Option<Color> {
    parse_color(s)
}

/// internal/tui/markdown/theme.go CustomDarkStyleConfig
struct Theme {
    document: Blk,
    block_quote: Blk,
    paragraph: Blk,
    list_level_indent: usize,
    heading: Blk,
    h: [Blk; 6],
    text: Prim,
    strikethrough: Prim,
    emph: Prim,
    strong: Prim,
    hr: Prim,
    item: Prim,
    enumeration: Prim,
    task: Prim,
    ticked: String,
    unticked: String,
    link: Prim,
    link_text: Prim,
    image: Prim,
    image_text: Prim,
    code: Blk,
    code_block: Blk,
    html: Prim,
}

fn theme() -> Theme {
    let prim = |c: Option<&str>| Prim {
        color: c.and_then(col),
        ..Default::default()
    };
    let zero = Blk {
        indent: Some(0),
        margin: Some(0),
        indent_token: Some(String::new()),
        ..Default::default()
    };
    let heading = Blk {
        prim: Prim {
            block_suffix: "\n".into(),
            color: col("#82aaff"),
            bold: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let hb = |c: &str, bold: bool| Blk {
        prim: Prim {
            color: col(c),
            bold: bold.then_some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    Theme {
        document: zero.clone(),
        block_quote: Blk {
            prim: prim(Some("#636da6")),
            indent: Some(1),
            indent_token: Some("│ ".into()),
            ..Default::default()
        },
        paragraph: zero,
        list_level_indent: 2,
        heading,
        h: [
            hb("#c099ff", true),
            hb("#82aaff", false),
            hb("#86e1fc", false),
            hb("#c8d3f5", false),
            hb("#c8d3f5", false),
            hb("#c8d3f5", false),
        ],
        text: Prim::default(),
        strikethrough: Prim {
            crossed: Some(true),
            ..Default::default()
        },
        emph: Prim {
            italic: Some(true),
            ..Default::default()
        },
        strong: Prim {
            bold: Some(true),
            ..Default::default()
        },
        hr: Prim {
            color: col("#3b4261"),
            format: "\n--------\n".into(),
            ..Default::default()
        },
        item: Prim {
            block_prefix: "• ".into(),
            ..Default::default()
        },
        enumeration: Prim {
            block_prefix: ". ".into(),
            ..Default::default()
        },
        task: Prim::default(),
        ticked: "[✓] ".into(),
        unticked: "[ ] ".into(),
        link: Prim {
            color: col("#82aaff"),
            underline: Some(false),
            format: icons::LINK.into(),
            ..Default::default()
        },
        link_text: Prim {
            color: col("#82aaff"),
            bold: Some(true),
            ..Default::default()
        },
        image: Prim {
            color: col("#82aaff"),
            underline: Some(false),
            bold: Some(false),
            format: icons::IMAGE.into(),
            ..Default::default()
        },
        image_text: Prim {
            color: col("#82aaff"),
            bold: Some(true),
            format: icons::LINK.into(),
            ..Default::default()
        },
        code: Blk {
            prim: Prim {
                color: col("#ff966c"),
                bg: col("#2f334d"),
                prefix: " ".into(),
                suffix: " ".into(),
                ..Default::default()
            },
            ..Default::default()
        },
        code_block: Blk {
            prim: prim(Some("244")),
            margin: Some(0),
            ..Default::default()
        },
        html: Prim::default(),
    }
}

// ---------------- goldmark-shaped tree ----------------

#[derive(Debug, Clone)]
enum K {
    Document,
    Heading(u8),
    Paragraph,
    Blockquote,
    List { ordered: bool, start: u64 },
    Item,
    TextBlock,
    Text(String),
    Emph(u8),
    Strike,
    CodeSpan(String),
    Link(String),
    AutoLink { url: String, email: bool },
    Image(String),
    Code { lang: String, code: String },
    HtmlBlock(String),
    RawHtml(String),
    Rule,
    TaskCheck(bool),
    Table,
}

#[derive(Debug, Clone)]
struct Node {
    k: K,
    ch: Vec<Node>,
}

impl Node {
    fn new(k: K) -> Node {
        Node { k, ch: vec![] }
    }
    fn is_inline(&self) -> bool {
        matches!(
            self.k,
            K::Text(_)
                | K::Emph(_)
                | K::Strike
                | K::CodeSpan(_)
                | K::Link(_)
                | K::AutoLink { .. }
                | K::Image(_)
                | K::RawHtml(_)
                | K::TaskCheck(_)
        )
    }
    /// The plain text of a node (goldmark Node.Text).
    fn plain(&self) -> String {
        match &self.k {
            K::Text(t) => t.trim_end_matches('\n').to_string(),
            K::CodeSpan(t) => t.clone(),
            K::AutoLink { url, .. } => url.clone(),
            _ => self.ch.iter().map(|c| c.plain()).collect(),
        }
    }
}

fn is_trigger(c: char) -> bool {
    c.is_whitespace() || matches!(c, '*' | '_' | '~' | '(')
}

fn url_end(s: &str) -> Option<usize> {
    static URL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static WWW: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let url = URL.get_or_init(|| {
        regex::Regex::new(r#"^(?:http|https|ftp)://[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-z]+(?::\d+)?(?:[/#?][-a-zA-Z0-9@:%_+.~#$!?&/=\(\);,'">\^{}\[\]`]*)?"#).unwrap()
    });
    let www = WWW.get_or_init(|| {
        regex::Regex::new(r#"^www\.[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-z]+(?:[/#?][-a-zA-Z0-9@:%_\+.~#!?&/=\(\);,'">\^{}\[\]`]*)?"#).unwrap()
    });
    let m = if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("ftp://") {
        url.find(s)
    } else if s.starts_with("www.") {
        www.find(s)
    } else {
        None
    }?;
    let b = s.as_bytes();
    let mut end = m.end();
    match b[end - 1] {
        b'.' => end -= 1,
        b')' => {
            let mut closing: i64 = 0;
            for &c in b[..end].iter().rev() {
                if c == b')' {
                    closing += 1;
                } else if c == b'(' {
                    closing -= 1;
                }
            }
            if closing > 0 {
                end -= closing as usize;
            }
        }
        b';' => {
            let mut i = end as i64 - 2;
            while i >= 0 && b[i as usize].is_ascii_alphanumeric() {
                i -= 1;
            }
            if i != end as i64 - 2 && i >= 0 && b[i as usize] == b'&' {
                end = i as usize;
            }
        }
        _ => {}
    }
    let mut i = end;
    while i > 1
        && matches!(
            b[i - 1],
            b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~'
        )
    {
        i -= 1;
    }
    Some(i)
}

fn email_end(s: &str) -> Option<usize> {
    static DOMAIN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let b = s.as_bytes();
    if b.first().map(|c| c.is_ascii_punctuation()).unwrap_or(true) {
        return None;
    }
    let local = |c: u8| c.is_ascii_alphanumeric() || b"!#$%&'*+/=?^_`{|}~-.".contains(&c);
    let mut i = 0;
    while i < b.len() && local(b[i]) {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'@' {
        return None;
    }
    let at = i;
    let re = DOMAIN.get_or_init(|| {
        regex::Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+").unwrap()
    });
    let m = re.find(&s[at + 1..])?;
    let mut stop = at + 1 + m.end();
    if !s[at..stop].contains('.') {
        return None;
    }
    if b[stop - 1] == b'.' {
        stop -= 1;
    }
    if stop < b.len() && (b[stop] == b'-' || b[stop] == b'_') {
        return None;
    }
    Some(stop)
}

/// goldmark's GFM linkify, applied to a text run outside links.
fn linkify(text: &str, out: &mut Vec<Node>) {
    let mut buf = String::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    let mut prev: Option<char> = None;
    while i < text.len() {
        let at_trigger = prev.map(is_trigger).unwrap_or(true);
        if at_trigger {
            let rest = &text[i..];
            let (end, email) = match url_end(rest) {
                Some(e) => (Some(e), false),
                None => (email_end(rest), true),
            };
            if let Some(e) = end.filter(|e| *e > 0) {
                if !buf.is_empty() {
                    out.push(Node::new(K::Text(std::mem::take(&mut buf))));
                }
                let url = rest[..e].to_string();
                let mut n = Node::new(K::AutoLink {
                    url: url.clone(),
                    email,
                });
                n.ch.push(Node::new(K::Text(url)));
                out.push(n);
                i += e;
                prev = text[..i].chars().last();
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        prev = Some(ch);
        i += ch.len_utf8();
        let _ = bytes;
    }
    if !buf.is_empty() {
        out.push(Node::new(K::Text(buf)));
    }
}

fn parse(md: &str) -> Node {
    use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_DEFINITION_LIST);
    let mut stack: Vec<Node> = vec![Node::new(K::Document)];
    let in_link = |stack: &Vec<Node>| {
        stack
            .iter()
            .any(|n| matches!(n.k, K::Link(_) | K::Image(_) | K::AutoLink { .. }))
    };
    let push_text = |stack: &mut Vec<Node>, t: &str, linkable: bool| {
        let parent = stack.last_mut().unwrap();
        if linkable {
            let mut nodes = vec![];
            linkify(t, &mut nodes);
            for n in nodes {
                match (parent.ch.last_mut(), &n.k) {
                    (
                        Some(Node {
                            k: K::Text(prev), ..
                        }),
                        K::Text(cur),
                    ) => prev.push_str(cur),
                    _ => parent.ch.push(n),
                }
            }
        } else if let Some(Node {
            k: K::Text(prev), ..
        }) = parent.ch.last_mut()
        {
            prev.push_str(t);
        } else {
            parent.ch.push(Node::new(K::Text(t.to_string())));
        }
    };
    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Start(tag) => {
                let k = match tag {
                    Tag::Paragraph => K::Paragraph,
                    Tag::Heading { level, .. } => K::Heading(level as u8),
                    Tag::BlockQuote(_) => K::Blockquote,
                    Tag::CodeBlock(kind) => K::Code {
                        lang: match kind {
                            CodeBlockKind::Fenced(l) => {
                                l.split_whitespace().next().unwrap_or("").to_string()
                            }
                            CodeBlockKind::Indented => String::new(),
                        },
                        code: String::new(),
                    },
                    Tag::List(start) => K::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                    },
                    Tag::Item => K::Item,
                    Tag::Emphasis => K::Emph(1),
                    Tag::Strong => K::Emph(2),
                    Tag::Strikethrough => K::Strike,
                    Tag::Link {
                        link_type: LinkType::Autolink,
                        dest_url,
                        ..
                    } => K::AutoLink {
                        url: dest_url.to_string(),
                        email: false,
                    },
                    Tag::Link {
                        link_type: LinkType::Email,
                        dest_url,
                        ..
                    } => K::AutoLink {
                        url: dest_url.to_string(),
                        email: true,
                    },
                    Tag::Link { dest_url, .. } => K::Link(dest_url.to_string()),
                    Tag::Image { dest_url, .. } => K::Image(dest_url.to_string()),
                    Tag::HtmlBlock => K::HtmlBlock(String::new()),
                    Tag::Table(_) => K::Table,
                    _ => K::TextBlock,
                };
                stack.push(Node::new(k));
            }
            Event::End(tag) => {
                let mut n = stack.pop().unwrap();
                if matches!(tag, TagEnd::Item) {
                    // goldmark wraps a tight item's inline content in a TextBlock.
                    let mut ch = vec![];
                    let mut run: Vec<Node> = vec![];
                    for c in n.ch.drain(..) {
                        if c.is_inline() {
                            run.push(c);
                        } else {
                            if !run.is_empty() {
                                ch.push(Node {
                                    k: K::TextBlock,
                                    ch: std::mem::take(&mut run),
                                });
                            }
                            ch.push(c);
                        }
                    }
                    if !run.is_empty() {
                        ch.push(Node {
                            k: K::TextBlock,
                            ch: run,
                        });
                    }
                    n.ch = ch;
                }
                if matches!(n.k, K::TextBlock) && !matches!(tag, TagEnd::Item) && n.ch.is_empty() {
                    continue;
                }
                stack.last_mut().unwrap().ch.push(n);
            }
            Event::Text(t) => {
                let top = stack.last_mut().unwrap();
                match &mut top.k {
                    K::Code { code, .. } => code.push_str(&t),
                    K::HtmlBlock(h) => h.push_str(&t),
                    _ => {
                        let linkable = !in_link(&stack);
                        push_text(&mut stack, &t, linkable);
                    }
                }
            }
            Event::Code(t) => stack
                .last_mut()
                .unwrap()
                .ch
                .push(Node::new(K::CodeSpan(t.to_string()))),
            Event::SoftBreak | Event::HardBreak => push_text(&mut stack, "\n", false),
            Event::Rule => stack.last_mut().unwrap().ch.push(Node::new(K::Rule)),
            Event::TaskListMarker(c) => stack
                .last_mut()
                .unwrap()
                .ch
                .push(Node::new(K::TaskCheck(c))),
            Event::Html(h) => {
                if let K::HtmlBlock(b) = &mut stack.last_mut().unwrap().k {
                    b.push_str(&h);
                }
            }
            Event::InlineHtml(h) => stack
                .last_mut()
                .unwrap()
                .ch
                .push(Node::new(K::RawHtml(h.to_string()))),
            _ => {}
        }
    }
    stack.pop().unwrap()
}

/// bluemonday StrictPolicy + TrimSpace + html.UnescapeString.
fn sanitize_html(s: &str) -> String {
    static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static DROP: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let drop = DROP.get_or_init(|| {
        regex::Regex::new(r"(?is)<(script|style)[^>]*>.*?</(script|style)>").unwrap()
    });
    let tag = TAG.get_or_init(|| regex::Regex::new(r"(?s)<[^>]*>").unwrap());
    let s = drop.replace_all(s, "");
    let s = tag.replace_all(&s, "");
    unescape_html(s.trim())
}

fn unescape_html(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
}

// ---------------- renderer ----------------

struct BlockEl {
    block: Vec<Cell>,
    style: Blk,
}

#[derive(Clone, Copy)]
enum W {
    Out,
    Block(usize),
}

struct Ctx {
    width: usize,
    th: Theme,
    stack: Vec<BlockEl>,
    out: Vec<Cell>,
    /// Enclosing lists: (ordered, start, items seen so far).
    items: Vec<(bool, u64, u64)>,
    /// For each enclosing node, whether it is a list item.
    parents: Vec<bool>,
}

const BREAKPOINTS: &str = " ,.;-+|";

fn text_cells(s: &str, style: Style) -> Vec<Cell> {
    s.chars().map(|c| (c, style)).collect()
}

impl Ctx {
    fn buf(&mut self, w: W) -> &mut Vec<Cell> {
        match w {
            W::Out => &mut self.out,
            W::Block(i) => &mut self.stack[i].block,
        }
    }
    fn cur_w(&self) -> W {
        if self.stack.is_empty() {
            W::Out
        } else {
            W::Block(self.stack.len() - 1)
        }
    }
    fn parent_w(&self) -> W {
        if self.stack.len() >= 2 {
            W::Block(self.stack.len() - 2)
        } else {
            W::Out
        }
    }
    fn current(&self) -> Blk {
        self.stack
            .last()
            .map(|b| b.style.clone())
            .unwrap_or_default()
    }
    fn parent(&self) -> Blk {
        if self.stack.len() >= 2 {
            self.stack[self.stack.len() - 2].style.clone()
        } else {
            Blk::default()
        }
    }
    /// BlockStack.Width
    fn width(&self) -> usize {
        let indent: usize = self.stack.iter().map(|b| b.style.indent.unwrap_or(0)).sum();
        let margin: usize = self.stack.iter().map(|b| b.style.margin.unwrap_or(0)).sum();
        self.width.saturating_sub(indent + margin * 2)
    }
    fn render_text(&mut self, w: W, rules: &Prim, s: &str) {
        if s.is_empty() {
            return;
        }
        let st = to_style(rules);
        self.buf(w).extend(text_cells(s, st));
    }
    fn write(&mut self, w: W, cells: &[Cell]) {
        self.buf(w).extend_from_slice(cells);
    }

    /// MarginWriter: indent tokens (parent style) at each line start and
    /// padding to the block width (rules style) before each newline.
    fn margin_write(&mut self, w: W, rules: &Blk, cells: &[Cell]) {
        let indent = rules.indent.unwrap_or(0) + rules.margin.unwrap_or(0);
        let token = rules.indent_token.clone().unwrap_or_else(|| " ".into());
        let token_style = to_style(&self.parent().prim);
        let pad_style = to_style(&rules.prim);
        let width = self.width();
        let mut out: Vec<Cell> = vec![];
        let mut line_w = 0usize;
        let mut at_line_start = true;
        for &(c, st) in cells {
            if at_line_start {
                for _ in 0..indent {
                    out.extend(text_cells(&token, token_style));
                }
                at_line_start = false;
            }
            if c == '\n' {
                if width > 0 && line_w < width {
                    out.extend(std::iter::repeat_n((' ', pad_style), width - line_w));
                }
                line_w = 0;
                at_line_start = true;
            } else {
                line_w += str_width(&c.to_string());
            }
            out.push((c, st));
        }
        self.write(w, &out);
    }

    fn push_block(&mut self, style: Blk) {
        self.stack.push(BlockEl {
            block: vec![],
            style,
        });
    }

    /// BlockElement.Render
    fn block_render(&mut self, w: W, style: Blk) {
        self.push_block(style.clone());
        let parent = self.parent().prim;
        self.render_text(w, &parent, &style.prim.block_prefix);
        let cur = self.current().prim;
        let cw = self.cur_w();
        self.render_text(cw, &cur, &style.prim.prefix);
    }

    /// BlockElement.Finish
    fn block_finish(&mut self, w: W, margin: bool, newline: bool) {
        let cur = self.current();
        if margin {
            let block = std::mem::take(&mut self.stack.last_mut().unwrap().block);
            let mut s = join_wrapped(&block, self.width(), BREAKPOINTS);
            if newline {
                s.push(('\n', Style::default()));
            }
            self.margin_write(w, &cur, &s);
        } else {
            let block = std::mem::take(&mut self.stack.last_mut().unwrap().block);
            let pw = self.parent_w();
            self.write(pw, &block);
        }
        self.render_text(w, &cur.prim, &cur.prim.suffix);
        let parent = self.parent().prim;
        self.render_text(w, &parent, &cur.prim.block_suffix);
        self.stack.pop();
    }

    /// BaseElement.doRender
    fn base_render(
        &mut self,
        w: W,
        token: &str,
        prefix: &str,
        suffix: &str,
        st1: &Prim,
        st2: &Prim,
    ) {
        self.render_text(w, st1, prefix);
        self.render_text(w, st1, &st2.block_prefix);
        self.render_text(w, st2, &st2.prefix);
        let s = if st2.format.is_empty() {
            token.to_string()
        } else {
            st2.format.replace("{{.text}}", token)
        };
        self.render_text(w, st2, &s);
        self.render_text(w, st2, &st2.suffix);
        self.render_text(w, st1, &st2.block_suffix);
        self.render_text(w, st1, suffix);
    }

    /// BaseElement.Render
    fn base(&mut self, w: W, token: &str, prefix: &str, suffix: &str, style: &Prim) {
        let st1 = self.current().prim;
        let st2 = cascade_blk(
            &self.current(),
            &Blk {
                prim: style.clone(),
                ..Default::default()
            },
            false,
        )
        .prim;
        self.base_render(w, token, prefix, suffix, &st1, &st2);
    }

    /// BaseElement.StyleOverrideRender
    fn base_override(&mut self, w: W, token: &str, style: &Prim, over: &Prim) {
        let cur = self.current();
        let st1 = cascade_prims(&[&cur.prim, over]);
        let with = cascade_blk(
            &cur,
            &Blk {
                prim: style.clone(),
                ..Default::default()
            },
            false,
        )
        .prim;
        let st2 = cascade_prims(&[&with, over]);
        self.base_render(w, token, "", "", &st1, &st2);
    }

    /// Renders an inline node that its parent renders itself (isChild nodes).
    fn inline(&mut self, w: W, n: &Node, over: Option<&Prim>) {
        let th_text = self.th.text.clone();
        match &n.k {
            K::Text(t) => match over {
                Some(o) => self.base_override(w, t, &th_text, o),
                None => self.base(w, t, "", "", &th_text),
            },
            K::Emph(level) => {
                let base = if *level > 1 {
                    self.th.strong.clone()
                } else {
                    self.th.emph.clone()
                };
                let style = match over {
                    Some(o) => cascade_prims(&[&base, o]),
                    None => base,
                };
                for c in &n.ch {
                    match &c.k {
                        K::Text(_) | K::Emph(_) | K::Strike => self.inline(w, c, Some(&style)),
                        _ => self.inline(w, c, None),
                    }
                }
            }
            K::Strike => {
                let token = n.plain();
                let st = self.th.strikethrough.clone();
                match over {
                    Some(o) => self.base_override(w, &token, &st, o),
                    None => self.base(w, &token, "", "", &st),
                }
            }
            K::CodeSpan(t) => {
                let st = cascade_blk(&self.current(), &self.th.code, false).prim;
                let s = format!("{}{}{}", st.prefix, t, st.suffix);
                self.render_text(w, &st, &s);
            }
            K::Link(url) => self.link(w, url, &n.ch, false, false),
            K::AutoLink { url, email } => {
                let ch = if n.ch.is_empty() {
                    vec![Node::new(K::Text(url.clone()))]
                } else {
                    n.ch.clone()
                };
                let url = if *email && !url.to_lowercase().starts_with("mailto:") {
                    format!("mailto:{url}")
                } else {
                    url.clone()
                };
                self.link(w, &url, &ch, !*email, *email);
            }
            K::Image(url) => {
                let text = n.plain();
                if !text.is_empty() {
                    let st = self.th.image_text.clone();
                    self.base(w, &text, "", "", &st);
                }
                if !url.is_empty() {
                    let st = self.th.image.clone();
                    self.base(w, url, " ", "", &st);
                }
            }
            K::RawHtml(h) => {
                let st = self.th.html.clone();
                self.base(w, &sanitize_html(h), "", "", &st);
            }
            K::TaskCheck(_) => {}
            _ => {}
        }
    }

    /// LinkElement.Render
    fn link(&mut self, w: W, url: &str, children: &[Node], skip_text: bool, skip_href: bool) {
        let valid = !url.starts_with('#');
        if !skip_text {
            let lt = self.th.link_text.clone();
            for c in children {
                match &c.k {
                    K::Text(_) | K::Emph(_) | K::Strike => self.inline(w, c, Some(&lt)),
                    _ => {
                        // Rendered first, then re-rendered as a LinkText token.
                        let save = std::mem::take(self.buf(w));
                        self.inline(w, c, None);
                        let inner = std::mem::replace(self.buf(w), save);
                        let st1 = self.current().prim;
                        let st2 = cascade_blk(
                            &self.current(),
                            &Blk {
                                prim: lt.clone(),
                                ..Default::default()
                            },
                            false,
                        )
                        .prim;
                        self.render_text(w, &st1, &st2.block_prefix);
                        self.render_text(w, &st2, &st2.prefix);
                        // The inner SGR codes only override what they set, so the
                        // LinkText attributes (bold) carry into the inner text.
                        let outer = to_style(&st2);
                        let inner: Vec<Cell> = inner
                            .into_iter()
                            .map(|(c, s)| (c, outer.patch(s)))
                            .collect();
                        self.write(w, &inner);
                        self.render_text(w, &st2, &st2.suffix);
                        self.render_text(w, &st1, &st2.block_suffix);
                    }
                }
            }
        }
        if !skip_href && valid {
            let prefix = if skip_text { "" } else { " " };
            let st = self.th.link.clone();
            self.base(w, url, prefix, "", &st);
        }
    }

    fn node(&mut self, n: &Node, first: bool, next_sibling: Option<&Node>) {
        // entering
        let w = self.cur_w();
        match &n.k {
            K::Document => {
                let st = self.th.document.clone();
                self.block_render(w, st);
                self.children(n);
                self.block_finish(W::Out, true, false);
            }
            K::Heading(level) => {
                let idx = (*level as usize).clamp(1, 6) - 1;
                let rules = cascade_blk(
                    &cascade_blk(&Blk::default(), &self.th.heading, true),
                    &self.th.h[idx],
                    true,
                );
                if !first {
                    let cur = self.current().prim;
                    self.render_text(w, &cur, "\n");
                }
                let st = cascade_blk(&self.current(), &rules, false);
                self.push_block(st);
                let parent = self.parent().prim;
                self.render_text(w, &parent, &rules.prim.block_prefix);
                let cur = self.current().prim;
                let cw = self.cur_w();
                self.render_text(cw, &cur, &rules.prim.prefix);
                self.children(n);
                // HeadingElement.Finish
                let pw = self.parent_w();
                let rules = self.current();
                let block = std::mem::take(&mut self.stack.last_mut().unwrap().block);
                let flow = join_wrapped(&block, self.width(), "");
                self.margin_write(pw, &rules, &flow);
                self.render_text(pw, &rules.prim, &rules.prim.suffix);
                let parent = self.parent().prim;
                self.render_text(pw, &parent, &rules.prim.block_suffix);
                self.stack.pop();
            }
            K::Paragraph if self.in_item() => self.children(n),
            K::Paragraph => {
                if !first {
                    self.write(w, &[('\n', Style::default())]);
                }
                let rules = self.th.paragraph.clone();
                let st = cascade_blk(&self.current(), &rules, false);
                self.push_block(st);
                let parent = self.parent().prim;
                self.render_text(w, &parent, &rules.prim.block_prefix);
                let cur = self.current().prim;
                let cw = self.cur_w();
                self.render_text(cw, &cur, &rules.prim.prefix);
                self.children(n);
                // ParagraphElement.Finish
                let pw = self.parent_w();
                let rules = self.current();
                let block = std::mem::take(&mut self.stack.last_mut().unwrap().block);
                let text: String = block.iter().map(|c| c.0).collect();
                if !text.trim().is_empty() {
                    let blk: Vec<Cell> = block
                        .into_iter()
                        .map(|(c, s)| if c == '\n' { (' ', s) } else { (c, s) })
                        .collect();
                    let mut flow = join_wrapped(&blk, self.width(), "");
                    flow.push(('\n', Style::default()));
                    self.margin_write(pw, &rules, &flow);
                }
                self.render_text(pw, &rules.prim, &rules.prim.suffix);
                let parent = self.parent().prim;
                self.render_text(pw, &parent, &rules.prim.block_suffix);
                self.stack.pop();
            }
            K::Blockquote => {
                self.write(w, &[('\n', Style::default())]);
                let st = cascade_blk(&self.current(), &self.th.block_quote, false);
                self.block_render(w, st);
                self.children(n);
                let pw = self.parent_w();
                self.block_finish(pw, true, false);
            }
            K::List { .. } => {
                self.write(w, &[('\n', Style::default())]);
                let nested = self.stack_has_list();
                let s = Blk {
                    indent: Some(if nested { self.th.list_level_indent } else { 0 }),
                    ..Default::default()
                };
                let st = cascade_blk(&self.current(), &s, false);
                self.block_render(w, st);
                self.stack.last_mut().unwrap().style.indent_token = None;
                self.children(n);
                let pw = self.parent_w();
                self.block_finish(pw, true, true);
            }
            K::Item => {
                let task = n.ch.first().and_then(|c| c.ch.first()).and_then(|c| {
                    if let K::TaskCheck(b) = c.k {
                        Some(b)
                    } else {
                        None
                    }
                });
                if let Some(checked) = task {
                    let pre = if checked {
                        self.th.ticked.clone()
                    } else {
                        self.th.unticked.clone()
                    };
                    let st = self.th.task.clone();
                    self.base(w, "", &pre, "", &st);
                } else if let Some((ordered, enumeration)) = self.item_info() {
                    if ordered {
                        let st = self.th.enumeration.clone();
                        self.base(w, "", &enumeration.to_string(), "", &st);
                    } else {
                        let st = self.th.item.clone();
                        self.base(w, "", "", "", &st);
                    }
                }
                self.children(n);
                let last_is_list =
                    n.ch.last()
                        .map(|c| matches!(c.k, K::List { .. }))
                        .unwrap_or(false);
                if !(last_is_list || next_sibling.is_none()) {
                    let cw = self.cur_w();
                    self.write(cw, &[('\n', Style::default())]);
                }
            }
            K::TextBlock => self.children(n),
            K::Code { lang, code } => {
                self.write(w, &[('\n', Style::default())]);
                let cur = self.current().prim;
                let rules = self.th.code_block.clone();
                self.render_text(w, &cur, &rules.prim.block_prefix);
                let mut cells: Vec<Cell> = vec![];
                let lines = highlight::highlight(code, lang);
                for l in lines {
                    for s in l.spans {
                        cells.extend(s.content.chars().map(|c| (c, s.style)));
                    }
                    cells.push(('\n', Style::default()));
                }
                if code.is_empty() {
                    cells.clear();
                }
                self.write(w, &cells);
                self.render_text(w, &cur, &rules.prim.block_suffix);
            }
            K::HtmlBlock(h) => {
                let st = self.th.html.clone();
                self.base(w, &sanitize_html(h), "", "", &st);
            }
            K::Rule => {
                let st = self.th.hr.clone();
                self.base(w, "", "", "", &st);
            }
            K::Table => {}
            _ => self.inline(w, n, None),
        }
    }

    fn children(&mut self, n: &Node) {
        self.parents.push(matches!(n.k, K::Item));
        let saved = self.items.len();
        if let K::List { ordered, start } = n.k {
            self.items.push((ordered, start, 0));
        }
        for (i, c) in n.ch.iter().enumerate() {
            if matches!(c.k, K::Item)
                && let Some(top) = self.items.last_mut()
            {
                top.2 += 1;
            }
            self.node(c, i == 0, n.ch.get(i + 1));
        }
        self.items.truncate(saved);
        self.parents.pop();
    }
}

// List bookkeeping kept outside the Go-shaped core for clarity.
impl Ctx {
    fn in_item(&self) -> bool {
        self.parents.last().copied().unwrap_or(false)
    }
    fn stack_has_list(&self) -> bool {
        !self.items.is_empty()
    }
    fn item_info(&self) -> Option<(bool, u64)> {
        self.items
            .last()
            .map(|(o, start, idx)| (*o, if *o { start + idx - 1 } else { 0 }))
    }
}

/// lipgloss.Wrap over styled cells, returned as one cell run with newlines.
fn join_wrapped(cells: &[Cell], width: usize, breakpoints: &str) -> Vec<Cell> {
    if width < 1 {
        return cells.to_vec();
    }
    let lines = ansi_wrap_with(cells, width, breakpoints);
    let mut out = vec![];
    for (i, l) in lines.into_iter().enumerate() {
        if i > 0 {
            out.push(('\n', Style::default()));
        }
        out.extend(l);
    }
    out
}

pub fn render(md: &str, width: usize) -> Block {
    let tree = parse(md);
    let mut ctx = Ctx {
        width,
        th: theme(),
        stack: vec![],
        out: vec![],
        items: vec![],
        parents: vec![],
    };
    ctx.node(&tree, true, None);
    let mut lines: Block = vec![];
    let mut cur: Vec<Cell> = vec![];
    for c in ctx.out {
        if c.0 == '\n' {
            lines.push(cells_to_line(&cur));
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    lines.push(cells_to_line(&cur));
    lines.into_iter().map(|l: Line<'static>| l).collect()
}
