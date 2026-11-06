//! Theme colors: the dark variants of the adaptive defaults, plus fixed colors.

use ratatui::style::{Color, Modifier, Style};

use crate::config::Config;
use crate::icons;

#[derive(Clone, Debug)]
pub struct Theme {
    pub selected_background: Color,
    pub primary_border: Color,
    pub faint_border: Color,
    pub secondary_border: Color,
    pub faint_text: Color,
    pub primary_text: Color,
    pub secondary_text: Color,
    pub inverted_text: Color,
    pub success_text: Color,
    pub warning_text: Color,
    pub error_text: Color,
    pub actor_text: Color,
    pub new_contributor_icon_color: Color,
    pub contributor_icon_color: Color,
    pub collaborator_icon_color: Color,
    pub member_icon_color: Color,
    pub owner_icon_color: Color,
    pub unknown_role_icon_color: Color,
    pub new_contributor_icon: String,
    pub contributor_icon: String,
    pub collaborator_icon: String,
    pub member_icon: String,
    pub owner_icon: String,
    pub unknown_role_icon: String,
}

pub const OPEN_ISSUE: Color = Color::Rgb(0x42, 0xA0, 0xFA);
pub const CLOSED_ISSUE: Color = Color::Rgb(0xC3, 0x80, 0x80);
pub const SUCCESS_GREEN: Color = Color::Rgb(0x3D, 0xF2, 0x94);
pub const OPEN_PR: Color = OPEN_ISSUE;
pub const CLOSED_PR: Color = Color::Rgb(0x65, 0x6C, 0x76);
pub const MERGED_PR: Color = Color::Rgb(0xA3, 0x71, 0xF7);
pub const LOGO: Color = Color::Rgb(0x00, 0xF9, 0xFB);
/// lipgloss.Darken(LogoColor, 0.7)
pub const LOGO_BG: Color = Color::Rgb(0, 74, 75);
pub const GOLD: Color = Color::Rgb(0xFF, 0xD7, 0x00);

impl Default for Theme {
    fn default() -> Self {
        Theme {
            primary_border: Color::Indexed(8),
            secondary_border: Color::Indexed(7),
            selected_background: Color::Indexed(236),
            faint_border: Color::Indexed(234),
            primary_text: Color::Indexed(15),
            secondary_text: Color::Indexed(251),
            faint_text: Color::Indexed(245),
            inverted_text: Color::Indexed(236),
            success_text: Color::Indexed(10),
            warning_text: Color::Indexed(11),
            error_text: Color::Indexed(9),
            actor_text: Color::Indexed(251),
            new_contributor_icon_color: Color::Indexed(77),
            contributor_icon_color: Color::Indexed(75),
            collaborator_icon_color: Color::Indexed(178),
            member_icon_color: Color::Indexed(178),
            owner_icon_color: Color::Indexed(178),
            unknown_role_icon_color: Color::Indexed(178),
            new_contributor_icon: icons::NEW_CONTRIBUTOR.into(),
            contributor_icon: icons::CONTRIBUTOR.into(),
            collaborator_icon: icons::COLLABORATOR.into(),
            member_icon: icons::MEMBER.into(),
            owner_icon: icons::OWNER.into(),
            unknown_role_icon: icons::UNKNOWN_ROLE.into(),
        }
    }
}

pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let expand = |h: &str| -> Option<u8> { u8::from_str_radix(h, 16).ok() };
        return match hex.len() {
            6 => Some(Color::Rgb(
                expand(&hex[0..2])?,
                expand(&hex[2..4])?,
                expand(&hex[4..6])?,
            )),
            3 => {
                let c: Vec<String> = hex.chars().map(|c| format!("{c}{c}")).collect();
                Some(Color::Rgb(expand(&c[0])?, expand(&c[1])?, expand(&c[2])?))
            }
            _ => None,
        };
    }
    s.parse::<u8>().ok().map(Color::Indexed)
}

/// lipgloss.Darken: scales each channel down, truncating.
pub fn darken(c: Color, pct: f64) -> Color {
    let mult = 1.0 - pct.clamp(0.0, 1.0);
    match c {
        Color::Rgb(r, g, b) => {
            let f = |v: u8| (v as f64 * mult) as u8;
            Color::Rgb(f(r), f(g), f(b))
        }
        other => other,
    }
}

/// lipgloss.Lighten: adds 255 * pct to each channel, capped at 255.
pub fn lighten(c: Color, pct: f64) -> Color {
    let add = 255.0 * pct.clamp(0.0, 1.0);
    match c {
        Color::Rgb(r, g, b) => {
            let f = |v: u8| (v as f64 + add).min(255.0) as u8;
            Color::Rgb(f(r), f(g), f(b))
        }
        other => other,
    }
}

impl Theme {
    pub fn from_config(cfg: &Config) -> Theme {
        let mut t = Theme::default();
        let shim = |s: &str, fallback: Color| -> Color {
            if s.is_empty() {
                fallback
            } else {
                parse_color(s).unwrap_or(fallback)
            }
        };
        if let Some(c) = &cfg.theme.colors {
            t.selected_background = shim(&c.background.selected, t.selected_background);
            t.primary_border = shim(&c.border.primary, t.primary_border);
            t.faint_border = shim(&c.border.faint, t.faint_border);
            t.secondary_border = shim(&c.border.secondary, t.secondary_border);
            t.faint_text = shim(&c.text.faint, t.faint_text);
            t.primary_text = shim(&c.text.primary, t.primary_text);
            t.secondary_text = shim(&c.text.secondary, t.secondary_text);
            t.inverted_text = shim(&c.text.inverted, t.inverted_text);
            t.success_text = shim(&c.text.success, t.success_text);
            t.warning_text = shim(&c.text.warning, t.warning_text);
            t.error_text = shim(&c.text.error, t.error_text);
            t.actor_text = shim(&c.text.actor, t.actor_text);
            t.new_contributor_icon_color =
                shim(&c.icon.newcontributor, t.new_contributor_icon_color);
            t.contributor_icon_color = shim(&c.icon.contributor, t.contributor_icon_color);
            t.collaborator_icon_color = shim(&c.icon.collaborator, t.collaborator_icon_color);
            t.member_icon_color = shim(&c.icon.member, t.member_icon_color);
            t.owner_icon_color = shim(&c.icon.owner, t.owner_icon_color);
        }
        if cfg.show_author_icons
            && let Some(i) = &cfg.theme.icons
        {
            let pick = |s: &str, f: &str| {
                if s.is_empty() {
                    f.to_string()
                } else {
                    s.to_string()
                }
            };
            t.new_contributor_icon = pick(&i.newcontributor, &t.new_contributor_icon);
            t.contributor_icon = pick(&i.contributor, &t.contributor_icon);
            t.collaborator_icon = pick(&i.collaborator, &t.collaborator_icon);
            t.member_icon = pick(&i.member, &t.member_icon);
            t.owner_icon = pick(&i.owner, &t.owner_icon);
            t.unknown_role_icon = pick(&i.unknownrole, &t.unknown_role_icon);
        }
        t
    }

    pub fn fg(&self, c: Color) -> Style {
        Style::default().fg(c)
    }
    pub fn faint(&self) -> Style {
        Style::default().fg(self.faint_text)
    }
    pub fn primary(&self) -> Style {
        Style::default().fg(self.primary_text)
    }
    pub fn secondary(&self) -> Style {
        Style::default().fg(self.secondary_text)
    }
    pub fn main_text(&self) -> Style {
        Style::default()
            .fg(self.primary_text)
            .add_modifier(Modifier::BOLD)
    }

    pub fn author_role_icon(&self, role: &str) -> (String, Color) {
        match role {
            "FIRST_TIMER" | "FIRST_TIME_CONTRIBUTOR" | "NONE" => (
                self.new_contributor_icon.clone(),
                self.new_contributor_icon_color,
            ),
            "COLLABORATOR" => (self.collaborator_icon.clone(), self.collaborator_icon_color),
            "CONTRIBUTOR" => (self.contributor_icon.clone(), self.contributor_icon_color),
            "MEMBER" => (self.member_icon.clone(), self.member_icon_color),
            "OWNER" => (self.owner_icon.clone(), self.owner_icon_color),
            _ => (self.unknown_role_icon.clone(), self.unknown_role_icon_color),
        }
    }

    pub fn waiting_glyph(&self) -> (&'static str, Style) {
        (icons::WAITING, self.fg(self.warning_text))
    }
    pub fn waiting_dot_glyph(&self) -> (&'static str, Style) {
        (icons::DOT, self.fg(self.warning_text))
    }
    pub fn failure_glyph(&self) -> (&'static str, Style) {
        (icons::FAILURE, self.fg(self.error_text))
    }
    pub fn success_glyph(&self) -> (&'static str, Style) {
        (icons::SUCCESS, self.fg(self.success_text))
    }
    pub fn comment_glyph(&self) -> (&'static str, Style) {
        (icons::COMMENT, self.fg(self.primary_text))
    }
    pub fn draft_glyph(&self) -> (&'static str, Style) {
        (icons::DRAFT, self.fg(self.primary_text))
    }
    pub fn person_glyph(&self) -> (&'static str, Style) {
        (icons::PERSON, self.fg(self.faint_text))
    }
    pub fn merged_glyph(&self) -> (&'static str, Style) {
        (icons::MERGED, self.fg(MERGED_PR))
    }
    pub fn action_required_glyph(&self) -> (&'static str, Style) {
        (icons::ACTION_REQUIRED, self.fg(self.warning_text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_ansi() {
        assert_eq!(parse_color("#2f334d"), Some(Color::Rgb(0x2f, 0x33, 0x4d)));
        assert_eq!(parse_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_color("236"), Some(Color::Indexed(236)));
        assert_eq!(parse_color("red"), None);
    }
}
