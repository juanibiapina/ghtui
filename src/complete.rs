//! Autocomplete for the search bar and the input box: suggestion sources, the
//! fuzzy filter, the popup and key routing.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

use crate::icons;
use crate::input::Editable;
use crate::model::Label;
use crate::mouse::{HitMap, Target};
use crate::section::SPINNER;
use crate::text::{raw, span, truncate_line};
use crate::theme::{self, Theme};

const MAX_VISIBLE: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Search,
    Mention { with_at: bool },
    Labels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub value: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CursorToken {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchState {
    Idle,
    Loading,
    Success,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Completer {
    pub kind: SourceKind,
    pub users: Vec<(String, String)>,
    pub labels: Vec<Label>,
    pub filtered: Vec<Suggestion>,
    pub selected: usize,
    pub visible: bool,
    pub hidden_by_user: bool,
    pub hide_on_empty: bool,
    pub fetch: FetchState,
    pub repo: Option<(String, String)>,
}

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn is_boundary(c: char) -> bool {
    is_whitespace(c) || ",.!?;:()[]{}<>\"'`".contains(c)
}

fn line_chars(input: &str, y: usize) -> Vec<char> {
    input.split('\n').nth(y).unwrap_or("").chars().collect()
}

pub fn word_at(input: &str, (x, y): (usize, usize)) -> CursorToken {
    let runes = line_chars(input, y);
    let x = x.min(runes.len());
    let mut start = 0;
    for i in (0..x).rev() {
        if is_whitespace(runes[i]) {
            start = i + 1;
            break;
        }
        start = i;
    }
    let mut end = runes.len();
    for (i, c) in runes.iter().enumerate().skip(x) {
        if is_whitespace(*c) {
            end = i;
            break;
        }
    }
    CursorToken {
        start: (start, y),
        end: (end, y),
        content: runes[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_string(),
    }
}

/// The comma-separated item around the cursor.
fn label_at(input: &str, (x, y): (usize, usize)) -> (CursorToken, bool) {
    let runes = line_chars(input, y);
    let x = x.min(runes.len());
    let start = (0..x)
        .rev()
        .find(|i| runes[*i] == ',')
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = (x..runes.len())
        .find(|i| runes[*i] == ',')
        .unwrap_or(runes.len());
    let content = runes[start..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string();
    (
        CursorToken {
            start: (start, y),
            end: (end, y),
            content,
        },
        start == 0,
    )
}

fn prefix_of(word: &str, names: [&'static str; 2]) -> Option<&'static str> {
    names.into_iter().find(|p| word.starts_with(p))
}

fn fold_eq(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// sahilm/fuzzy FindFromNoSort scoring for one candidate; None when not all
/// pattern characters match in order.
pub fn fuzzy_score(pattern: &str, target: &str) -> Option<i64> {
    const FIRST_CHAR: i64 = 10;
    const AFTER_SEPARATOR: i64 = 20;
    const CAMEL_CASE: i64 = 20;
    const ADJACENT: i64 = 5;
    const LEADING_PENALTY: i64 = -5;
    const MAX_LEADING_PENALTY: i64 = -15;
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = target.chars().collect();
    if p.is_empty() {
        return None;
    }
    let is_sep = |c: char| "/-_ .\\".contains(c);
    let mut total = 0i64;
    let mut matched: Vec<usize> = vec![];
    let mut pi = 0;
    let mut best = -1i64;
    let mut best_idx: i64 = -1;
    let mut adjacent_bonus = 0i64;
    let mut last = '\0';
    let mut last_index = 0usize;
    for j in 0..t.len() {
        if pi >= p.len() {
            break;
        }
        let c = t[j];
        if fold_eq(c, p[pi]) {
            let mut score = 0;
            if j == 0 {
                score += FIRST_CHAR;
            }
            if last.is_lowercase() && c.is_uppercase() {
                score += CAMEL_CASE;
            }
            if j != 0 && is_sep(last) {
                score += AFTER_SEPARATOR;
            }
            if let Some(&lm) = matched.last() {
                let bonus = if lm == last_index {
                    adjacent_bonus * 2 + ADJACENT
                } else {
                    0
                };
                score += bonus;
                adjacent_bonus += bonus;
            }
            if score > best {
                best = score;
                best_idx = j as i64;
            }
        }
        let next_p = p.get(pi + 1).copied().unwrap_or('\0');
        let next_c = t.get(j + 1).copied().unwrap_or('\0');
        if (fold_eq(next_p, next_c) || next_c == '\0') && best_idx > -1 {
            if matched.is_empty() {
                best += (best_idx * LEADING_PENALTY).max(MAX_LEADING_PENALTY);
            }
            total += best;
            matched.push(best_idx as usize);
            best = -1;
            best_idx = -1;
            pi += 1;
        }
        last_index = j;
        last = c;
    }
    if matched.len() != p.len() {
        return None;
    }
    Some(total + matched.len() as i64 - t.len() as i64)
}

impl Completer {
    pub fn new(kind: SourceKind, repo: Option<(String, String)>, hide_on_empty: bool) -> Completer {
        Completer {
            kind,
            users: vec![],
            labels: vec![],
            filtered: vec![],
            selected: 0,
            visible: false,
            hidden_by_user: false,
            hide_on_empty,
            fetch: FetchState::Idle,
            repo,
        }
    }

    pub fn context(&self, input: &str, pos: (usize, usize)) -> CursorToken {
        match self.kind {
            SourceKind::Search => {
                let w = word_at(input, pos);
                if let Some(p) = prefix_of(&w.content, ["author:", "-author:"])
                    .or(prefix_of(&w.content, ["label:", "-label:"]))
                {
                    let plen = p.chars().count();
                    return CursorToken {
                        start: (w.start.0 + plen, w.start.1),
                        end: w.end,
                        content: w.content[p.len()..].to_string(),
                    };
                }
                w
            }
            SourceKind::Labels => label_at(input, pos).0,
            SourceKind::Mention { with_at } => {
                if input.is_empty() {
                    return CursorToken::default();
                }
                let runes = line_chars(input, pos.1);
                let x = pos.0.min(runes.len());
                let mut start = 0;
                for i in (0..x).rev() {
                    if is_boundary(runes[i]) {
                        start = i + 1;
                        break;
                    }
                    start = i;
                }
                if with_at {
                    start += 1;
                }
                if start > runes.len()
                    || (start > 0 && start < runes.len() && with_at && runes[start - 1] != '@')
                {
                    return CursorToken::default();
                }
                let end = (x..runes.len())
                    .find(|i| is_boundary(runes[*i]))
                    .unwrap_or(runes.len());
                let end = end.max(start.min(runes.len()));
                CursorToken {
                    start: (start.min(runes.len()), pos.1),
                    end: (end, pos.1),
                    content: runes[start.min(runes.len())..end].iter().collect(),
                }
            }
        }
    }

    fn suggestions(&self, input: &str, pos: (usize, usize)) -> Vec<Suggestion> {
        let users = || {
            self.users.iter().map(|(l, n)| Suggestion {
                value: l.clone(),
                detail: n.clone(),
            })
        };
        let labels = || {
            self.labels.iter().map(|l| Suggestion {
                value: l.name.clone(),
                detail: l.description.as_deref().unwrap_or("").trim().to_string(),
            })
        };
        match self.kind {
            SourceKind::Search => {
                let w = word_at(input, pos);
                if prefix_of(&w.content, ["author:", "-author:"]).is_some() {
                    let mut v = vec![Suggestion {
                        value: "@me".into(),
                        detail: "Signed-in user".into(),
                    }];
                    v.extend(users().filter(|s| !s.value.is_empty()));
                    v
                } else if prefix_of(&w.content, ["label:", "-label:"]).is_some() {
                    labels().collect()
                } else {
                    vec![]
                }
            }
            SourceKind::Labels => labels().collect(),
            SourceKind::Mention { .. } => users().collect(),
        }
    }

    fn excluded(&self, input: &str, pos: (usize, usize)) -> Vec<String> {
        if input.trim().is_empty() {
            return vec![];
        }
        match self.kind {
            SourceKind::Search => vec![],
            SourceKind::Labels => {
                let current = label_at(input, pos).0.content;
                input
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && *s != current)
                    .collect()
            }
            SourceKind::Mention { .. } => {
                let current = word_at(input, pos).content;
                input
                    .split_whitespace()
                    .map(String::from)
                    .filter(|w| *w != current)
                    .collect()
            }
        }
    }

    pub fn filter(&mut self, input: &str, pos: (usize, usize)) {
        let ctx = self.context(input, pos);
        let ex: Vec<String> = self
            .excluded(input, pos)
            .iter()
            .map(|s| s.trim().to_lowercase())
            .collect();
        let items: Vec<Suggestion> = self
            .suggestions(input, ctx.start)
            .into_iter()
            .filter(|s| !ex.contains(&s.value.trim().to_lowercase()))
            .collect();
        if ctx.content.is_empty() || items.is_empty() {
            self.filtered = items.into_iter().take(MAX_VISIBLE).collect();
        } else {
            let mut scored: Vec<(i64, usize, Suggestion)> = items
                .into_iter()
                .enumerate()
                .filter_map(|(i, s)| fuzzy_score(&ctx.content, &s.value).map(|sc| (sc, i, s)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.filtered = scored
                .into_iter()
                .take(MAX_VISIBLE)
                .map(|(_, _, s)| s)
                .collect();
        }
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    fn show(&mut self) {
        self.selected = 0;
        self.visible = true;
    }

    pub fn show_completions(&mut self, input: &str, pos: (usize, usize)) {
        let ctx = self.context(input, pos);
        if self.hide_on_empty && self.filtered.is_empty() {
            return;
        }
        match self.kind {
            SourceKind::Mention { with_at: true } => {
                let line = line_chars(input, ctx.start.1);
                if ctx.start.0 >= 1 && line.get(ctx.start.0 - 1) == Some(&'@') {
                    self.show();
                }
            }
            _ => self.show(),
        }
    }

    pub fn selected_value(&self) -> Option<String> {
        if !self.visible {
            return None;
        }
        self.filtered.get(self.selected).map(|s| s.value.clone())
    }

    fn insert(&self, input: &str, value: &str, ctx: &CursorToken) -> (String, (usize, usize)) {
        let lines: Vec<&str> = input.split('\n').collect();
        let y = ctx.start.1;
        let runes: Vec<char> = lines.get(y).unwrap_or(&"").chars().collect();
        let join = |ls: &[&str]| ls.join("\n");
        let (new_line, cursor) = match self.kind {
            SourceKind::Labels => {
                let (info, first) = label_at(input, ctx.end);
                let replacement = if first {
                    format!("{value}, ")
                } else {
                    format!(" {value}, ")
                };
                let rest: String = runes[info.end.0.min(runes.len())..].iter().collect();
                let rest = rest
                    .strip_prefix(',')
                    .unwrap_or(&rest)
                    .trim_start_matches([' ', '\t'])
                    .to_string();
                let before: String = runes[..info.start.0.min(runes.len())].iter().collect();
                (
                    format!("{before}{replacement}{rest}"),
                    info.start.0 + replacement.chars().count(),
                )
            }
            _ => {
                let replacement = format!("{value} ");
                let before: String = runes[..ctx.start.0.min(runes.len())].iter().collect();
                let after: String = runes[ctx.end.0.min(runes.len())..].iter().collect();
                (
                    format!("{before}{replacement}{after}"),
                    ctx.start.0 + replacement.chars().count(),
                )
            }
        };
        let mut parts: Vec<String> = vec![];
        if y > 0 {
            parts.push(join(&lines[..y]));
        }
        parts.push(new_line);
        if y + 1 < lines.len() {
            parts.push(join(&lines[y + 1..]));
        }
        (parts.join("\n"), (cursor, y))
    }

    /// Returns true when the key was consumed.
    pub fn handle_key<E: Editable>(&mut self, key: &str, input: &mut E) -> bool {
        if key == "ctrl+h" {
            if self.visible {
                self.hidden_by_user = true;
                self.visible = false;
            } else {
                self.hidden_by_user = false;
                self.filter(&input.text(), input.cursor_pos());
                self.show();
            }
            return true;
        }
        if self.visible || !self.filtered.is_empty() {
            match key {
                "up" | "ctrl+p" => {
                    if !self.filtered.is_empty() {
                        self.selected =
                            (self.selected + self.filtered.len() - 1) % self.filtered.len();
                    }
                    return true;
                }
                "down" | "ctrl+n" => {
                    if !self.filtered.is_empty() {
                        self.selected = (self.selected + 1) % self.filtered.len();
                    }
                    return true;
                }
                "ctrl+y" => {
                    if let Some(v) = self.selected_value() {
                        let text = input.text();
                        let ctx = self.context(&text, input.cursor_pos());
                        let (nv, pos) = self.insert(&text, &v, &ctx);
                        input.set_text(&nv, pos);
                        if !matches!(self.kind, SourceKind::Search) {
                            self.filter(&input.text(), input.cursor_pos());
                            self.show();
                        }
                        return true;
                    }
                }
                _ => {}
            }
        }
        let before = self.context(&input.text(), input.cursor_pos());
        let consumed = input.key(key);
        let text = input.text();
        let pos = input.cursor_pos();
        self.filter(&text, pos);
        let after = self.context(&text, pos);
        if after != before {
            if self.hide_on_empty && after == CursorToken::default() {
                self.visible = false;
            } else if !self.hidden_by_user {
                self.show_completions(&text, pos);
            }
        }
        consumed
    }

    /// Rows of the popup including its borders, zero while hidden.
    pub fn height(&self) -> u16 {
        if !self.visible {
            return 0;
        }
        let rows = self.filtered.len().min(MAX_VISIBLE);
        let no_results = rows == 0 && matches!(self.fetch, FetchState::Idle | FetchState::Success);
        let status = matches!(self.fetch, FetchState::Loading | FetchState::Error(_));
        (rows + usize::from(no_results) + usize::from(status) + 4) as u16
    }

    pub fn record_hits(&self, area: Rect, hits: &mut HitMap) {
        if !self.visible {
            return;
        }
        let area = Rect {
            height: area.height.min(self.height()),
            ..area
        };
        let inner = Block::bordered().inner(area);
        let rows = self.filtered.len().min(MAX_VISIBLE);
        for i in 0..rows.min(inner.height as usize) {
            let row = Rect {
                y: inner.y + i as u16,
                height: 1,
                ..inner
            };
            hits.push(row, Target::Suggestion(i));
        }
    }

    pub fn select(&mut self, i: usize) {
        if i < self.filtered.len() {
            self.selected = i;
        }
    }

    /// Draws the rows, status and help in a rounded box whose top border joins any
    /// border already drawn on its first row.
    pub fn render(&self, t: &Theme, spinner: usize, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }
        let area = Rect {
            height: area.height.min(self.height()),
            ..area
        };
        let [_, below_top] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        Clear.render(below_top, buf);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::OPEN_ISSUE))
            .merge_borders(MergeStrategy::Fuzzy);
        let inner = block.inner(area);
        block.render(area, buf);
        Text::from(self.lines(t, spinner, inner.width as usize)).render(inner, buf);
    }

    fn lines(&self, t: &Theme, spinner: usize, width: usize) -> Vec<Line<'static>> {
        let sel_bg = Style::default().bg(t.selected_background);
        let mut lines = vec![];
        for (i, s) in self.filtered.iter().take(MAX_VISIBLE).enumerate() {
            let selected = i == self.selected;
            let bg = if selected { sel_bg } else { Style::default() };
            let marker = if selected {
                span(
                    format!("{} ", icons::SELECTION),
                    Style::default().fg(t.success_text),
                )
            } else {
                raw("  ")
            };
            lines.push(
                Line::from(vec![
                    marker,
                    span(s.value.clone(), t.primary().add_modifier(Modifier::BOLD)),
                    raw(" "),
                    span(s.detail.clone(), t.faint()),
                ])
                .style(bg),
            );
        }
        if lines.is_empty() && matches!(self.fetch, FetchState::Idle | FetchState::Success) {
            lines.push(Line::from(span(
                format!(" no results{}", icons::ELLIPSIS),
                t.faint().add_modifier(Modifier::ITALIC),
            )));
        }
        match &self.fetch {
            FetchState::Loading => lines.push(Line::from(vec![
                raw(" "),
                span(SPINNER[spinner % SPINNER.len()], t.secondary()),
                span(
                    format!("fetching suggestions{}", icons::ELLIPSIS),
                    t.faint().add_modifier(Modifier::ITALIC),
                ),
            ])),
            FetchState::Error(e) => lines.push(Line::from(vec![
                raw(" "),
                span(icons::FAILURE, Style::default().fg(t.error_text)),
                span(format!(" {e}"), t.faint().add_modifier(Modifier::ITALIC)),
            ])),
            _ => {}
        }
        lines.push(Line::from(span(
            "─".repeat(width),
            Style::default().fg(t.faint_border),
        )));
        let sep = || span(" • ", Style::default().fg(t.secondary_border));
        let mut help = vec![raw(" ")];
        for (i, (k, d)) in [
            ("↓/ctrl+n", "next"),
            ("↑/ctrl+p", "previous"),
            ("ctrl+y", "select"),
            ("ctrl+f", "refresh"),
            ("ctrl+h", "toggle"),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                help.push(sep());
            }
            help.push(span(k, t.primary()));
            help.push(span(format!(" {d}"), t.faint()));
        }
        lines.push(truncate_line(&Line::from(help), width, icons::ELLIPSIS));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{TextArea, TextInput};

    #[test]
    fn search_context_strips_prefixes() {
        let c = Completer::new(SourceKind::Search, None, false);
        let ctx = c.context("is:open author:jua", (18, 0));
        assert_eq!(ctx.content, "jua");
        assert_eq!(ctx.start, (15, 0));
    }

    #[test]
    fn selecting_a_label_rewrites_the_list() {
        let mut c = Completer::new(SourceKind::Labels, None, false);
        c.labels = vec![
            Label {
                name: "bug".into(),
                ..Default::default()
            },
            Label {
                name: "docs".into(),
                ..Default::default()
            },
        ];
        let mut input = TextArea::new("bug, do");
        c.handle_key("c", &mut input);
        c.visible = true;
        assert_eq!(c.filtered.first().map(|s| s.value.as_str()), Some("docs"));
        c.handle_key("ctrl+y", &mut input);
        assert_eq!(input.value(), "bug, docs, ");
    }

    #[test]
    fn mention_requires_at_sign() {
        let mut c = Completer::new(SourceKind::Mention { with_at: true }, None, true);
        c.users = vec![("juanibiapina".into(), "Juan".into())];
        let mut input = TextArea::new("hi ");
        c.handle_key("@", &mut input);
        c.handle_key("j", &mut input);
        assert!(c.visible);
        c.handle_key("ctrl+y", &mut input);
        assert_eq!(input.value(), "hi @juanibiapina ");

        let mut plain = TextInput::new("");
        let mut s = Completer::new(SourceKind::Search, None, false);
        s.handle_key("x", &mut plain);
        assert!(s.filtered.is_empty());
    }

    #[test]
    fn fuzzy_matches_sahilm_scores() {
        // first char +10, then adjacent +5 and +15, minus 1 unmatched char
        assert_eq!(fuzzy_score("doc", "docs"), Some(29));
        assert!(fuzzy_score("doc", "docs").unwrap() > fuzzy_score("doc", "undocumented").unwrap());
        assert!(fuzzy_score("xyz", "docs").is_none());
    }
}
