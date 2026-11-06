//! Styled-text helpers that stand in for the lipgloss operations the Go code uses
//! (Width, JoinHorizontal, JoinVertical, Truncate, PlaceHorizontal, word wrap).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub type Block = Vec<Line<'static>>;

pub fn span(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

pub fn raw(text: impl Into<String>) -> Span<'static> {
    Span::raw(text.into())
}

pub fn str_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn line_width(l: &Line) -> usize {
    l.spans.iter().map(|s| str_width(&s.content)).sum()
}

pub fn block_width(b: &Block) -> usize {
    b.iter().map(line_width).max().unwrap_or(0)
}

/// Cuts a line to `max` cells, appending `ellipsis` when something was cut.
pub fn truncate_line(l: &Line<'static>, max: usize, ellipsis: &str) -> Line<'static> {
    if line_width(l) <= max {
        return l.clone();
    }
    let ell_w = str_width(ellipsis);
    let budget = max.saturating_sub(ell_w);
    let mut out: Vec<Span<'static>> = vec![];
    let mut used = 0;
    let mut last_style = Style::default();
    for s in &l.spans {
        last_style = s.style;
        let mut buf = String::new();
        for ch in s.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > budget {
                break;
            }
            used += w;
            buf.push(ch);
        }
        let done = buf.chars().count() < s.content.chars().count();
        if !buf.is_empty() {
            out.push(Span::styled(buf, s.style));
        }
        if done {
            break;
        }
    }
    if ell_w > 0 && max >= ell_w {
        out.push(Span::styled(ellipsis.to_string(), last_style));
    }
    Line::from(out)
}

pub fn truncate_str(s: &str, max: usize, ellipsis: &str) -> String {
    let l = truncate_line(&Line::from(s.to_string()), max, ellipsis);
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn pad_line(mut l: Line<'static>, w: usize, style: Style) -> Line<'static> {
    let cur = line_width(&l);
    if cur < w {
        l.spans.push(Span::styled(" ".repeat(w - cur), style));
    }
    l
}

/// Truncates (without ellipsis) and pads to exactly `w` cells.
pub fn fit_line(l: &Line<'static>, w: usize, pad_style: Style) -> Line<'static> {
    pad_line(truncate_line(l, w, ""), w, pad_style)
}

pub fn align_line(l: Line<'static>, w: usize, align: Align, pad_style: Style) -> Line<'static> {
    let cur = line_width(&l);
    if cur >= w {
        return truncate_line(&l, w, "");
    }
    let gap = w - cur;
    let (left, right) = match align {
        Align::Left => (0, gap),
        Align::Right => (gap, 0),
        Align::Center => (gap / 2, gap - gap / 2),
    };
    let mut spans = vec![];
    if left > 0 {
        spans.push(Span::styled(" ".repeat(left), pad_style));
    }
    spans.extend(l.spans);
    if right > 0 {
        spans.push(Span::styled(" ".repeat(right), pad_style));
    }
    Line::from(spans)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// lipgloss.JoinHorizontal(Top, ...): each block is padded to its own width.
pub fn hjoin(blocks: Vec<Block>) -> Block {
    let height = blocks.iter().map(|b| b.len()).max().unwrap_or(0);
    let widths: Vec<usize> = blocks.iter().map(block_width).collect();
    let mut out: Block = vec![Line::default(); height];
    for (b, w) in blocks.into_iter().zip(widths) {
        for (i, row) in out.iter_mut().enumerate() {
            let l = b.get(i).cloned().unwrap_or_default();
            let padded = pad_line(l, w, Style::default());
            row.spans.extend(padded.spans);
        }
    }
    out
}

/// Concatenates spans of single-line pieces.
pub fn concat(lines: Vec<Line<'static>>) -> Line<'static> {
    let mut spans = vec![];
    for l in lines {
        spans.extend(l.spans);
    }
    Line::from(spans)
}

/// Port of charmbracelet/x/ansi `wrap` (what lipgloss.Wrap and Style.Width use):
/// breaks on spaces and hyphens, hard-wraps words longer than the limit, and
/// keeps a line's trailing spaces unless they overflow.
pub fn ansi_wrap(cells: &[(char, Style)], limit: usize) -> Vec<Vec<(char, Style)>> {
    ansi_wrap_with(cells, limit, "")
}

/// `ansi_wrap` with extra ASCII breakpoint characters (glamour uses " ,.;-+|").
pub fn ansi_wrap_with(cells: &[(char, Style)], limit: usize, breakpoints: &str) -> Vec<Vec<(char, Style)>> {
    if limit < 1 {
        return vec![cells.to_vec()];
    }
    let cw = |c: char| UnicodeWidthChar::width(c).unwrap_or(0);
    let mut lines: Vec<Vec<(char, Style)>> = vec![vec![]];
    let mut word: Vec<(char, Style)> = vec![];
    let mut space: Vec<(char, Style)> = vec![];
    let mut space_w = 0usize;
    let mut cur_w = 0usize;
    let mut word_len = 0usize;

    macro_rules! add_space {
        () => {{
            if space_w != 0 || !space.is_empty() {
                cur_w += space_w;
                lines.last_mut().unwrap().append(&mut space);
                space_w = 0;
            }
        }};
    }
    macro_rules! add_word {
        () => {{
            if !word.is_empty() {
                add_space!();
                cur_w += word_len;
                lines.last_mut().unwrap().append(&mut word);
                word_len = 0;
            }
        }};
    }
    macro_rules! add_newline {
        () => {{
            lines.push(vec![]);
            cur_w = 0;
            space.clear();
            space_w = 0;
        }};
    }

    for &(c, st) in cells {
        if !c.is_ascii() {
            let w = cw(c);
            if c.is_whitespace() && c != '\u{a0}' {
                add_word!();
                space.push((c, st));
                space_w += w;
            } else {
                if word_len + w > limit {
                    add_word!();
                }
                word.push((c, st));
                word_len += w;
                if cur_w + word_len + space_w > limit {
                    add_newline!();
                }
                if word_len == limit {
                    add_word!();
                }
            }
            continue;
        }
        match c {
            '\n' => {
                if word_len == 0 {
                    if cur_w + space_w <= limit {
                        lines.last_mut().unwrap().append(&mut space);
                    }
                    space.clear();
                    space_w = 0;
                }
                add_word!();
                add_newline!();
            }
            c if c.is_ascii_whitespace() => {
                add_word!();
                space.push((c, st));
                space_w += 1;
            }
            c if c == '-' || breakpoints.contains(c) => {
                add_space!();
                if cur_w + word_len >= limit {
                    word.push((c, st));
                    word_len += 1;
                } else {
                    add_word!();
                    lines.last_mut().unwrap().push((c, st));
                    cur_w += 1;
                }
            }
            c if c.is_ascii_control() => {}
            _ => {
                if cur_w == limit {
                    add_newline!();
                }
                word.push((c, st));
                word_len += 1;
                if word_len == limit {
                    add_word!();
                }
                if cur_w + word_len + space_w > limit {
                    add_newline!();
                }
            }
        }
    }
    if word_len == 0 {
        if cur_w + space_w <= limit {
            lines.last_mut().unwrap().append(&mut space);
        }
        space.clear();
    }
    add_word!();
    let _ = (space_w, cur_w);
    lines
}

pub fn spans_to_cells(spans: &[Span<'static>]) -> Vec<(char, Style)> {
    spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style))).collect()
}

pub fn cells_to_line(cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![];
    let mut cur = String::new();
    let mut cur_style: Option<Style> = None;
    for &(c, st) in cells {
        if cur_style != Some(st) && !cur.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut cur), cur_style.unwrap()));
        }
        cur_style = Some(st);
        cur.push(c);
    }
    if !cur.is_empty() {
        spans.push(Span::styled(cur, cur_style.unwrap_or_default()));
    }
    Line::from(spans)
}

/// Word-wraps styled spans to `width` with `ansi_wrap`.
pub fn wrap_spans(spans: &[Span<'static>], width: usize) -> Block {
    ansi_wrap(&spans_to_cells(spans), width.max(1)).iter().map(|l| cells_to_line(l)).collect()
}

pub fn wrap_str(text: &str, width: usize, style: Style) -> Block {
    wrap_spans(&[Span::styled(text.to_string(), style)], width)
}

/// Prefixes every line of a block (lipgloss Padding-left / margin).
pub fn indent(block: Block, n: usize) -> Block {
    let pad = " ".repeat(n);
    block
        .into_iter()
        .map(|mut l| {
            l.spans.insert(0, Span::raw(pad.clone()));
            l
        })
        .collect()
}

/// Draws a block into the buffer, clipping to the area.
pub fn draw_block(buf: &mut Buffer, area: Rect, block: &[Line<'static>]) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    for (i, l) in block.iter().enumerate() {
        if i as u16 >= area.height {
            break;
        }
        buf.set_line(area.x, area.y + i as u16, l, area.width);
    }
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
    fn wraps_on_words() {
        let b = wrap_str("hello brave new world", 11, Style::default());
        assert_eq!(plain(&b), vec!["hello brave", "new world"]);
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate_str("abcdef", 4, "…"), "abc…");
        assert_eq!(truncate_str("abc", 4, "…"), "abc");
    }

    #[test]
    fn hjoin_pads_columns() {
        let a: Block = vec![Line::from("ab"), Line::from("a")];
        let b: Block = vec![Line::from("x")];
        assert_eq!(plain(&hjoin(vec![a, b])), vec!["abx", "a  "]);
    }
}
