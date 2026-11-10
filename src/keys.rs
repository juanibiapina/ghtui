//! Keybindings: default bindings, help text and rebinding from config. Keys are
//! matched by bubbletea-style names ("ctrl+d", "alt+d", "enter", "space", "J").

use anyhow::{Result, bail};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{Keybinding, Keybindings};
use crate::icons;
use crate::util::truncate_command;

#[derive(Clone, Debug, Default)]
pub struct Binding {
    pub keys: Vec<String>,
    pub help_key: String,
    pub help_desc: String,
}

impl Binding {
    fn new(keys: &[&str], help_key: &str, desc: &str) -> Self {
        Binding {
            keys: keys.iter().map(|s| s.to_string()).collect(),
            help_key: help_key.into(),
            help_desc: desc.into(),
        }
    }
    pub fn matches(&self, key: &str) -> bool {
        self.keys.iter().any(|k| k == key)
    }
    fn rebind(&mut self, kb: &Keybinding, desc_from_command: bool) {
        self.keys = vec![kb.key.clone()];
        let mut desc = self.help_desc.clone();
        if !kb.name.is_empty() {
            desc = kb.name.clone();
        } else if desc_from_command && !kb.command.is_empty() {
            desc = kb.command.clone();
        }
        self.help_key = kb.key.clone();
        self.help_desc = desc;
    }
}

#[derive(Clone, Debug)]
pub struct Universal {
    pub up: Binding,
    pub down: Binding,
    pub first_line: Binding,
    pub last_line: Binding,
    pub toggle_preview: Binding,
    pub toggle_preview_position: Binding,
    pub open_github: Binding,
    pub refresh: Binding,
    pub refresh_all: Binding,
    pub redraw: Binding,
    pub page_down: Binding,
    pub page_up: Binding,
    pub next_section: Binding,
    pub prev_section: Binding,
    pub search: Binding,
    pub copy_url: Binding,
    pub copy_number: Binding,
    pub section_mode: Binding,
    pub new_section: Binding,
    pub remove_section: Binding,
    pub help: Binding,
    pub quit: Binding,
}

#[derive(Clone, Debug)]
pub struct PrKeys {
    pub prev_preview_tab: Binding,
    pub next_preview_tab: Binding,
    pub approve: Binding,
    pub assign: Binding,
    pub unassign: Binding,
    pub label: Binding,
    pub comment: Binding,
    pub diff: Binding,
    pub checkout: Binding,
    pub close: Binding,
    pub summary_view_more: Binding,
    pub ready: Binding,
    pub reopen: Binding,
    pub merge: Binding,
    pub update: Binding,
    pub watch_checks: Binding,
    pub approve_workflows: Binding,
    pub toggle_smart_filtering: Binding,
    pub view_issues: Binding,
}

#[derive(Clone, Debug)]
pub struct IssueKeys {
    pub label: Binding,
    pub assign: Binding,
    pub unassign: Binding,
    pub comment: Binding,
    pub checkout: Binding,
    pub close: Binding,
    pub reopen: Binding,
    pub toggle_smart_filtering: Binding,
    pub view_prs: Binding,
}

#[derive(Clone, Debug)]
pub struct NotificationKeys {
    pub mark_as_done: Binding,
    pub mark_all_as_done: Binding,
    pub unsubscribe: Binding,
    pub toggle_bookmark: Binding,
    pub open: Binding,
    pub sort_by_repo: Binding,
    pub switch_to_prs: Binding,
    pub toggle_smart_filtering: Binding,
}

#[derive(Clone, Debug)]
pub struct BranchKeys {
    pub checkout: Binding,
    pub new: Binding,
    pub create_pr: Binding,
    pub fast_forward: Binding,
    pub push: Binding,
    pub force_push: Binding,
    pub delete: Binding,
    pub update_pr: Binding,
    pub view_prs: Binding,
}

impl BranchKeys {
    pub fn all(&self) -> [&Binding; 9] {
        [
            &self.checkout,
            &self.new,
            &self.create_pr,
            &self.fast_forward,
            &self.push,
            &self.force_push,
            &self.delete,
            &self.update_pr,
            &self.view_prs,
        ]
    }

    pub fn binds(&self, key: &str) -> bool {
        self.all().iter().any(|b| b.matches(key))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NotificationSubject {
    #[default]
    None,
    Pr,
    Issue,
}

#[derive(Clone, Debug)]
pub struct Keys {
    pub u: Universal,
    pub pr: PrKeys,
    pub issue: IssueKeys,
    pub notif: NotificationKeys,
    pub branch: BranchKeys,
    pub custom_universal: Vec<Binding>,
    pub custom_pr: Vec<Binding>,
    pub custom_issue: Vec<Binding>,
    pub custom_branch: Vec<Binding>,
    pub custom_notification: Vec<Binding>,
}

impl Default for Keys {
    fn default() -> Self {
        let b = Binding::new;
        Keys {
            u: Universal {
                up: b(&["up", "k"], "↑/k", "move up"),
                down: b(&["down", "j"], "↓/j", "move down"),
                first_line: b(&["g", "home"], "g/home", "first item"),
                last_line: b(&["G", "end"], "G/end", "last item"),
                toggle_preview: b(&["p"], "p", "toggle preview"),
                toggle_preview_position: b(&["P"], "P", "toggle preview position"),
                open_github: b(&["o"], "o", "open in GitHub"),
                refresh: b(&["r"], "r", "refresh"),
                refresh_all: b(&["R"], "R", "refresh all"),
                redraw: Binding::default(),
                page_down: b(&["ctrl+d"], "ctrl+d", "preview page down"),
                page_up: b(&["ctrl+u"], "ctrl+u", "preview page up"),
                next_section: b(
                    &["right", "l"],
                    &format!("{}/l", icons::ARROW_RIGHT),
                    "next section",
                ),
                prev_section: b(
                    &["left", "h"],
                    &format!("{}/h", icons::ARROW_LEFT),
                    "previous section",
                ),
                search: b(&["/"], "/", "search"),
                copy_number: b(&["y"], "y", "copy number"),
                section_mode: b(&["ctrl+s"], "ctrl+s", "section mode"),
                new_section: b(&["n"], "ctrl+s n", "new section"),
                remove_section: b(&["x"], "ctrl+s x", "remove section"),
                copy_url: b(&["Y"], "Y", "copy url"),
                help: b(&["?"], "?", "help"),
                quit: b(&["q", "ctrl+c"], "q", "quit"),
            },
            pr: PrKeys {
                prev_preview_tab: b(&["["], "[", "previous sidebar tab"),
                next_preview_tab: b(&["]"], "]", "next sidebar tab"),
                approve: b(&["v"], "v", "approve"),
                assign: b(&["a"], "a", "assign"),
                unassign: b(&["A"], "A", "unassign"),
                label: b(&["L"], "L", "label"),
                comment: b(&["c"], "c", "comment"),
                diff: b(&["D"], "D", "diff"),
                checkout: b(&["C", "space"], "C/space", "checkout"),
                close: b(&["x"], "x", "close"),
                summary_view_more: b(&["e"], "e", "expand description"),
                ready: b(&["W"], "W", "ready for review"),
                reopen: b(&["X"], "X", "reopen"),
                merge: b(&["m"], "m", "merge"),
                update: b(&["u"], "u", "update pr from base branch"),
                watch_checks: b(&["w"], "w", "watch checks"),
                approve_workflows: b(&["V"], "V", "approve all workflows"),
                toggle_smart_filtering: b(&["t"], "t", "toggle smart filtering"),
                view_issues: b(&["s"], "s", "switch to issues"),
            },
            issue: IssueKeys {
                label: b(&["L"], "L", "label"),
                assign: b(&["a"], "a", "assign"),
                unassign: b(&["A"], "A", "unassign"),
                comment: b(&["c"], "c", "comment"),
                checkout: b(&["C"], "C", "checkout"),
                close: b(&["x"], "x", "close"),
                reopen: b(&["X"], "X", "reopen"),
                toggle_smart_filtering: b(&["t"], "t", "toggle smart filtering"),
                view_prs: b(&["s"], "s", "switch to notifications"),
            },
            notif: NotificationKeys {
                mark_as_done: b(&["d"], "d", "mark as done"),
                mark_all_as_done: b(&["alt+d"], "alt+d", "mark all as done"),
                unsubscribe: b(&["u"], "u", "unsubscribe"),
                toggle_bookmark: b(&["b"], "b", "toggle bookmark"),
                open: b(&["o"], "o", "open in browser"),
                sort_by_repo: b(&["S"], "S", "sort by repo"),
                switch_to_prs: b(&["s"], "s", "switch to PRs"),
                toggle_smart_filtering: b(&["t"], "t", "toggle smart filtering"),
            },
            branch: BranchKeys {
                checkout: b(&["C", "space"], "C/space", "checkout"),
                new: b(&["n"], "n", "new"),
                create_pr: b(&["O"], "O", "create PR"),
                fast_forward: b(&["f"], "f", "fast-forward"),
                push: b(&["P"], "P", "push"),
                force_push: b(&["F"], "F", "force-push"),
                delete: b(&["d", "backspace"], "d/backspace", "delete"),
                update_pr: b(&["u"], "u", "update PR"),
                view_prs: b(&["s"], "s", "switch to PRs"),
            },
            custom_universal: vec![],
            custom_pr: vec![],
            custom_issue: vec![],
            custom_branch: vec![],
            custom_notification: vec![],
        }
    }
}

fn custom_binding(kb: &Keybinding) -> Binding {
    let name = if kb.name.is_empty() {
        truncate_command(&kb.command)
    } else {
        kb.name.clone()
    };
    Binding {
        keys: vec![kb.key.clone()],
        help_key: kb.key.clone(),
        help_desc: name,
    }
}

impl Keys {
    pub fn rebind(&mut self, kbs: &Keybindings) -> Result<()> {
        for kb in &kbs.universal {
            if kb.builtin.is_empty() {
                if !kb.command.is_empty() {
                    self.custom_universal.push(custom_binding(kb));
                }
                continue;
            }
            let u = &mut self.u;
            let target = match kb.builtin.as_str() {
                "up" => &mut u.up,
                "down" => &mut u.down,
                "firstLine" => &mut u.first_line,
                "lastLine" => &mut u.last_line,
                "togglePreview" => &mut u.toggle_preview,
                "togglePreviewPosition" => &mut u.toggle_preview_position,
                "openGithub" => &mut u.open_github,
                "refresh" => &mut u.refresh,
                "refreshAll" => &mut u.refresh_all,
                "redraw" => &mut u.redraw,
                "pageDown" => &mut u.page_down,
                "pageUp" => &mut u.page_up,
                "nextSection" => &mut u.next_section,
                "prevSection" => &mut u.prev_section,
                "search" => &mut u.search,
                "copyurl" => &mut u.copy_url,
                "copyNumber" => &mut u.copy_number,
                "newSection" => &mut u.new_section,
                "help" => &mut u.help,
                "quit" => &mut u.quit,
                other => bail!("unknown built-in universal key: '{other}'"),
            };
            target.rebind(kb, true);
        }
        for kb in &kbs.prs {
            if kb.builtin.is_empty() {
                if !kb.command.is_empty() {
                    self.custom_pr.push(custom_binding(kb));
                }
                continue;
            }
            let p = &mut self.pr;
            let target = match kb.builtin.as_str() {
                "prevSidebarTab" => &mut p.prev_preview_tab,
                "nextSidebarTab" => &mut p.next_preview_tab,
                "approve" => &mut p.approve,
                "assign" => &mut p.assign,
                "unassign" => &mut p.unassign,
                "label" => &mut p.label,
                "comment" => &mut p.comment,
                "diff" => &mut p.diff,
                "checkout" => &mut p.checkout,
                "close" => &mut p.close,
                "ready" => &mut p.ready,
                "reopen" => &mut p.reopen,
                "merge" => &mut p.merge,
                "update" => &mut p.update,
                "watchChecks" => &mut p.watch_checks,
                "approveWorkflows" => &mut p.approve_workflows,
                "viewIssues" => &mut p.view_issues,
                "summaryViewMore" => &mut p.summary_view_more,
                other => bail!("unknown built-in pr key: '{other}'"),
            };
            target.rebind(kb, false);
        }
        for kb in &kbs.branches {
            if kb.builtin.is_empty() {
                if !kb.command.is_empty() {
                    self.custom_branch.push(custom_binding(kb));
                }
                continue;
            }
            let br = &mut self.branch;
            let target = match kb.builtin.as_str() {
                "new" => &mut br.new,
                "createPr" => &mut br.create_pr,
                "delete" => &mut br.delete,
                "push" => &mut br.push,
                "forcePush" => &mut br.force_push,
                "fastForward" => &mut br.fast_forward,
                "checkout" => &mut br.checkout,
                "viewPRs" => &mut br.view_prs,
                "updatePr" => &mut br.update_pr,
                other => bail!("unknown built-in branch key: '{other}'"),
            };
            target.rebind(kb, false);
        }
        for kb in &kbs.issues {
            if kb.builtin.is_empty() {
                if !kb.command.is_empty() {
                    self.custom_issue.push(custom_binding(kb));
                }
                continue;
            }
            let i = &mut self.issue;
            let target = match kb.builtin.as_str() {
                "label" => &mut i.label,
                "assign" => &mut i.assign,
                "unassign" => &mut i.unassign,
                "comment" => &mut i.comment,
                "checkout" => &mut i.checkout,
                "close" => &mut i.close,
                "reopen" => &mut i.reopen,
                "viewPrs" => &mut i.view_prs,
                other => bail!("unknown built-in issue key: '{other}'"),
            };
            target.rebind(kb, false);
        }
        for kb in &kbs.notifications {
            if kb.builtin.is_empty() {
                if !kb.command.is_empty() {
                    self.custom_notification.push(custom_binding(kb));
                }
                continue;
            }
            let n = &mut self.notif;
            let target = match kb.builtin.as_str() {
                "markAsDone" => &mut n.mark_as_done,
                "markAllAsDone" => &mut n.mark_all_as_done,
                "unsubscribe" => &mut n.unsubscribe,
                "toggleBookmark" => &mut n.toggle_bookmark,
                "open" => &mut n.open,
                "sortByRepo" => &mut n.sort_by_repo,
                "switchToPRs" => &mut n.switch_to_prs,
                "toggleSmartFiltering" => &mut n.toggle_smart_filtering,
                other => bail!("unknown built-in notification key: '{other}'"),
            };
            target.rebind(kb, false);
        }
        Ok(())
    }
}

/// Converts a crossterm key event into bubbletea's key string form.
pub fn key_string(ev: &KeyEvent) -> String {
    let m = ev.modifiers;
    let base = match ev.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => {
            if m.contains(KeyModifiers::CONTROL) {
                return format!("ctrl+{}", c.to_ascii_lowercase());
            }
            if m.contains(KeyModifiers::ALT) {
                return format!("alt+{c}");
            }
            return c.to_string();
        }
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdown".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => return "shift+tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return String::new(),
    };
    let mut prefix = String::new();
    if m.contains(KeyModifiers::CONTROL) {
        prefix.push_str("ctrl+");
    }
    if m.contains(KeyModifiers::ALT) {
        prefix.push_str("alt+");
    }
    if m.contains(KeyModifiers::SHIFT) && base != "space" {
        prefix.push_str("shift+");
    }
    format!("{prefix}{base}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebinding_replaces_keys_and_keeps_desc() {
        let mut k = Keys::default();
        k.rebind(&Keybindings {
            universal: vec![Keybinding {
                key: "J".into(),
                builtin: "pageDown".into(),
                ..Default::default()
            }],
            prs: vec![Keybinding {
                key: "O".into(),
                command: "echo {{.PrNumber}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        assert!(k.u.page_down.matches("J"));
        assert!(!k.u.page_down.matches("ctrl+d"));
        assert_eq!(k.u.page_down.help_desc, "preview page down");
        assert_eq!(k.custom_pr[0].help_desc, "echo {{.PrNumber}}");
    }

    #[test]
    fn unknown_builtin_errors() {
        let mut k = Keys::default();
        let r = k.rebind(&Keybindings {
            prs: vec![Keybinding {
                key: "z".into(),
                builtin: "nope".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert!(r.is_err());
    }

    #[test]
    fn key_names_match_bubbletea() {
        use ratatui::crossterm::event::KeyEventKind;
        let ev = |code, mods| KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: ratatui::crossterm::event::KeyEventState::NONE,
        };
        assert_eq!(
            key_string(&ev(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            "ctrl+d"
        );
        assert_eq!(
            key_string(&ev(KeyCode::Char('J'), KeyModifiers::SHIFT)),
            "J"
        );
        assert_eq!(
            key_string(&ev(KeyCode::Char('d'), KeyModifiers::ALT)),
            "alt+d"
        );
        assert_eq!(
            key_string(&ev(KeyCode::Char(' '), KeyModifiers::NONE)),
            "space"
        );
        assert_eq!(key_string(&ev(KeyCode::Enter, KeyModifiers::NONE)), "enter");
    }
}
