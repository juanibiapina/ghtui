//! Help popup: the keys that work in the current context, grouped by purpose,
//! drawn centered over a dimmed dashboard.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    StatefulWidget, Widget,
};

use crate::config::ViewType;
use crate::keys::{Binding, Keys, NotificationSubject};
use crate::text::str_width;
use crate::theme::Theme;

const MAX_COLUMNS: usize = 3;
const COLUMN_GAP: u16 = 3;
const KEY_GAP: usize = 2;
const PAD_X: u16 = 2;
const PAD_Y: u16 = 1;
const SCREEN_MARGIN: u16 = 2;

struct Group {
    title: &'static str,
    bindings: Vec<Binding>,
}

impl Group {
    fn key_width(&self) -> usize {
        self.bindings
            .iter()
            .map(|b| str_width(&b.help_key))
            .max()
            .unwrap_or(0)
    }

    fn width(&self) -> u16 {
        let kw = self.key_width();
        let rows = self
            .bindings
            .iter()
            .map(|b| kw + KEY_GAP + str_width(&b.help_desc));
        rows.chain([str_width(self.title)]).max().unwrap_or(0) as u16
    }

    fn height(&self) -> u16 {
        1 + self.bindings.len() as u16
    }
}

/// True when every key of `b` is taken by one of `by`, which the dispatcher checks first.
fn shadowed(b: &Binding, by: &[&Binding]) -> bool {
    !b.keys.is_empty() && b.keys.iter().all(|k| by.iter().any(|s| s.matches(k)))
}

struct Placement {
    columns: Vec<Vec<usize>>,
    widths: Vec<u16>,
    popup: Rect,
    max_scroll: u16,
    page: u16,
}

pub struct Help {
    title: &'static str,
    groups: Vec<Group>,
    scroll: u16,
}

impl Help {
    /// Captures the groups for the context where help was opened.
    pub fn new(keys: &Keys, view: ViewType, subject: NotificationSubject) -> Help {
        let u = &keys.u;
        let (p, i, n, br) = (&keys.pr, &keys.issue, &keys.notif, &keys.branch);
        let pr_shown = view == ViewType::Prs
            || (view == ViewType::Notifications && subject == NotificationSubject::Pr);

        let mut sections = vec![
            u.search.clone(),
            u.refresh.clone(),
            u.refresh_all.clone(),
            u.new_section.clone(),
            u.remove_section.clone(),
        ];
        sections.extend(match view {
            ViewType::Prs => vec![p.toggle_smart_filtering.clone(), p.view_issues.clone()],
            ViewType::Issues => vec![i.toggle_smart_filtering.clone(), i.view_prs.clone()],
            ViewType::Repo => vec![br.view_prs.clone()],
            ViewType::Notifications => {
                vec![n.toggle_smart_filtering.clone(), n.switch_to_prs.clone()]
            }
        });

        let mut preview = vec![
            u.toggle_preview.clone(),
            u.toggle_preview_position.clone(),
            u.page_down.clone(),
            u.page_up.clone(),
        ];
        if pr_shown {
            preview.extend([
                p.prev_sidebar_tab.clone(),
                p.next_sidebar_tab.clone(),
                p.summary_view_more.clone(),
            ]);
        }

        let mut review = vec![
            p.diff.clone(),
            p.checkout.clone(),
            p.comment.clone(),
            p.approve.clone(),
            p.approve_workflows.clone(),
        ];
        if view == ViewType::Prs {
            review.push(p.watch_checks.clone());
        }
        let pr_groups = vec![
            Group {
                title: "Review",
                bindings: review,
            },
            Group {
                title: "Pull request",
                bindings: vec![
                    p.assign.clone(),
                    p.unassign.clone(),
                    p.label.clone(),
                    p.ready.clone(),
                    p.merge.clone(),
                    p.update.clone(),
                    p.close.clone(),
                    p.reopen.clone(),
                ],
            },
        ];
        let issue_group = || Group {
            title: "Issue",
            bindings: vec![
                i.assign.clone(),
                i.unassign.clone(),
                i.label.clone(),
                i.comment.clone(),
                i.checkout.clone(),
                i.close.clone(),
                i.reopen.clone(),
            ],
        };

        let mut groups = vec![
            Group {
                title: "Navigation",
                bindings: vec![
                    u.up.clone(),
                    u.down.clone(),
                    u.prev_section.clone(),
                    u.next_section.clone(),
                    u.first_line.clone(),
                    u.last_line.clone(),
                ],
            },
            Group {
                title: "Sections",
                bindings: sections,
            },
            Group {
                title: "Preview",
                bindings: preview,
            },
            Group {
                title: "General",
                bindings: vec![
                    u.open_github.clone(),
                    u.copy_number.clone(),
                    u.copy_url.clone(),
                    u.help.clone(),
                    u.quit.clone(),
                ],
            },
        ];

        let mut custom = keys.custom_universal.clone();
        match view {
            ViewType::Prs => {
                groups.extend(pr_groups);
                custom.extend(keys.custom_pr.clone());
            }
            ViewType::Issues => {
                groups.push(issue_group());
                custom.extend(keys.custom_issue.clone());
            }
            ViewType::Repo => {
                let branch_keys: Vec<&Binding> = br
                    .all()
                    .into_iter()
                    .filter(|b| !std::ptr::eq(*b, &br.view_prs))
                    .collect();
                for g in &mut groups {
                    g.bindings.retain(|b| !shadowed(b, &branch_keys));
                }
                groups.push(Group {
                    title: "Branch",
                    bindings: vec![
                        br.checkout.clone(),
                        br.new.clone(),
                        br.fast_forward.clone(),
                        br.push.clone(),
                        br.force_push.clone(),
                        br.delete.clone(),
                    ],
                });
                groups.push(Group {
                    title: "Pull request",
                    bindings: vec![br.create_pr.clone(), br.update_pr.clone()],
                });
                custom.extend(keys.custom_branch.clone());
            }
            ViewType::Notifications => {
                let subject_groups = match subject {
                    NotificationSubject::Pr => pr_groups,
                    NotificationSubject::Issue => vec![issue_group()],
                    NotificationSubject::None => vec![],
                };
                let subject_keys: Vec<&Binding> =
                    subject_groups.iter().flat_map(|g| &g.bindings).collect();
                let notification: Vec<Binding> = [
                    &n.view,
                    &n.back_to_notification,
                    &n.mark_as_done,
                    &n.mark_all_as_done,
                    &n.mark_as_read,
                    &n.mark_all_as_read,
                    &n.unsubscribe,
                    &n.toggle_bookmark,
                    &n.sort_by_repo,
                ]
                .into_iter()
                .filter(|b| !shadowed(b, &subject_keys))
                .cloned()
                .collect();
                groups.push(Group {
                    title: "Notification",
                    bindings: notification,
                });
                groups.extend(subject_groups);
                custom.extend(keys.custom_notification.clone());
                match subject {
                    NotificationSubject::Pr => custom.extend(keys.custom_pr.clone()),
                    NotificationSubject::Issue => custom.extend(keys.custom_issue.clone()),
                    NotificationSubject::None => {}
                }
            }
        }
        groups.push(Group {
            title: "Custom commands",
            bindings: custom,
        });

        for g in &mut groups {
            g.bindings.retain(|b| !b.keys.is_empty());
        }
        groups.retain(|g| !g.bindings.is_empty());

        let title = match view {
            ViewType::Prs => "Pull requests",
            ViewType::Issues => "Issues",
            ViewType::Repo => "Branches",
            ViewType::Notifications => "Notifications",
        };
        Help {
            title,
            groups,
            scroll: 0,
        }
    }

    /// Returns true when the popup should close. `area` is the terminal size.
    pub fn handle_key(&mut self, key: &str, keys: &Keys, area: Rect) -> bool {
        let u = &keys.u;
        if key == "esc" || u.help.matches(key) || u.quit.matches(key) {
            return true;
        }
        let p = self.placement(area);
        let scroll = self.scroll.min(p.max_scroll);
        self.scroll = if u.down.matches(key) {
            scroll.saturating_add(1)
        } else if u.up.matches(key) {
            scroll.saturating_sub(1)
        } else if u.page_down.matches(key) {
            scroll.saturating_add(p.page)
        } else if u.page_up.matches(key) {
            scroll.saturating_sub(p.page)
        } else if u.first_line.matches(key) {
            0
        } else if u.last_line.matches(key) {
            p.max_scroll
        } else {
            scroll
        }
        .min(p.max_scroll);
        false
    }

    /// Draws the popup over everything already in `buf`.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
        let p = self.placement(area);
        if p.popup.is_empty() {
            return;
        }
        Clear.render(p.popup, buf);

        let border = Style::new().fg(theme.primary_border);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(border)
            .title(Line::from(vec![
                Span::styled("─", border),
                Span::styled(
                    format!(" Keybindings · {} ", self.title),
                    theme.primary().bold(),
                ),
            ]))
            .title_bottom(
                Line::from(vec![
                    Span::styled(" esc", theme.primary()),
                    Span::styled(" close ", theme.faint()),
                    Span::styled("─", border),
                ])
                .right_aligned(),
            )
            .padding(Padding::symmetric(PAD_X, PAD_Y));
        let inner = block.inner(p.popup);
        block.render(p.popup, buf);

        let scroll = self.scroll.min(p.max_scroll);
        let rects = Layout::horizontal(p.widths.iter().map(|w| Constraint::Length(*w)))
            .spacing(COLUMN_GAP)
            .split(inner);
        for (col, rect) in p.columns.iter().zip(rects.iter()) {
            Paragraph::new(self.column_lines(col, theme))
                .scroll((scroll, 0))
                .render(*rect, buf);
        }

        if p.max_scroll > 0 {
            let mut state = ScrollbarState::new(p.max_scroll as usize + 1)
                .viewport_content_length(inner.height as usize)
                .position(scroll as usize);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .track_style(border)
                .thumb_symbol("┃")
                .thumb_style(theme.primary())
                .render(
                    p.popup.inner(Margin {
                        vertical: 1,
                        horizontal: 0,
                    }),
                    buf,
                    &mut state,
                );
        }
    }

    fn column_lines(&self, col: &[usize], theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = vec![];
        for (n, &gi) in col.iter().enumerate() {
            let g = &self.groups[gi];
            if n > 0 {
                lines.push(Line::default());
            }
            lines.push(Line::styled(
                g.title,
                Style::new()
                    .fg(theme.secondary_text)
                    .add_modifier(Modifier::BOLD),
            ));
            let kw = g.key_width();
            for b in &g.bindings {
                let pad = kw - str_width(&b.help_key) + KEY_GAP;
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}{}", b.help_key, " ".repeat(pad)),
                        theme.primary(),
                    ),
                    Span::styled(b.help_desc.clone(), theme.faint()),
                ]));
            }
        }
        lines
    }

    /// Splits the groups into columns in reading order and sizes the popup.
    fn placement(&self, area: Rect) -> Placement {
        let chrome_w = 2 + 2 * PAD_X;
        let chrome_h = 2 + 2 * PAD_Y;
        let max_inner_w = area
            .width
            .saturating_sub(2 * SCREEN_MARGIN + chrome_w)
            .max(1);

        let mut chosen = None;
        for n in (1..=MAX_COLUMNS.min(self.groups.len().max(1))).rev() {
            let columns = self.split_columns(n);
            let widths: Vec<u16> = columns
                .iter()
                .map(|c| c.iter().map(|&g| self.groups[g].width()).max().unwrap_or(0))
                .collect();
            let total = widths.iter().sum::<u16>() + COLUMN_GAP * (widths.len() as u16 - 1);
            if total <= max_inner_w || n == 1 {
                chosen = Some((columns, widths, total));
                break;
            }
        }
        let (columns, widths, total) = chosen.unwrap_or_default();

        let content_height = columns
            .iter()
            .map(|c| self.column_height(c))
            .max()
            .unwrap_or(0);
        let max_h = area.height.saturating_sub(SCREEN_MARGIN);
        let popup = area.centered(
            Constraint::Length((total + chrome_w).min(area.width)),
            Constraint::Length((content_height + chrome_h).min(max_h)),
        );
        let inner_h = popup.height.saturating_sub(chrome_h);
        Placement {
            columns,
            widths,
            popup,
            max_scroll: content_height.saturating_sub(inner_h),
            page: (inner_h / 2).max(1),
        }
    }

    fn column_height(&self, col: &[usize]) -> u16 {
        let groups: u16 = col.iter().map(|&g| self.groups[g].height()).sum();
        groups + col.len().saturating_sub(1) as u16
    }

    fn split_columns(&self, n: usize) -> Vec<Vec<usize>> {
        let all: Vec<usize> = (0..self.groups.len()).collect();
        let target = self.column_height(&all).div_ceil(n as u16);
        let mut columns: Vec<Vec<usize>> = vec![vec![]];
        for g in all {
            let full = columns.len() == n;
            let current = columns.last_mut().unwrap();
            let grown = self.column_height(current) + 1 + self.groups[g].height();
            if !current.is_empty() && grown > target && !full {
                columns.push(vec![g]);
            } else {
                current.push(g);
            }
        }
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Keybinding, Keybindings};

    fn titles(h: &Help) -> Vec<&str> {
        h.groups.iter().map(|g| g.title).collect()
    }

    fn group<'a>(h: &'a Help, title: &str) -> Option<&'a Group> {
        h.groups.iter().find(|g| g.title == title)
    }

    fn has_key(h: &Help, title: &str, key: &str) -> bool {
        group(h, title).is_some_and(|g| g.bindings.iter().any(|b| b.matches(key)))
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn prs_view_groups_by_purpose() {
        let h = Help::new(&Keys::default(), ViewType::Prs, NotificationSubject::None);
        assert_eq!(
            titles(&h),
            vec![
                "Navigation",
                "Sections",
                "Preview",
                "General",
                "Review",
                "Pull request"
            ]
        );
    }

    #[test]
    fn unbound_keys_never_show() {
        let h = Help::new(&Keys::default(), ViewType::Prs, NotificationSubject::None);
        let all = h.groups.iter().flat_map(|g| &g.bindings);
        assert!(all.into_iter().all(|b| !b.keys.is_empty()));
    }

    #[test]
    fn notification_keys_shadowed_by_the_loaded_pr_are_hidden() {
        let h = Help::new(
            &Keys::default(),
            ViewType::Notifications,
            NotificationSubject::Pr,
        );
        assert!(has_key(&h, "Pull request", "m"));
        assert!(!has_key(&h, "Notification", "m"));
        assert!(!has_key(&h, "Notification", "u"));
        assert!(has_key(&h, "Notification", "D"));
    }

    #[test]
    fn branch_keys_hide_the_universal_keys_they_take_over() {
        let h = Help::new(&Keys::default(), ViewType::Repo, NotificationSubject::None);
        assert!(has_key(&h, "Branch", "P"));
        assert!(!has_key(&h, "Preview", "P"));
        assert!(has_key(&h, "Preview", "p"));
        assert!(has_key(&h, "Sections", "s"));
    }

    #[test]
    fn notifications_without_subject_have_no_pr_groups() {
        let h = Help::new(
            &Keys::default(),
            ViewType::Notifications,
            NotificationSubject::None,
        );
        assert!(group(&h, "Review").is_none());
        assert!(has_key(&h, "Notification", "m"));
    }

    #[test]
    fn custom_commands_get_their_own_group() {
        let h = Help::new(&Keys::default(), ViewType::Prs, NotificationSubject::None);
        assert!(group(&h, "Custom commands").is_none());

        let mut keys = Keys::default();
        keys.rebind(&Keybindings {
            prs: vec![Keybinding {
                key: "O".into(),
                command: "echo {{.PrNumber}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        let h = Help::new(&keys, ViewType::Prs, NotificationSubject::None);
        assert!(has_key(&h, "Custom commands", "O"));
    }

    #[test]
    fn esc_help_and_quit_close_it() {
        let keys = Keys::default();
        let area = Rect::new(0, 0, 200, 50);
        let mut h = Help::new(&keys, ViewType::Prs, NotificationSubject::None);
        for key in ["esc", "?", "q"] {
            assert!(h.handle_key(key, &keys, area), "{key} should close");
        }
        for key in ["j", "x"] {
            assert!(!h.handle_key(key, &keys, area), "{key} should not close");
        }
    }

    #[test]
    fn rebound_help_key_closes_it() {
        let mut keys = Keys::default();
        keys.rebind(&Keybindings {
            universal: vec![Keybinding {
                key: "H".into(),
                builtin: "help".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        let mut h = Help::new(&keys, ViewType::Prs, NotificationSubject::None);
        assert!(h.handle_key("H", &keys, Rect::new(0, 0, 200, 50)));
    }

    #[test]
    fn scrolls_within_the_content() {
        let keys = Keys::default();
        let theme = Theme::default();
        let area = Rect::new(0, 0, 50, 12);
        let mut h = Help::new(&keys, ViewType::Prs, NotificationSubject::None);
        let last = h
            .groups
            .last()
            .unwrap()
            .bindings
            .last()
            .unwrap()
            .help_desc
            .clone();
        let shows_last = |h: &Help| {
            let mut buf = Buffer::empty(area);
            h.render(area, &mut buf, &theme);
            (0..area.height).any(|y| row(&buf, y).contains(&last))
        };
        assert!(!shows_last(&h));

        h.handle_key("G", &keys, area);
        assert!(shows_last(&h));
        h.handle_key("j", &keys, area);
        assert!(shows_last(&h));
        h.handle_key("k", &keys, area);
        assert!(!shows_last(&h));
    }

    #[test]
    fn renders_a_centered_popup_over_a_dimmed_screen() {
        let theme = Theme::default();
        let h = Help::new(&Keys::default(), ViewType::Prs, NotificationSubject::None);
        for (w, ht) in [(200, 50), (50, 12)] {
            let area = Rect::new(0, 0, w, ht);
            let mut buf = Buffer::empty(area);
            h.render(area, &mut buf, &theme);
            let p = h.placement(area);
            assert!(area.contains(p.popup.as_position()));
            assert!(p.popup.right() <= area.right() && p.popup.bottom() <= area.bottom());
            let (left, right) = (p.popup.x, w - p.popup.right());
            assert!(left.abs_diff(right) <= 1, "{left} vs {right}");
            assert!(row(&buf, p.popup.y).contains("Keybindings · Pull requests"));
            assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
            let scrollbar = buf[(p.popup.right() - 1, p.popup.y + 1)].symbol() == "┃";
            assert_eq!(scrollbar, p.max_scroll > 0, "{w}x{ht}");
        }
        assert_eq!(h.placement(Rect::new(0, 0, 200, 50)).max_scroll, 0);
        assert!(h.placement(Rect::new(0, 0, 50, 12)).max_scroll > 0);
    }
}
