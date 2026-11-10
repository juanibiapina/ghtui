//! Theme colors: the defaults, overrides from the config, and fixed colors.

use ratatui::style::{Color, Modifier, Style};

use crate::config::{Config, ConfigColor};
use crate::icons;
use crate::model::AuthorAssociation;

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
/// `darken(LOGO, 0.7)`
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

/// Scales each channel down by `pct`, truncating.
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

/// Adds 255 * `pct` to each channel, capped at 255.
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
        let set = |target: &mut Color, value: &Option<ConfigColor>| {
            if let Some(c) = value {
                *target = c.0;
            }
        };
        if let Some(c) = &cfg.theme.colors {
            set(&mut t.selected_background, &c.background.selected);
            set(&mut t.primary_border, &c.border.primary);
            set(&mut t.faint_border, &c.border.faint);
            set(&mut t.secondary_border, &c.border.secondary);
            set(&mut t.faint_text, &c.text.faint);
            set(&mut t.primary_text, &c.text.primary);
            set(&mut t.secondary_text, &c.text.secondary);
            set(&mut t.inverted_text, &c.text.inverted);
            set(&mut t.success_text, &c.text.success);
            set(&mut t.warning_text, &c.text.warning);
            set(&mut t.error_text, &c.text.error);
            set(&mut t.actor_text, &c.text.actor);
            set(&mut t.new_contributor_icon_color, &c.icon.new_contributor);
            set(&mut t.contributor_icon_color, &c.icon.contributor);
            set(&mut t.collaborator_icon_color, &c.icon.collaborator);
            set(&mut t.member_icon_color, &c.icon.member);
            set(&mut t.owner_icon_color, &c.icon.owner);
            set(&mut t.unknown_role_icon_color, &c.icon.unknown_role);
        }
        if cfg.show_author_icons
            && let Some(i) = &cfg.theme.icons
        {
            let set = |target: &mut String, value: &Option<String>| {
                if let Some(v) = value {
                    *target = v.clone();
                }
            };
            set(&mut t.new_contributor_icon, &i.new_contributor);
            set(&mut t.contributor_icon, &i.contributor);
            set(&mut t.collaborator_icon, &i.collaborator);
            set(&mut t.member_icon, &i.member);
            set(&mut t.owner_icon, &i.owner);
            set(&mut t.unknown_role_icon, &i.unknown_role);
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

    pub fn author_role_icon(&self, role: AuthorAssociation) -> (String, Color) {
        use AuthorAssociation as A;
        match role {
            A::FirstTimer | A::FirstTimeContributor | A::None => (
                self.new_contributor_icon.clone(),
                self.new_contributor_icon_color,
            ),
            A::Collaborator => (self.collaborator_icon.clone(), self.collaborator_icon_color),
            A::Contributor => (self.contributor_icon.clone(), self.contributor_icon_color),
            A::Member => (self.member_icon.clone(), self.member_icon_color),
            A::Owner => (self.owner_icon.clone(), self.owner_icon_color),
            A::Mannequin | A::Unknown => {
                (self.unknown_role_icon.clone(), self.unknown_role_icon_color)
            }
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
