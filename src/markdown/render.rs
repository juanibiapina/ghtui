//! Renders markdown to styled lines in the ghtui Tokyo Night style. Markdown is
//! parsed with pulldown-cmark into a tree with GFM linkify, then laid out with a
//! block stack, margin/indent/padding writers and wrapping passes.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use crate::highlight;
use crate::icons;
use crate::text::{Block, ansi_wrap_with, cells_to_line, str_width};
use crate::theme::parse_color;

type Cell = (char, Style);

#[derive(Clone, Default, Debug)]
struct StylePrimitive {
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
struct StyleBlock {
    primitive: StylePrimitive,
    indent: Option<usize>,
    margin: Option<usize>,
    indent_token: Option<String>,
}

fn cascade_primitive(
    parent: &StylePrimitive,
    child: &StylePrimitive,
    to_block: bool,
) -> StylePrimitive {
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

fn cascade_primitives(list: &[&StylePrimitive]) -> StylePrimitive {
    let mut r = StylePrimitive::default();
    for p in list {
        r = cascade_primitive(&r, p, true);
    }
    r
}

fn cascade_block(parent: &StyleBlock, child: &StyleBlock, to_block: bool) -> StyleBlock {
    let mut s = child.clone();
    s.primitive = cascade_primitive(&parent.primitive, &child.primitive, to_block);
    if to_block {
        s.indent = parent.indent;
        s.margin = parent.margin;
    }
    if child.indent.is_some() {
        s.indent = child.indent;
    }
    s
}

fn to_style(p: &StylePrimitive) -> Style {
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

struct Theme {
    document: StyleBlock,
    block_quote: StyleBlock,
    paragraph: StyleBlock,
    list_level_indent: usize,
    heading: StyleBlock,
    h: [StyleBlock; 6],
    text: StylePrimitive,
    strikethrough: StylePrimitive,
    emph: StylePrimitive,
    strong: StylePrimitive,
    hr: StylePrimitive,
    item: StylePrimitive,
    enumeration: StylePrimitive,
    task: StylePrimitive,
    ticked: String,
    unticked: String,
    link: StylePrimitive,
    link_text: StylePrimitive,
    image: StylePrimitive,
    image_text: StylePrimitive,
    code: StyleBlock,
    code_block: StyleBlock,
    html: StylePrimitive,
}

fn theme() -> Theme {
    let primitive = |c: Option<&str>| StylePrimitive {
        color: c.and_then(parse_color),
        ..Default::default()
    };
    let zero = StyleBlock {
        indent: Some(0),
        margin: Some(0),
        indent_token: Some(String::new()),
        ..Default::default()
    };
    let heading = StyleBlock {
        primitive: StylePrimitive {
            block_suffix: "\n".into(),
            color: parse_color("#82aaff"),
            bold: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let hb = |c: &str, bold: bool| StyleBlock {
        primitive: StylePrimitive {
            color: parse_color(c),
            bold: bold.then_some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    Theme {
        document: zero.clone(),
        block_quote: StyleBlock {
            primitive: primitive(Some("#636da6")),
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
        text: StylePrimitive::default(),
        strikethrough: StylePrimitive {
            crossed: Some(true),
            ..Default::default()
        },
        emph: StylePrimitive {
            italic: Some(true),
            ..Default::default()
        },
        strong: StylePrimitive {
            bold: Some(true),
            ..Default::default()
        },
        hr: StylePrimitive {
            color: parse_color("#3b4261"),
            format: "\n--------\n".into(),
            ..Default::default()
        },
        item: StylePrimitive {
            block_prefix: "• ".into(),
            ..Default::default()
        },
        enumeration: StylePrimitive {
            block_prefix: ". ".into(),
            ..Default::default()
        },
        task: StylePrimitive::default(),
        ticked: "[✓] ".into(),
        unticked: "[ ] ".into(),
        link: StylePrimitive {
            color: parse_color("#82aaff"),
            underline: Some(false),
            format: icons::LINK.into(),
            ..Default::default()
        },
        link_text: StylePrimitive {
            color: parse_color("#82aaff"),
            bold: Some(true),
            ..Default::default()
        },
        image: StylePrimitive {
            color: parse_color("#82aaff"),
            underline: Some(false),
            bold: Some(false),
            format: icons::IMAGE.into(),
            ..Default::default()
        },
        image_text: StylePrimitive {
            color: parse_color("#82aaff"),
            bold: Some(true),
            format: icons::LINK.into(),
            ..Default::default()
        },
        code: StyleBlock {
            primitive: StylePrimitive {
                color: parse_color("#ff966c"),
                bg: parse_color("#2f334d"),
                prefix: " ".into(),
                suffix: " ".into(),
                ..Default::default()
            },
            ..Default::default()
        },
        code_block: StyleBlock {
            primitive: primitive(Some("244")),
            margin: Some(0),
            ..Default::default()
        },
        html: StylePrimitive::default(),
    }
}

#[derive(Debug, Clone)]
enum NodeKind {
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
    k: NodeKind,
    ch: Vec<Node>,
}

impl Node {
    fn new(k: NodeKind) -> Node {
        Node { k, ch: vec![] }
    }
    fn is_inline(&self) -> bool {
        matches!(
            self.k,
            NodeKind::Text(_)
                | NodeKind::Emph(_)
                | NodeKind::Strike
                | NodeKind::CodeSpan(_)
                | NodeKind::Link(_)
                | NodeKind::AutoLink { .. }
                | NodeKind::Image(_)
                | NodeKind::RawHtml(_)
                | NodeKind::TaskCheck(_)
        )
    }
    /// The plain text of a node.
    fn plain(&self) -> String {
        match &self.k {
            NodeKind::Text(t) => t.trim_end_matches('\n').to_string(),
            NodeKind::CodeSpan(t) => t.clone(),
            NodeKind::AutoLink { url, .. } => url.clone(),
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
                    out.push(Node::new(NodeKind::Text(std::mem::take(&mut buf))));
                }
                let url = rest[..e].to_string();
                let mut n = Node::new(NodeKind::AutoLink {
                    url: url.clone(),
                    email,
                });
                n.ch.push(Node::new(NodeKind::Text(url)));
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
        out.push(Node::new(NodeKind::Text(buf)));
    }
}

fn parse(md: &str) -> Node {
    use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_DEFINITION_LIST);
    let mut stack: Vec<Node> = vec![Node::new(NodeKind::Document)];
    let in_link = |stack: &Vec<Node>| {
        stack.iter().any(|n| {
            matches!(
                n.k,
                NodeKind::Link(_) | NodeKind::Image(_) | NodeKind::AutoLink { .. }
            )
        })
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
                            k: NodeKind::Text(prev),
                            ..
                        }),
                        NodeKind::Text(cur),
                    ) => prev.push_str(cur),
                    _ => parent.ch.push(n),
                }
            }
        } else if let Some(Node {
            k: NodeKind::Text(prev),
            ..
        }) = parent.ch.last_mut()
        {
            prev.push_str(t);
        } else {
            parent.ch.push(Node::new(NodeKind::Text(t.to_string())));
        }
    };
    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Start(tag) => {
                let k = match tag {
                    Tag::Paragraph => NodeKind::Paragraph,
                    Tag::Heading { level, .. } => NodeKind::Heading(level as u8),
                    Tag::BlockQuote(_) => NodeKind::Blockquote,
                    Tag::CodeBlock(kind) => NodeKind::Code {
                        lang: match kind {
                            CodeBlockKind::Fenced(l) => {
                                l.split_whitespace().next().unwrap_or("").to_string()
                            }
                            CodeBlockKind::Indented => String::new(),
                        },
                        code: String::new(),
                    },
                    Tag::List(start) => NodeKind::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                    },
                    Tag::Item => NodeKind::Item,
                    Tag::Emphasis => NodeKind::Emph(1),
                    Tag::Strong => NodeKind::Emph(2),
                    Tag::Strikethrough => NodeKind::Strike,
                    Tag::Link {
                        link_type: LinkType::Autolink,
                        dest_url,
                        ..
                    } => NodeKind::AutoLink {
                        url: dest_url.to_string(),
                        email: false,
                    },
                    Tag::Link {
                        link_type: LinkType::Email,
                        dest_url,
                        ..
                    } => NodeKind::AutoLink {
                        url: dest_url.to_string(),
                        email: true,
                    },
                    Tag::Link { dest_url, .. } => NodeKind::Link(dest_url.to_string()),
                    Tag::Image { dest_url, .. } => NodeKind::Image(dest_url.to_string()),
                    Tag::HtmlBlock => NodeKind::HtmlBlock(String::new()),
                    Tag::Table(_) => NodeKind::Table,
                    _ => NodeKind::TextBlock,
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
                                    k: NodeKind::TextBlock,
                                    ch: std::mem::take(&mut run),
                                });
                            }
                            ch.push(c);
                        }
                    }
                    if !run.is_empty() {
                        ch.push(Node {
                            k: NodeKind::TextBlock,
                            ch: run,
                        });
                    }
                    n.ch = ch;
                }
                if matches!(n.k, NodeKind::TextBlock)
                    && !matches!(tag, TagEnd::Item)
                    && n.ch.is_empty()
                {
                    continue;
                }
                stack.last_mut().unwrap().ch.push(n);
            }
            Event::Text(t) => {
                let top = stack.last_mut().unwrap();
                match &mut top.k {
                    NodeKind::Code { code, .. } => code.push_str(&t),
                    NodeKind::HtmlBlock(h) => h.push_str(&t),
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
                .push(Node::new(NodeKind::CodeSpan(t.to_string()))),
            Event::SoftBreak | Event::HardBreak => push_text(&mut stack, "\n", false),
            Event::Rule => stack.last_mut().unwrap().ch.push(Node::new(NodeKind::Rule)),
            Event::TaskListMarker(c) => stack
                .last_mut()
                .unwrap()
                .ch
                .push(Node::new(NodeKind::TaskCheck(c))),
            Event::Html(h) => {
                if let NodeKind::HtmlBlock(b) = &mut stack.last_mut().unwrap().k {
                    b.push_str(&h);
                }
            }
            Event::InlineHtml(h) => stack
                .last_mut()
                .unwrap()
                .ch
                .push(Node::new(NodeKind::RawHtml(h.to_string()))),
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

struct BlockElement {
    block: Vec<Cell>,
    style: StyleBlock,
}

#[derive(Clone, Copy)]
enum Dest {
    Out,
    Block(usize),
}

struct Renderer {
    width: usize,
    theme: Theme,
    stack: Vec<BlockElement>,
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

impl Renderer {
    fn buf(&mut self, w: Dest) -> &mut Vec<Cell> {
        match w {
            Dest::Out => &mut self.out,
            Dest::Block(i) => &mut self.stack[i].block,
        }
    }
    fn cur_w(&self) -> Dest {
        if self.stack.is_empty() {
            Dest::Out
        } else {
            Dest::Block(self.stack.len() - 1)
        }
    }
    fn parent_w(&self) -> Dest {
        if self.stack.len() >= 2 {
            Dest::Block(self.stack.len() - 2)
        } else {
            Dest::Out
        }
    }
    fn current(&self) -> StyleBlock {
        self.stack
            .last()
            .map(|b| b.style.clone())
            .unwrap_or_default()
    }
    fn parent(&self) -> StyleBlock {
        if self.stack.len() >= 2 {
            self.stack[self.stack.len() - 2].style.clone()
        } else {
            StyleBlock::default()
        }
    }
    fn width(&self) -> usize {
        let indent: usize = self.stack.iter().map(|b| b.style.indent.unwrap_or(0)).sum();
        let margin: usize = self.stack.iter().map(|b| b.style.margin.unwrap_or(0)).sum();
        self.width.saturating_sub(indent + margin * 2)
    }
    fn render_text(&mut self, w: Dest, rules: &StylePrimitive, s: &str) {
        if s.is_empty() {
            return;
        }
        let st = to_style(rules);
        self.buf(w).extend(text_cells(s, st));
    }
    fn write(&mut self, w: Dest, cells: &[Cell]) {
        self.buf(w).extend_from_slice(cells);
    }

    /// Writes indent tokens (parent style) at each line start and padding to the
    /// block width (rules style) before each newline.
    fn margin_write(&mut self, w: Dest, rules: &StyleBlock, cells: &[Cell]) {
        let indent = rules.indent.unwrap_or(0) + rules.margin.unwrap_or(0);
        let token = rules.indent_token.clone().unwrap_or_else(|| " ".into());
        let token_style = to_style(&self.parent().primitive);
        let pad_style = to_style(&rules.primitive);
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

    fn push_block(&mut self, style: StyleBlock) {
        self.stack.push(BlockElement {
            block: vec![],
            style,
        });
    }

    fn block_render(&mut self, w: Dest, style: StyleBlock) {
        self.push_block(style.clone());
        let parent = self.parent().primitive;
        self.render_text(w, &parent, &style.primitive.block_prefix);
        let cur = self.current().primitive;
        let cw = self.cur_w();
        self.render_text(cw, &cur, &style.primitive.prefix);
    }

    fn block_finish(&mut self, w: Dest, margin: bool, newline: bool) {
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
        self.render_text(w, &cur.primitive, &cur.primitive.suffix);
        let parent = self.parent().primitive;
        self.render_text(w, &parent, &cur.primitive.block_suffix);
        self.stack.pop();
    }

    fn write_styled_token(
        &mut self,
        w: Dest,
        token: &str,
        prefix: &str,
        suffix: &str,
        st1: &StylePrimitive,
        st2: &StylePrimitive,
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

    fn render_token(
        &mut self,
        w: Dest,
        token: &str,
        prefix: &str,
        suffix: &str,
        style: &StylePrimitive,
    ) {
        let st1 = self.current().primitive;
        let st2 = cascade_block(
            &self.current(),
            &StyleBlock {
                primitive: style.clone(),
                ..Default::default()
            },
            false,
        )
        .primitive;
        self.write_styled_token(w, token, prefix, suffix, &st1, &st2);
    }

    fn render_token_with_override(
        &mut self,
        w: Dest,
        token: &str,
        style: &StylePrimitive,
        over: &StylePrimitive,
    ) {
        let cur = self.current();
        let st1 = cascade_primitives(&[&cur.primitive, over]);
        let with = cascade_block(
            &cur,
            &StyleBlock {
                primitive: style.clone(),
                ..Default::default()
            },
            false,
        )
        .primitive;
        let st2 = cascade_primitives(&[&with, over]);
        self.write_styled_token(w, token, "", "", &st1, &st2);
    }

    /// Renders an inline node that its parent renders itself.
    fn inline(&mut self, w: Dest, n: &Node, over: Option<&StylePrimitive>) {
        let th_text = self.theme.text.clone();
        match &n.k {
            NodeKind::Text(t) => match over {
                Some(o) => self.render_token_with_override(w, t, &th_text, o),
                None => self.render_token(w, t, "", "", &th_text),
            },
            NodeKind::Emph(level) => {
                let base = if *level > 1 {
                    self.theme.strong.clone()
                } else {
                    self.theme.emph.clone()
                };
                let style = match over {
                    Some(o) => cascade_primitives(&[&base, o]),
                    None => base,
                };
                for c in &n.ch {
                    match &c.k {
                        NodeKind::Text(_) | NodeKind::Emph(_) | NodeKind::Strike => {
                            self.inline(w, c, Some(&style))
                        }
                        _ => self.inline(w, c, None),
                    }
                }
            }
            NodeKind::Strike => {
                let token = n.plain();
                let st = self.theme.strikethrough.clone();
                match over {
                    Some(o) => self.render_token_with_override(w, &token, &st, o),
                    None => self.render_token(w, &token, "", "", &st),
                }
            }
            NodeKind::CodeSpan(t) => {
                let st = cascade_block(&self.current(), &self.theme.code, false).primitive;
                let s = format!("{}{}{}", st.prefix, t, st.suffix);
                self.render_text(w, &st, &s);
            }
            NodeKind::Link(url) => self.link(w, url, &n.ch, false, false),
            NodeKind::AutoLink { url, email } => {
                let ch = if n.ch.is_empty() {
                    vec![Node::new(NodeKind::Text(url.clone()))]
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
            NodeKind::Image(url) => {
                let text = n.plain();
                if !text.is_empty() {
                    let st = self.theme.image_text.clone();
                    self.render_token(w, &text, "", "", &st);
                }
                if !url.is_empty() {
                    let st = self.theme.image.clone();
                    self.render_token(w, url, " ", "", &st);
                }
            }
            NodeKind::RawHtml(h) => {
                let st = self.theme.html.clone();
                self.render_token(w, &sanitize_html(h), "", "", &st);
            }
            NodeKind::TaskCheck(_) => {}
            _ => {}
        }
    }

    fn link(&mut self, w: Dest, url: &str, children: &[Node], skip_text: bool, skip_href: bool) {
        let valid = !url.starts_with('#');
        if !skip_text {
            let lt = self.theme.link_text.clone();
            for c in children {
                match &c.k {
                    NodeKind::Text(_) | NodeKind::Emph(_) | NodeKind::Strike => {
                        self.inline(w, c, Some(&lt))
                    }
                    _ => {
                        // Rendered first, then re-rendered as a LinkText token.
                        let save = std::mem::take(self.buf(w));
                        self.inline(w, c, None);
                        let inner = std::mem::replace(self.buf(w), save);
                        let st1 = self.current().primitive;
                        let st2 = cascade_block(
                            &self.current(),
                            &StyleBlock {
                                primitive: lt.clone(),
                                ..Default::default()
                            },
                            false,
                        )
                        .primitive;
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
            let st = self.theme.link.clone();
            self.render_token(w, url, prefix, "", &st);
        }
    }

    fn node(&mut self, n: &Node, first: bool, next_sibling: Option<&Node>) {
        let w = self.cur_w();
        match &n.k {
            NodeKind::Document => {
                let st = self.theme.document.clone();
                self.block_render(w, st);
                self.children(n);
                self.block_finish(Dest::Out, true, false);
            }
            NodeKind::Heading(level) => {
                let idx = (*level as usize).clamp(1, 6) - 1;
                let rules = cascade_block(
                    &cascade_block(&StyleBlock::default(), &self.theme.heading, true),
                    &self.theme.h[idx],
                    true,
                );
                if !first {
                    let cur = self.current().primitive;
                    self.render_text(w, &cur, "\n");
                }
                let st = cascade_block(&self.current(), &rules, false);
                self.push_block(st);
                let parent = self.parent().primitive;
                self.render_text(w, &parent, &rules.primitive.block_prefix);
                let cur = self.current().primitive;
                let cw = self.cur_w();
                self.render_text(cw, &cur, &rules.primitive.prefix);
                self.children(n);
                let pw = self.parent_w();
                let rules = self.current();
                let block = std::mem::take(&mut self.stack.last_mut().unwrap().block);
                let flow = join_wrapped(&block, self.width(), "");
                self.margin_write(pw, &rules, &flow);
                self.render_text(pw, &rules.primitive, &rules.primitive.suffix);
                let parent = self.parent().primitive;
                self.render_text(pw, &parent, &rules.primitive.block_suffix);
                self.stack.pop();
            }
            NodeKind::Paragraph if self.in_item() => self.children(n),
            NodeKind::Paragraph => {
                if !first {
                    self.write(w, &[('\n', Style::default())]);
                }
                let rules = self.theme.paragraph.clone();
                let st = cascade_block(&self.current(), &rules, false);
                self.push_block(st);
                let parent = self.parent().primitive;
                self.render_text(w, &parent, &rules.primitive.block_prefix);
                let cur = self.current().primitive;
                let cw = self.cur_w();
                self.render_text(cw, &cur, &rules.primitive.prefix);
                self.children(n);
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
                self.render_text(pw, &rules.primitive, &rules.primitive.suffix);
                let parent = self.parent().primitive;
                self.render_text(pw, &parent, &rules.primitive.block_suffix);
                self.stack.pop();
            }
            NodeKind::Blockquote => {
                self.write(w, &[('\n', Style::default())]);
                let st = cascade_block(&self.current(), &self.theme.block_quote, false);
                self.block_render(w, st);
                self.children(n);
                let pw = self.parent_w();
                self.block_finish(pw, true, false);
            }
            NodeKind::List { .. } => {
                self.write(w, &[('\n', Style::default())]);
                let nested = self.stack_has_list();
                let s = StyleBlock {
                    indent: Some(if nested {
                        self.theme.list_level_indent
                    } else {
                        0
                    }),
                    ..Default::default()
                };
                let st = cascade_block(&self.current(), &s, false);
                self.block_render(w, st);
                self.stack.last_mut().unwrap().style.indent_token = None;
                self.children(n);
                let pw = self.parent_w();
                self.block_finish(pw, true, true);
            }
            NodeKind::Item => {
                let task = n.ch.first().and_then(|c| c.ch.first()).and_then(|c| {
                    if let NodeKind::TaskCheck(b) = c.k {
                        Some(b)
                    } else {
                        None
                    }
                });
                if let Some(checked) = task {
                    let pre = if checked {
                        self.theme.ticked.clone()
                    } else {
                        self.theme.unticked.clone()
                    };
                    let st = self.theme.task.clone();
                    self.render_token(w, "", &pre, "", &st);
                } else if let Some((ordered, enumeration)) = self.item_info() {
                    if ordered {
                        let st = self.theme.enumeration.clone();
                        self.render_token(w, "", &enumeration.to_string(), "", &st);
                    } else {
                        let st = self.theme.item.clone();
                        self.render_token(w, "", "", "", &st);
                    }
                }
                self.children(n);
                let last_is_list =
                    n.ch.last()
                        .map(|c| matches!(c.k, NodeKind::List { .. }))
                        .unwrap_or(false);
                if !(last_is_list || next_sibling.is_none()) {
                    let cw = self.cur_w();
                    self.write(cw, &[('\n', Style::default())]);
                }
            }
            NodeKind::TextBlock => self.children(n),
            NodeKind::Code { lang, code } => {
                self.write(w, &[('\n', Style::default())]);
                let cur = self.current().primitive;
                let rules = self.theme.code_block.clone();
                self.render_text(w, &cur, &rules.primitive.block_prefix);
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
                self.render_text(w, &cur, &rules.primitive.block_suffix);
            }
            NodeKind::HtmlBlock(h) => {
                let st = self.theme.html.clone();
                self.render_token(w, &sanitize_html(h), "", "", &st);
            }
            NodeKind::Rule => {
                let st = self.theme.hr.clone();
                self.render_token(w, "", "", "", &st);
            }
            NodeKind::Table => {}
            _ => self.inline(w, n, None),
        }
    }

    fn children(&mut self, n: &Node) {
        self.parents.push(matches!(n.k, NodeKind::Item));
        let saved = self.items.len();
        if let NodeKind::List { ordered, start } = n.k {
            self.items.push((ordered, start, 0));
        }
        for (i, c) in n.ch.iter().enumerate() {
            if matches!(c.k, NodeKind::Item)
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

impl Renderer {
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

/// Wraps styled cells to a width, returned as one cell run with newlines.
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
    let mut ctx = Renderer {
        width,
        theme: theme(),
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
