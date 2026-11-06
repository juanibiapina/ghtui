//! Code block highlighting the way glamour does it: Chroma tokens (see
//! chroma.rs), colored by the gh-dash markdown theme's Chroma entries, reduced
//! to the xterm 256 palette by Chroma's terminal256 formatter.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::chroma::{self, TT, category, sub_category};
use crate::text::Block;

/// Chroma's terminal256 foreground table (formatters/tty_indexed.go); repeated colours keep the later index.
const CHROMA_256: [(u32, u8); 247] = [
    (0x800000, 1),
    (0x008000, 2),
    (0x808000, 3),
    (0x000080, 4),
    (0x800080, 5),
    (0x008080, 6),
    (0xc0c0c0, 7),
    (0x000000, 16),
    (0x00005f, 17),
    (0x000087, 18),
    (0x0000af, 19),
    (0x0000d7, 20),
    (0x0000ff, 21),
    (0x005f00, 22),
    (0x005f5f, 23),
    (0x005f87, 24),
    (0x005faf, 25),
    (0x005fd7, 26),
    (0x005fff, 27),
    (0x008700, 28),
    (0x00875f, 29),
    (0x008787, 30),
    (0x0087af, 31),
    (0x0087d7, 32),
    (0x0087ff, 33),
    (0x00af00, 34),
    (0x00af5f, 35),
    (0x00af87, 36),
    (0x00afaf, 37),
    (0x00afd7, 38),
    (0x00afff, 39),
    (0x00d700, 40),
    (0x00d75f, 41),
    (0x00d787, 42),
    (0x00d7af, 43),
    (0x00d7d7, 44),
    (0x00d7ff, 45),
    (0x00ff00, 46),
    (0x00ff5f, 47),
    (0x00ff87, 48),
    (0x00ffaf, 49),
    (0x00ffd7, 50),
    (0x00ffff, 51),
    (0x5f0000, 52),
    (0x5f005f, 53),
    (0x5f0087, 54),
    (0x5f00af, 55),
    (0x5f00d7, 56),
    (0x5f00ff, 57),
    (0x5f5f00, 58),
    (0x5f5f5f, 59),
    (0x5f5f87, 60),
    (0x5f5faf, 61),
    (0x5f5fd7, 62),
    (0x5f5fff, 63),
    (0x5f8700, 64),
    (0x5f875f, 65),
    (0x5f8787, 66),
    (0x5f87af, 67),
    (0x5f87d7, 68),
    (0x5f87ff, 69),
    (0x5faf00, 70),
    (0x5faf5f, 71),
    (0x5faf87, 72),
    (0x5fafaf, 73),
    (0x5fafd7, 74),
    (0x5fafff, 75),
    (0x5fd700, 76),
    (0x5fd75f, 77),
    (0x5fd787, 78),
    (0x5fd7af, 79),
    (0x5fd7d7, 80),
    (0x5fd7ff, 81),
    (0x5fff00, 82),
    (0x5fff5f, 83),
    (0x5fff87, 84),
    (0x5fffaf, 85),
    (0x5fffd7, 86),
    (0x5fffff, 87),
    (0x870000, 88),
    (0x87005f, 89),
    (0x870087, 90),
    (0x8700af, 91),
    (0x8700d7, 92),
    (0x8700ff, 93),
    (0x875f00, 94),
    (0x875f5f, 95),
    (0x875f87, 96),
    (0x875faf, 97),
    (0x875fd7, 98),
    (0x875fff, 99),
    (0x878700, 100),
    (0x87875f, 101),
    (0x878787, 102),
    (0x8787af, 103),
    (0x8787d7, 104),
    (0x8787ff, 105),
    (0x87af00, 106),
    (0x87af5f, 107),
    (0x87af87, 108),
    (0x87afaf, 109),
    (0x87afd7, 110),
    (0x87afff, 111),
    (0x87d700, 112),
    (0x87d75f, 113),
    (0x87d787, 114),
    (0x87d7af, 115),
    (0x87d7d7, 116),
    (0x87d7ff, 117),
    (0x87ff00, 118),
    (0x87ff5f, 119),
    (0x87ff87, 120),
    (0x87ffaf, 121),
    (0x87ffd7, 122),
    (0x87ffff, 123),
    (0xaf0000, 124),
    (0xaf005f, 125),
    (0xaf0087, 126),
    (0xaf00af, 127),
    (0xaf00d7, 128),
    (0xaf00ff, 129),
    (0xaf5f00, 130),
    (0xaf5f5f, 131),
    (0xaf5f87, 132),
    (0xaf5faf, 133),
    (0xaf5fd7, 134),
    (0xaf5fff, 135),
    (0xaf8700, 136),
    (0xaf875f, 137),
    (0xaf8787, 138),
    (0xaf87af, 139),
    (0xaf87d7, 140),
    (0xaf87ff, 141),
    (0xafaf00, 142),
    (0xafaf5f, 143),
    (0xafaf87, 144),
    (0xafafaf, 145),
    (0xafafd7, 146),
    (0xafafff, 147),
    (0xafd700, 148),
    (0xafd75f, 149),
    (0xafd787, 150),
    (0xafd7af, 151),
    (0xafd7d7, 152),
    (0xafd7ff, 153),
    (0xafff00, 154),
    (0xafff5f, 155),
    (0xafff87, 156),
    (0xafffaf, 157),
    (0xafffd7, 158),
    (0xafffff, 159),
    (0xd70000, 160),
    (0xd7005f, 161),
    (0xd70087, 162),
    (0xd700af, 163),
    (0xd700d7, 164),
    (0xd700ff, 165),
    (0xd75f00, 166),
    (0xd75f5f, 167),
    (0xd75f87, 168),
    (0xd75faf, 169),
    (0xd75fd7, 170),
    (0xd75fff, 171),
    (0xd78700, 172),
    (0xd7875f, 173),
    (0xd78787, 174),
    (0xd787af, 175),
    (0xd787d7, 176),
    (0xd787ff, 177),
    (0xd7af00, 178),
    (0xd7af5f, 179),
    (0xd7af87, 180),
    (0xd7afaf, 181),
    (0xd7afd7, 182),
    (0xd7afff, 183),
    (0xd7d700, 184),
    (0xd7d75f, 185),
    (0xd7d787, 186),
    (0xd7d7af, 187),
    (0xd7d7d7, 188),
    (0xd7d7ff, 189),
    (0xd7ff00, 190),
    (0xd7ff5f, 191),
    (0xd7ff87, 192),
    (0xd7ffaf, 193),
    (0xd7ffd7, 194),
    (0xd7ffff, 195),
    (0xff0000, 196),
    (0xff005f, 197),
    (0xff0087, 198),
    (0xff00af, 199),
    (0xff00d7, 200),
    (0xff00ff, 201),
    (0xff5f00, 202),
    (0xff5f5f, 203),
    (0xff5f87, 204),
    (0xff5faf, 205),
    (0xff5fd7, 206),
    (0xff5fff, 207),
    (0xff8700, 208),
    (0xff875f, 209),
    (0xff8787, 210),
    (0xff87af, 211),
    (0xff87d7, 212),
    (0xff87ff, 213),
    (0xffaf00, 214),
    (0xffaf5f, 215),
    (0xffaf87, 216),
    (0xffafaf, 217),
    (0xffafd7, 218),
    (0xffafff, 219),
    (0xffd700, 220),
    (0xffd75f, 221),
    (0xffd787, 222),
    (0xffd7af, 223),
    (0xffd7d7, 224),
    (0xffd7ff, 225),
    (0xffff00, 226),
    (0xffff5f, 227),
    (0xffff87, 228),
    (0xffffaf, 229),
    (0xffffd7, 230),
    (0xffffff, 231),
    (0x080808, 232),
    (0x121212, 233),
    (0x1c1c1c, 234),
    (0x262626, 235),
    (0x303030, 236),
    (0x3a3a3a, 237),
    (0x444444, 238),
    (0x4e4e4e, 239),
    (0x585858, 240),
    (0x626262, 241),
    (0x6c6c6c, 242),
    (0x767676, 243),
    (0x808080, 244),
    (0x8a8a8a, 245),
    (0x949494, 246),
    (0x9e9e9e, 247),
    (0xa8a8a8, 248),
    (0xb2b2b2, 249),
    (0xbcbcbc, 250),
    (0xc6c6c6, 251),
    (0xd0d0d0, 252),
    (0xdadada, 253),
    (0xe4e4e4, 254),
    (0xeeeeee, 255),
];

/// Nearest terminal256 index by Chroma's "redmean" Colour.Distance.
pub fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    let (ar, ag, ab) = (r as i64, g as i64, b as i64);
    let mut best = 0u8;
    let mut best_d = i64::MAX;
    for (hex, idx) in CHROMA_256 {
        let (br, bg, bb) = (
            (hex >> 16) as i64 & 0xff,
            (hex >> 8) as i64 & 0xff,
            hex as i64 & 0xff,
        );
        let rmean = (ar + br) / 2;
        let (dr, dg, db) = (ar - br, ag - bg, ab - bb);
        let d = (((512 + rmean) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rmean) * db * db) >> 8);
        if d <= best_d {
            best_d = d;
            best = idx;
        }
    }
    best
}

#[derive(Clone, Copy, Default)]
struct Entry {
    fg: Option<u32>,
    bg: Option<u32>,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// internal/tui/markdown/theme.go CodeBlock.Chroma
fn entry(t: TT) -> Option<Entry> {
    let fg = |c: u32| {
        Some(Entry {
            fg: Some(c),
            ..Default::default()
        })
    };
    match t {
        8000 => fg(0xC4C4C4),
        -11 => Some(Entry {
            fg: Some(0xF1F1F1),
            bg: Some(0xF05B5B),
            ..Default::default()
        }),
        6000 => fg(0x676767),
        6100 => fg(0xFF875F),
        1000 => fg(0x00AAFF),
        1005 => fg(0xFF5FD2),
        1003 => fg(0xFF5F87),
        1006 => fg(0x6E6ED8),
        4000 => fg(0xEF8080),
        5000 => fg(0xE8E8A8),
        2000 => fg(0xC4C4C4),
        2100 => fg(0xFF8EC7),
        2014 => fg(0xB083EA),
        2001 => fg(0x7A7AE6),
        2002 => Some(Entry {
            fg: Some(0xF1F1F1),
            bold: true,
            underline: true,
            ..Default::default()
        }),
        2004 => fg(0xFFFF87),
        2300 => fg(0x00D787),
        3200 => fg(0x6EEFC0),
        3100 => fg(0xC69669),
        3109 => fg(0xAFFFD7),
        7001 => fg(0xFD5B5B),
        7002 => Some(Entry {
            italic: true,
            ..Default::default()
        }),
        7005 => fg(0x00D787),
        7008 => Some(Entry {
            bold: true,
            ..Default::default()
        }),
        7009 => fg(0x777777),
        _ => None,
    }
}

/// Style.Get inheritance (type, sub-category, category, Text; the background is cleared).
fn token_style(t: TT) -> Style {
    let chain: Vec<Entry> = [t, sub_category(t), category(t), chroma::TEXT]
        .iter()
        .filter_map(|x| entry(*x))
        .collect();
    let fg = chain.iter().find_map(|e| e.fg);
    let bg = entry(t).and_then(|e| e.bg);
    let mut s = Style::default();
    if let Some(c) = fg {
        s = s.fg(c256(c));
    }
    if let Some(c) = bg {
        s = s.bg(c256(c));
    }
    if chain.iter().find(|e| e.bold).is_some() && entry(t).map(|e| e.bold).unwrap_or(true) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if chain.iter().any(|e| e.italic) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if chain.iter().any(|e| e.underline) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

fn c256(hex: u32) -> Color {
    Color::Indexed(nearest_256((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
}

/// Highlights a code block (`code` as written, `lang` from the fence) into lines.
pub fn highlight(code: &str, lang: &str) -> Block {
    let mut lines: Block = vec![];
    let mut cur: Vec<Span<'static>> = vec![];
    for (t, v) in chroma::tokenise_code(code, lang) {
        let st = token_style(t);
        for (i, part) in v.split('\n').enumerate() {
            if i > 0 {
                lines.push(Line::from(std::mem::take(&mut cur)));
            }
            if !part.is_empty() {
                cur.push(Span::styled(part.replace('\t', "    "), st));
            }
        }
    }
    if !cur.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_chroma_terminal256() {
        assert_eq!(nearest_256(0x00, 0xAA, 0xFF), 39);
        assert_eq!(nearest_256(0x00, 0xD7, 0x87), 42);
        assert_eq!(nearest_256(0xE8, 0xE8, 0xA8), 187);
        assert_eq!(nearest_256(0xC4, 0xC4, 0xC4), 251);
        assert_eq!(nearest_256(0xC6, 0x96, 0x69), 173);
    }

    #[test]
    fn go_tokens_get_chroma_colors() {
        let b = highlight("func main() {\n\tfmt.Println(\"hi\") // comment\n}\n", "go");
        let color_of = |line: usize, text: &str| {
            b[line]
                .spans
                .iter()
                .find(|s| s.content.contains(text))
                .map(|s| s.style.fg)
        };
        assert_eq!(color_of(0, "func"), Some(Some(Color::Indexed(39))));
        assert_eq!(color_of(0, "main"), Some(Some(Color::Indexed(42))));
        assert_eq!(color_of(1, "Println"), Some(Some(Color::Indexed(42))));
        assert_eq!(color_of(1, "hi"), Some(Some(Color::Indexed(173))));
        assert_eq!(color_of(1, "fmt"), Some(Some(Color::Indexed(251))));
        assert_eq!(color_of(2, "}"), Some(Some(Color::Indexed(187))));
    }
}
