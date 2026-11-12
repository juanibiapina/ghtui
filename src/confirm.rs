//! A modal yes/no dialog drawn over the screen.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph, Widget};

use crate::model::Issue;
use crate::mouse::{HitMap, Target};
use crate::msg::SectionKey;
use crate::section::PrRow;
use crate::text::{str_width, wrap_str};
use crate::theme::Theme;

const MAX_WIDTH: u16 = 56;
const MARGIN: u16 = 2;
const PAD_X: u16 = 2;
const PAD_Y: u16 = 1;
const BUTTON_GAP: u16 = 3;
const CANCEL_LABEL: &str = "Cancel";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Confirm,
    Cancel,
}

/// What to do on yes. It holds its target as it was when the dialog opened.
#[derive(Clone, Debug)]
pub enum Action {
    Quit,
    Pr {
        action: &'static str,
        pr: Box<PrRow>,
        key: SectionKey,
    },
    Issue {
        action: &'static str,
        issue: Box<Issue>,
        key: SectionKey,
    },
    DeleteBranch {
        name: String,
    },
    MarkAllDone {
        ids: Vec<String>,
        key: SectionKey,
    },
    DiscardEditor {
        is_pr: bool,
        review: bool,
    },
}

pub enum Answer {
    Confirmed(Action),
    Cancelled,
    Open(Confirm),
}

/// A yes/no dialog about one action.
#[derive(Clone, Debug)]
pub struct Confirm {
    action: Action,
    title: String,
    message: Vec<String>,
    confirm_label: &'static str,
    danger: bool,
    focus: Choice,
}

struct Placement {
    popup: Rect,
    inner: Rect,
    lines: Vec<Line<'static>>,
    buttons: [(Rect, Choice); 2],
}

impl Confirm {
    pub fn new(action: Action) -> Confirm {
        let (title, confirm_label, danger, message) = match &action {
            Action::Quit => ("Quit ghtui?".into(), "Quit", false, vec![]),
            Action::Pr { action, pr, .. } => {
                let n = pr.primary.number;
                let (title, label) = match *action {
                    "close" => (format!("Close PR #{n}?"), "Close"),
                    "reopen" => (format!("Reopen PR #{n}?"), "Reopen"),
                    "ready" => (format!("Mark PR #{n} as ready for review?"), "Mark ready"),
                    "merge" => (format!("Merge PR #{n}?"), "Merge"),
                    "update" => (format!("Update the branch of PR #{n}?"), "Update"),
                    _ => (format!("Approve all workflows for PR #{n}?"), "Approve"),
                };
                let message = vec![
                    pr.primary.title.clone(),
                    pr.primary.repository.name_with_owner.clone(),
                ];
                (title, label, *action == "close", message)
            }
            Action::Issue { action, issue, .. } => {
                let close = *action == "close";
                let verb = if close { "Close" } else { "Reopen" };
                let message = vec![
                    issue.title.clone(),
                    issue.repository.name_with_owner.clone(),
                ];
                (
                    format!("{verb} issue #{}?", issue.number),
                    verb,
                    close,
                    message,
                )
            }
            Action::DeleteBranch { name } => {
                (format!("Delete branch {name}?"), "Delete", true, vec![])
            }
            Action::MarkAllDone { ids, .. } => {
                let n = ids.len();
                let s = if n == 1 { "" } else { "s" };
                let title = format!("Mark {n} notification{s} as done?");
                (title, "Mark done", false, vec![])
            }
            Action::DiscardEditor { review, .. } => {
                let what = if *review { "review" } else { "comment" };
                (format!("Discard {what}?"), "Discard", true, vec![])
            }
        };
        Confirm {
            action,
            title,
            message: message.into_iter().filter(|l| !l.is_empty()).collect(),
            confirm_label,
            danger,
            focus: Choice::Confirm,
        }
    }

    pub fn handle_key(mut self, key: &str) -> Answer {
        match key {
            "y" | "Y" => self.answer(Choice::Confirm),
            "n" | "N" | "esc" | "ctrl+c" => self.answer(Choice::Cancel),
            "enter" | "space" => {
                let focus = self.focus;
                self.answer(focus)
            }
            "left" | "h" => {
                self.focus = Choice::Confirm;
                Answer::Open(self)
            }
            "right" | "l" => {
                self.focus = Choice::Cancel;
                Answer::Open(self)
            }
            "tab" | "shift+tab" => {
                self.focus = match self.focus {
                    Choice::Confirm => Choice::Cancel,
                    Choice::Cancel => Choice::Confirm,
                };
                Answer::Open(self)
            }
            _ => Answer::Open(self),
        }
    }

    /// Answers a click on `target` from the last frame: a button presses it, the
    /// backdrop cancels.
    pub fn click(self, target: Option<Target>) -> Answer {
        match target {
            Some(Target::ConfirmButton(choice)) => self.answer(choice),
            Some(Target::Backdrop) => self.answer(Choice::Cancel),
            _ => Answer::Open(self),
        }
    }

    fn answer(self, choice: Choice) -> Answer {
        match choice {
            Choice::Confirm => Answer::Confirmed(self.action),
            Choice::Cancel => Answer::Cancelled,
        }
    }

    fn label(&self, choice: Choice) -> String {
        let text = match choice {
            Choice::Confirm => self.confirm_label,
            Choice::Cancel => CANCEL_LABEL,
        };
        format!("[ {text} ]")
    }

    fn placement(&self, area: Rect, t: &Theme) -> Placement {
        let width = MAX_WIDTH.min(area.width.saturating_sub(2 * MARGIN));
        let text_w = width.saturating_sub(2 + 2 * PAD_X).max(1) as usize;

        let mut lines = wrap_str(
            &self.title,
            text_w,
            Style::new().fg(t.primary_text).add_modifier(Modifier::BOLD),
        );
        if !self.message.is_empty() {
            lines.push(Line::default());
            for m in &self.message {
                lines.extend(wrap_str(m, text_w, t.secondary()));
            }
        }
        lines.push(Line::default());

        let height = (lines.len() as u16 + 1 + 2 + 2 * PAD_Y).min(area.height);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        let inner = popup.inner(ratatui::layout::Margin {
            horizontal: 1 + PAD_X,
            vertical: 1 + PAD_Y,
        });

        let confirm_w = str_width(&self.label(Choice::Confirm)) as u16;
        let cancel_w = str_width(&self.label(Choice::Cancel)) as u16;
        let total = confirm_w + BUTTON_GAP + cancel_w;
        let x = inner.x + inner.width.saturating_sub(total) / 2;
        let y = (inner.y + lines.len() as u16).min(inner.bottom().saturating_sub(1));
        let row = Rect::new(inner.x, y, inner.width, 1);
        let confirm = Rect::new(x, y, confirm_w, 1).intersection(row);
        let cancel = Rect::new(x + confirm_w + BUTTON_GAP, y, cancel_w, 1).intersection(row);
        Placement {
            popup,
            inner,
            lines,
            buttons: [(confirm, Choice::Confirm), (cancel, Choice::Cancel)],
        }
    }

    /// Dims everything already in `buf` and draws the dialog in the middle of `area`.
    pub fn render(&self, area: Rect, buf: &mut Buffer, t: &Theme, hits: &mut HitMap) {
        buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
        hits.push(area, Target::Backdrop);
        let p = self.placement(area, t);
        if p.popup.is_empty() {
            return;
        }
        Clear.render(p.popup, buf);
        hits.push(p.popup, Target::ConfirmPopup);

        let border = Style::new().fg(t.primary_border);
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(border)
            .title_bottom(
                Line::from(vec![
                    Span::styled(" y", t.primary()),
                    Span::styled(" confirm · ", t.faint()),
                    Span::styled("n", t.primary()),
                    Span::styled(" cancel ", t.faint()),
                    Span::styled("─", border),
                ])
                .right_aligned(),
            )
            .padding(Padding::symmetric(PAD_X, PAD_Y))
            .render(p.popup, buf);
        Paragraph::new(p.lines).render(p.inner, buf);

        for (rect, choice) in p.buttons {
            let fg = match choice {
                Choice::Confirm if self.danger => t.error_text,
                _ => t.primary_text,
            };
            let mut style = Style::new().fg(fg);
            if choice == self.focus {
                style = style.bg(t.selected_background).bold();
            }
            Span::styled(self.label(choice), style).render(rect, buf);
            hits.push(rect, Target::ConfirmButton(choice));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirmed(a: Answer) -> bool {
        matches!(a, Answer::Confirmed(Action::Quit))
    }

    fn press(keys: &[&str]) -> Answer {
        let mut c = Confirm::new(Action::Quit);
        for k in keys {
            match c.handle_key(k) {
                Answer::Open(next) => c = next,
                done => return done,
            }
        }
        Answer::Open(c)
    }

    #[test]
    fn enter_presses_the_focused_button() {
        assert!(confirmed(press(&["enter"])));
        assert!(matches!(press(&["tab", "enter"]), Answer::Cancelled));
        assert!(confirmed(press(&["l", "h", "space"])));
    }

    #[test]
    fn y_and_n_answer_whatever_has_focus() {
        assert!(confirmed(press(&["l", "y"])));
        assert!(matches!(press(&["n"]), Answer::Cancelled));
        assert!(matches!(press(&["esc"]), Answer::Cancelled));
    }

    #[test]
    fn other_keys_leave_the_dialog_open() {
        assert!(matches!(press(&["x"]), Answer::Open(_)));
    }
}
