//! Code block highlighting: syntect with bat's syntaxes (from two-face), colored
//! by the markdown theme's code palette.

use std::str::FromStr;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    self, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::text::Block;

/// Blocks larger than this render as plain text, to bound the parsing work.
const MAX_HIGHLIGHTED_BYTES: usize = 64 * 1024;

const TEXT: u32 = 0xC4C4C4;

/// Scope selectors and their colors: (selectors, foreground, background, font style).
const PALETTE: &[(&str, Option<u32>, Option<u32>, FontStyle)] = &[
    ("comment", Some(0x676767), None, FontStyle::empty()),
    (
        "meta.preprocessor",
        Some(0xFF875F),
        None,
        FontStyle::empty(),
    ),
    ("keyword, storage", Some(0x00AAFF), None, FontStyle::empty()),
    (
        "keyword.control.import, keyword.other.import, keyword.other.package, keyword.control.package",
        Some(0xFF5F87),
        None,
        FontStyle::empty(),
    ),
    (
        "storage.type, support.type",
        Some(0x6E6ED8),
        None,
        FontStyle::empty(),
    ),
    (
        "storage.type.function, storage.type.keyword, storage.type.struct, storage.type.enum, \
         storage.type.impl, storage.type.trait, storage.type.class, storage.type.module, \
         keyword.declaration",
        Some(0x00AAFF),
        None,
        FontStyle::empty(),
    ),
    ("keyword.operator", Some(0xEF8080), None, FontStyle::empty()),
    ("punctuation", Some(0xE8E8A8), None, FontStyle::empty()),
    ("support.function", Some(0xFF8EC7), None, FontStyle::empty()),
    (
        "entity.name.function, variable.function",
        Some(0x00D787),
        None,
        FontStyle::empty(),
    ),
    (
        "entity.name.class, entity.name.struct, entity.name.enum, entity.name.type",
        Some(0xF1F1F1),
        None,
        FontStyle::BOLD.union(FontStyle::UNDERLINE),
    ),
    (
        "entity.other.attribute-name",
        Some(0x7A7AE6),
        None,
        FontStyle::empty(),
    ),
    (
        "meta.annotation, meta.decorator",
        Some(0xFFFF87),
        None,
        FontStyle::empty(),
    ),
    ("entity.name.tag", Some(0xB083EA), None, FontStyle::empty()),
    ("string", Some(0xC69669), None, FontStyle::empty()),
    (
        "constant.character.escape",
        Some(0xAFFFD7),
        None,
        FontStyle::empty(),
    ),
    ("constant.numeric", Some(0x6EEFC0), None, FontStyle::empty()),
    ("markup.deleted", Some(0xFD5B5B), None, FontStyle::empty()),
    ("markup.inserted", Some(0x00D787), None, FontStyle::empty()),
    ("markup.italic", None, None, FontStyle::ITALIC),
    ("markup.bold", None, None, FontStyle::BOLD),
    ("meta.diff.range", Some(0x777777), None, FontStyle::empty()),
    (
        "invalid",
        Some(0xF1F1F1),
        Some(0xF05B5B),
        FontStyle::empty(),
    ),
];

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn rgb(hex: u32) -> highlighting::Color {
    highlighting::Color {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
        a: 0xFF,
    }
}

/// The background of unstyled text: transparent, so the terminal's shows.
const NO_BACKGROUND: highlighting::Color = highlighting::Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| Theme {
        settings: ThemeSettings {
            foreground: Some(rgb(TEXT)),
            background: Some(NO_BACKGROUND),
            ..Default::default()
        },
        scopes: PALETTE
            .iter()
            .map(|(selectors, fg, bg, font)| ThemeItem {
                scope: ScopeSelectors::from_str(selectors).expect("valid scope selectors"),
                style: StyleModifier {
                    foreground: fg.map(rgb),
                    background: bg.map(rgb),
                    font_style: (!font.is_empty()).then_some(*font),
                },
            })
            .collect(),
        ..Default::default()
    })
}

fn syntax_for(code: &str, lang: &str) -> &'static SyntaxReference {
    let set = syntaxes();
    if code.len() > MAX_HIGHLIGHTED_BYTES {
        return set.find_syntax_plain_text();
    }
    set.find_syntax_by_token(lang)
        .or_else(|| set.find_syntax_by_first_line(code))
        .unwrap_or_else(|| set.find_syntax_plain_text())
}

fn to_style(s: highlighting::Style) -> Style {
    let mut style = Style::default().fg(Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b));
    if s.background.a != 0 {
        style = style.bg(Color::Rgb(s.background.r, s.background.g, s.background.b));
    }
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

/// Highlights a code block (`code` as written, `lang` from the fence) into lines.
pub fn highlight(code: &str, lang: &str) -> Block {
    let mut highlighter = HighlightLines::new(syntax_for(code, lang), theme());
    let plain = highlighting::Style {
        foreground: rgb(TEXT),
        background: NO_BACKGROUND,
        font_style: FontStyle::empty(),
    };
    LinesWithEndings::from(code)
        .map(|line| {
            let regions = highlighter
                .highlight_line(line, syntaxes())
                .unwrap_or_else(|_| vec![(plain, line)]);
            let spans: Vec<Span<'static>> = regions
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches(['\n', '\r']).replace('\t', "    ");
                    (!text.is_empty()).then(|| Span::styled(text, to_style(style)))
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_of(block: &Block, line: usize, text: &str) -> Option<Color> {
        block[line]
            .spans
            .iter()
            .find(|s| s.content.contains(text))
            .and_then(|s| s.style.fg)
    }

    #[test]
    fn keywords_strings_and_comments_get_palette_colors() {
        let b = highlight("fn main() {\n    let s = \"hi\"; // note\n}\n", "rust");
        assert_eq!(color_of(&b, 0, "fn"), Some(Color::Rgb(0x00, 0xAA, 0xFF)));
        assert_eq!(color_of(&b, 1, "hi"), Some(Color::Rgb(0xC6, 0x96, 0x69)));
        assert_eq!(color_of(&b, 1, "note"), Some(Color::Rgb(0x67, 0x67, 0x67)));
    }

    #[test]
    fn an_unknown_language_renders_plain() {
        let b = highlight("let x = 1;\n", "no-such-language");
        assert_eq!(b.len(), 1);
        assert!(
            b[0].spans
                .iter()
                .all(|s| s.style.fg == Some(Color::Rgb(0xC4, 0xC4, 0xC4)))
        );
    }
}
