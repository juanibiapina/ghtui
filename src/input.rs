//! Text input widgets: a single-line input (search bar, confirmation prompt)
//! and a multi-line editor (comment / approve / assign / label box).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::text::{Block, str_width};

#[derive(Clone, Debug, Default)]
pub struct TextInput {
    chars: Vec<char>,
    cursor: usize,
}

/// Shared editing keys. Returns true when the key changed or moved the input.
fn edit(chars: &mut Vec<char>, cursor: &mut usize, key: &str, multiline: bool) -> bool {
    match key {
        "left" | "ctrl+b" => *cursor = cursor.saturating_sub(1),
        "right" | "ctrl+f" => *cursor = (*cursor + 1).min(chars.len()),
        "home" | "ctrl+a" => {
            *cursor = if multiline {
                line_start(chars, *cursor)
            } else {
                0
            };
        }
        "end" | "ctrl+e" => {
            *cursor = if multiline {
                line_end(chars, *cursor)
            } else {
                chars.len()
            };
        }
        "backspace" | "ctrl+h" => {
            if *cursor > 0 {
                chars.remove(*cursor - 1);
                *cursor -= 1;
            }
        }
        "delete" => {
            if *cursor < chars.len() {
                chars.remove(*cursor);
            }
        }
        "ctrl+u" => {
            let start = if multiline {
                line_start(chars, *cursor)
            } else {
                0
            };
            chars.drain(start..*cursor);
            *cursor = start;
        }
        "ctrl+k" => {
            let end = if multiline {
                line_end(chars, *cursor)
            } else {
                chars.len()
            };
            chars.drain(*cursor..end);
        }
        "ctrl+w" | "alt+backspace" => {
            let mut i = *cursor;
            while i > 0 && chars[i - 1] == ' ' {
                i -= 1;
            }
            while i > 0 && chars[i - 1] != ' ' && chars[i - 1] != '\n' {
                i -= 1;
            }
            chars.drain(i..*cursor);
            *cursor = i;
        }
        "space" => {
            chars.insert(*cursor, ' ');
            *cursor += 1;
        }
        k if k.chars().count() == 1 => {
            chars.insert(*cursor, k.chars().next().unwrap());
            *cursor += 1;
        }
        _ => return false,
    }
    true
}

fn line_start(chars: &[char], cursor: usize) -> usize {
    let mut i = cursor;
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

fn line_end(chars: &[char], cursor: usize) -> usize {
    let mut i = cursor;
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

impl TextInput {
    pub fn new(value: &str) -> Self {
        let chars: Vec<char> = value.chars().collect();
        let cursor = chars.len();
        TextInput { chars, cursor }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn set_value(&mut self, v: &str) {
        self.chars = v.chars().collect();
        self.cursor = self.chars.len();
    }

    pub fn handle_key(&mut self, key: &str) -> bool {
        edit(&mut self.chars, &mut self.cursor, key, false)
    }
}

/// Value + cursor access for autocomplete, positions are (column, line) in chars.
pub trait Editable {
    fn text(&self) -> String;
    fn cursor_pos(&self) -> (usize, usize);
    fn set_text(&mut self, v: &str, pos: (usize, usize));
    fn key(&mut self, key: &str) -> bool;
}

impl Editable for TextInput {
    fn text(&self) -> String {
        self.value()
    }
    fn cursor_pos(&self) -> (usize, usize) {
        (self.cursor, 0)
    }
    fn set_text(&mut self, v: &str, pos: (usize, usize)) {
        self.chars = v.chars().collect();
        self.cursor = pos.0.min(self.chars.len());
    }
    fn key(&mut self, key: &str) -> bool {
        self.handle_key(key)
    }
}

impl Editable for TextArea {
    fn text(&self) -> String {
        self.value()
    }
    fn cursor_pos(&self) -> (usize, usize) {
        let start = line_start(&self.chars, self.cursor);
        let y = self.chars[..start].iter().filter(|c| **c == '\n').count();
        (self.cursor - start, y)
    }
    fn set_text(&mut self, v: &str, pos: (usize, usize)) {
        self.chars = v.chars().collect();
        let mut idx = 0;
        for _ in 0..pos.1 {
            match self.chars[idx..].iter().position(|c| *c == '\n') {
                Some(p) => idx += p + 1,
                None => break,
            }
        }
        self.cursor = (idx + pos.0).min(line_end(&self.chars, idx));
    }
    fn key(&mut self, key: &str) -> bool {
        self.handle_key(key)
    }
}

impl TextInput {
    /// Renders `value` with a block cursor, scrolled so the cursor stays within `width`.
    pub fn view(&self, width: usize, text: Style, focused: bool) -> Line<'static> {
        let mut start = 0;
        let cursor_w = |from: usize| -> usize {
            self.chars[from..self.cursor]
                .iter()
                .map(|c| str_width(&c.to_string()))
                .sum::<usize>()
        };
        while start < self.cursor && cursor_w(start) + 1 > width {
            start += 1;
        }
        let mut spans = vec![];
        let before: String = self.chars[start..self.cursor].iter().collect();
        spans.push(Span::styled(before, text));
        if focused {
            let at = self.chars.get(self.cursor).copied().unwrap_or(' ');
            spans.push(Span::styled(
                at.to_string(),
                text.add_modifier(Modifier::REVERSED),
            ));
            if self.cursor < self.chars.len() {
                let after: String = self.chars[self.cursor + 1..].iter().collect();
                spans.push(Span::styled(after, text));
            }
        } else if self.cursor < self.chars.len() {
            let after: String = self.chars[self.cursor..].iter().collect();
            spans.push(Span::styled(after, text));
        }
        Line::from(spans)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextArea {
    chars: Vec<char>,
    cursor: usize,
}

impl TextArea {
    pub fn new(value: &str) -> Self {
        let chars: Vec<char> = value.chars().collect();
        let cursor = chars.len();
        TextArea { chars, cursor }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn handle_key(&mut self, key: &str) -> bool {
        match key {
            "enter" => {
                self.chars.insert(self.cursor, '\n');
                self.cursor += 1;
                true
            }
            "up" => {
                let start = line_start(&self.chars, self.cursor);
                if start == 0 {
                    return true;
                }
                let col = self.cursor - start;
                let prev_start = line_start(&self.chars, start - 1);
                self.cursor = (prev_start + col).min(start - 1);
                true
            }
            "down" => {
                let end = line_end(&self.chars, self.cursor);
                if end >= self.chars.len() {
                    return true;
                }
                let col = self.cursor - line_start(&self.chars, self.cursor);
                let next_start = end + 1;
                self.cursor = (next_start + col).min(line_end(&self.chars, next_start));
                true
            }
            _ => edit(&mut self.chars, &mut self.cursor, key, true),
        }
    }

    /// Renders the text hard-wrapped to `width` with a block cursor, numbering
    /// logical lines ("  1 hello").
    pub fn view(&self, width: usize, height: usize, text: Style, number_style: Style) -> Block {
        let width = width.saturating_sub(4).max(1);
        let mut lines: Vec<Vec<Span<'static>>> = vec![vec![]];
        let mut numbers: Vec<Option<usize>> = vec![Some(1)];
        let mut logical = 1;
        let mut w = 0;
        let mut cursor_line = 0;
        for i in 0..=self.chars.len() {
            let is_cursor = i == self.cursor;
            let ch = self.chars.get(i).copied();
            if is_cursor {
                cursor_line = lines.len() - 1;
            }
            match ch {
                Some('\n') => {
                    if is_cursor {
                        lines
                            .last_mut()
                            .unwrap()
                            .push(Span::styled(" ", text.add_modifier(Modifier::REVERSED)));
                    }
                    lines.push(vec![]);
                    logical += 1;
                    numbers.push(Some(logical));
                    w = 0;
                }
                Some(c) => {
                    let cw = str_width(&c.to_string()).max(1);
                    if w + cw > width {
                        lines.push(vec![]);
                        numbers.push(None);
                        w = 0;
                        if is_cursor {
                            cursor_line = lines.len() - 1;
                        }
                    }
                    let st = if is_cursor {
                        text.add_modifier(Modifier::REVERSED)
                    } else {
                        text
                    };
                    lines
                        .last_mut()
                        .unwrap()
                        .push(Span::styled(c.to_string(), st));
                    w += cw;
                }
                None => {
                    if is_cursor {
                        lines
                            .last_mut()
                            .unwrap()
                            .push(Span::styled(" ", text.add_modifier(Modifier::REVERSED)));
                    }
                }
            }
        }
        let start = if cursor_line >= height {
            cursor_line + 1 - height
        } else {
            0
        };
        let mut out: Block = lines
            .into_iter()
            .zip(numbers)
            .skip(start)
            .take(height)
            .map(|(spans, n)| {
                let prefix = match n {
                    Some(n) => Span::styled(format!("{n:>3} "), number_style),
                    None => Span::raw("    "),
                };
                let mut v = vec![prefix];
                v.extend(spans);
                Line::from(v)
            })
            .collect();
        while out.len() < height {
            out.push(Line::default());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_editing() {
        let mut t = TextInput::new("is:open");
        t.handle_key("space");
        t.handle_key("a");
        assert_eq!(t.value(), "is:open a");
        t.handle_key("ctrl+w");
        assert_eq!(t.value(), "is:open ");
        t.handle_key("ctrl+u");
        assert_eq!(t.value(), "");
    }

    #[test]
    fn textarea_newlines() {
        let mut t = TextArea::new("a");
        t.handle_key("enter");
        t.handle_key("b");
        assert_eq!(t.value(), "a\nb");
        t.handle_key("up");
        t.handle_key("x");
        assert_eq!(t.value(), "ax\nb");
    }
}
