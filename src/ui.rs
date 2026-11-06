//! Drawing: tabs, sections, preview, footer and popups.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, Position, TABS_HEIGHT, TaskState};
use crate::config::ViewType;
use crate::icons;
use crate::section::{RenderCtx, SPINNER};
use crate::text::*;
use crate::theme;
use crate::util::short_number;

/// ansi.TruncateLeft: drops `n` cells from the left, prefixing `prefix` when something was cut.
fn truncate_left(l: &Line<'static>, n: usize, prefix: Option<Span<'static>>) -> Line<'static> {
    if n == 0 {
        return l.clone();
    }
    let mut skipped = 0;
    let mut out: Vec<Span<'static>> = vec![];
    if let Some(p) = prefix {
        out.push(p);
    }
    for s in &l.spans {
        let mut buf = String::new();
        for ch in s.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if skipped < n {
                skipped += w;
                continue;
            }
            buf.push(ch);
        }
        if !buf.is_empty() {
            out.push(Span::styled(buf, s.style));
        }
    }
    Line::from(out)
}

/// The section tabs carousel.
fn carousel(items: &[String], cursor: usize, width: usize, app: &App) -> Line<'static> {
    let t = &app.theme;
    let item_style = Style::default().add_modifier(Modifier::DIM);
    let selected_style = Style::default()
        .add_modifier(Modifier::BOLD)
        .bg(t.selected_background)
        .fg(t.primary_text);
    let sep_style = Style::default().fg(t.secondary_border);
    let overflow_style = t.faint().add_modifier(Modifier::BOLD);
    let ellipsis = || span(icons::ELLIPSIS, item_style);
    let n = items.len();
    if n == 0 {
        return Line::from(raw(" ".repeat(width)));
    }
    let render_item = |i: usize, max_w: i64| -> Line<'static> {
        let sep = if i != n - 1 {
            Some(span("|", sep_style))
        } else {
            None
        };
        if i == cursor {
            let mut l = vec![span(format!("  {}  ", items[i]), selected_style)];
            l.extend(sep);
            return Line::from(l);
        }
        let r = Line::from(span(format!("  {}  ", items[i]), item_style));
        let wants = line_width(&r) as i64 + sep.as_ref().map(|_| 1).unwrap_or(0);
        let item = if i < cursor {
            let remove = 1 + wants - max_w;
            if remove > 0 {
                truncate_left(&r, remove as usize, Some(ellipsis()))
            } else {
                r
            }
        } else {
            truncate_line(&r, max_w.max(0) as usize, icons::ELLIPSIS)
        };
        let mut spans = item.spans;
        spans.extend(sep);
        Line::from(spans)
    };

    let mut leftover = width as i64;
    let mut content: Vec<Span<'static>> = vec![];
    let mut dir: i64 = -1;
    let mut left = cursor as i64;
    let mut last_left = left;
    let mut right = (cursor + 1).min(n) as i64;
    let mut last_right = right;
    while leftover > 0 && (left >= 0 || right < n as i64) {
        if dir < 0 && left >= 0 {
            let it = render_item(left as usize, leftover);
            leftover -= line_width(&it) as i64;
            let mut s = it.spans;
            s.extend(content);
            content = s;
            last_left = left;
            left -= 1;
        } else if dir > 0 && right < n as i64 {
            let it = render_item(right as usize, leftover);
            leftover -= line_width(&it) as i64;
            content.extend(it.spans);
            last_right = right;
            right += 1;
        }
        if left < 0 {
            dir = 1;
        } else if right > n as i64 - 1 {
            dir = -1;
        } else {
            dir = -dir;
        }
    }
    let last_right = last_right.min(n as i64 - 1);
    let mut leftover = width as i64;
    let lo = (last_left != 0).then(|| span(" ← ", overflow_style));
    let ro = (last_right != n as i64 - 1).then(|| span(" → ", overflow_style));
    if lo.is_some() {
        leftover -= 3;
    }
    if ro.is_some() {
        leftover -= 3;
    }
    let mut items_line = Line::from(content);
    if lo.is_some() {
        let cut = line_width(&items_line) as i64 - leftover + 1;
        if cut > 0 {
            items_line = truncate_left(&items_line, cut as usize, Some(ellipsis()));
        }
    } else if line_width(&items_line) as i64 > leftover {
        let mut l = truncate_line(&items_line, leftover.max(0) as usize, "");
        l.spans.push(ellipsis());
        items_line = l;
    }
    let mut spans = vec![];
    spans.extend(lo);
    spans.extend(items_line.spans);
    spans.extend(ro);
    pad_line(Line::from(spans), width, Style::default())
}

fn tabs_line(app: &App) -> Line<'static> {
    let t = &app.theme;
    let logo = Line::from(vec![
        raw("  "),
        span(
            " GHTUI ",
            Style::default()
                .fg(theme::LOGO)
                .bg(theme::LOGO_BG)
                .add_modifier(Modifier::BOLD),
        ),
        raw(" "),
        span(app.version.clone(), t.secondary()),
    ]);
    let mut logo = logo;
    if !app.latest_version.is_empty() && app.version != app.latest_version {
        logo.spans.push(span(
            format!(" {} Update available!", icons::UPDATE_AVAILABLE),
            Style::default().fg(theme::SUCCESS_GREEN),
        ));
    }
    logo.spans.push(raw(" "));
    let button = Line::from(vec![
        raw(" "),
        span(
            format!("{} ", icons::NEW_SECTION),
            Style::default().fg(theme::SUCCESS_GREEN),
        ),
        raw(app.keys.u.new_section.help_key.clone()),
        raw(" "),
        span("│", Style::default().fg(t.secondary_border)),
    ]);
    let cw = app
        .width
        .saturating_sub(line_width(&logo) + line_width(&button));
    let titles: Vec<String> = app
        .sections()
        .iter()
        .map(|s| {
            if s.is_loading {
                format!(
                    "{} {}",
                    s.title,
                    format!("  {}", SPINNER[app.spinner % SPINNER.len()]).trim_end()
                )
            } else if app.cfg.theme.ui.sections_show_count {
                format!("{} ({})", s.title, short_number(s.total_count))
            } else {
                s.title.clone()
            }
        })
        .collect();
    // The carousel can overflow its width by a cell; the screen edge clips the logo instead.
    let car = carousel(&titles, app.curr, cw, app);
    fit_line(
        &concat(vec![car, button, logo]),
        app.width,
        Style::default(),
    )
}

fn view_button(app: &App, view: ViewType) -> Vec<Span<'static>> {
    let t = &app.theme;
    let active = app.view == view;
    let (icon, label) = match view {
        ViewType::Notifications => (
            if active {
                icons::BELL_SOLID
            } else {
                icons::BELL_OUTLINE
            },
            "",
        ),
        ViewType::Prs => (icons::OPEN, " PRs"),
        _ => (icons::ISSUE_OPEN, " Issues"),
    };
    if active {
        let color = if view == ViewType::Notifications {
            theme::GOLD
        } else {
            t.success_text
        };
        let st = Style::default()
            .fg(color)
            .bg(t.faint_border)
            .add_modifier(Modifier::BOLD);
        vec![span(icon, st), span(label, st)]
    } else {
        vec![span(
            format!("{icon}{label}"),
            Style::default()
                .bg(t.faint_border)
                .fg(t.faint_text)
                .add_modifier(Modifier::BOLD),
        )]
    }
}

fn view_switcher(app: &App) -> Line<'static> {
    let t = &app.theme;
    let sep = Style::default()
        .bg(t.faint_border)
        .fg(t.primary_border)
        .add_modifier(Modifier::BOLD);
    let footer = Style::default()
        .bg(t.selected_background)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![span(" ", sep)];
    spans.extend(view_button(app, ViewType::Notifications));
    spans.push(span(" │ ", sep));
    spans.extend(view_button(app, ViewType::Prs));
    spans.push(span(" │ ", sep));
    spans.extend(view_button(app, ViewType::Issues));
    spans.push(span(
        format!("{} ", icons::POWERLINE_RIGHT),
        footer.fg(t.faint_border),
    ));
    if let Some(p) = &app.repo_path {
        // footer.renderViewSwitcher: the origin's short name when known.
        let name = if !app.repo_url.is_empty() {
            crate::git::repo_short_name(&app.repo_url)
        } else {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        spans.push(span(
            format!("{} {name}", icons::REPO),
            footer.fg(t.inverted_text),
        ));
    }
    spans.push(span(" • ", footer.fg(t.faint_text)));
    if !app.user.is_empty() {
        spans.push(span(format!("@{}", app.user), footer.fg(t.inverted_text)));
    }
    spans.push(span(" │", footer.fg(t.faint_border)));
    Line::from(spans)
}

fn running_task(app: &App) -> Option<Line<'static>> {
    let t = &app.theme;
    let mut tasks: Vec<_> = app.tasks.values().collect();
    if tasks.is_empty() {
        return None;
    }
    tasks.sort_by(|a, b| match (a.finished, b.finished) {
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => y.cmp(&x),
        (None, None) => b.started.cmp(&a.started),
    });
    let task = tasks[0];
    let bg = Style::default().bg(t.selected_background);
    let processing = app
        .tasks
        .values()
        .filter(|t| t.state == TaskState::Running)
        .count();
    let mut spans = vec![span(" ", bg)];
    if processing > 1 {
        spans.push(span(
            format!("[{} {processing}] ", icons::TASKS_RUNNING),
            bg.fg(t.faint_text),
        ));
    }
    match task.state {
        TaskState::Running => {
            spans.push(span(SPINNER[app.spinner % SPINNER.len()], bg));
            spans.push(span(task.start_text.clone(), bg));
        }
        TaskState::Failed => spans.push(span(
            format!(
                "{} {}",
                icons::FAILURE,
                task.error.clone().unwrap_or_default()
            ),
            bg.fg(t.error_text),
        )),
        TaskState::Done => spans.push(span(
            format!("{} {}", icons::SUCCESS, task.finished_text),
            bg.fg(t.success_text),
        )),
    }
    spans.push(span(" ", bg));
    Some(Line::from(spans))
}

fn footer_line(app: &App) -> Line<'static> {
    let t = &app.theme;
    if let Some(e) = &app.error {
        let first = e.lines().next().unwrap_or_default().to_string();
        return Line::from(vec![span(
            format!("{} {first}", icons::FAILURE),
            Style::default().fg(t.error_text),
        )]);
    }
    if app.confirm_quit_shown {
        return Line::from("Really quit? (Press y/enter to confirm, any other key to cancel)");
    }
    let pager_style = Style::default().bg(t.selected_background).fg(t.faint_text);
    let left_text = if let Some(p) = &app.notif_prompt {
        Some(p.clone())
    } else if let Some(s) = app.section() {
        match s.prompt_text() {
            Some(p) => {
                let input = s
                    .prompt
                    .as_ref()
                    .map(|(_, i)| i.value())
                    .unwrap_or_default();
                Some(format!("{p}{input}"))
            }
            None => {
                let c = s.pager_content();
                Some(c)
            }
        }
    } else {
        None
    };
    let branch_pager = app
        .section()
        .filter(|s| {
            s.kind == crate::section::Kind::Branch
                && s.prompt.is_none()
                && app.notif_prompt.is_none()
        })
        .map(|s| crate::section::branch_pager(s, t));
    let left = match left_text {
        _ if branch_pager.is_some() => branch_pager.unwrap(),
        Some(c) if !c.is_empty() => Line::from(vec![
            span(" ", pager_style),
            span(c, pager_style),
            span(" ", pager_style),
        ]),
        _ => Line::from(span("  ", pager_style)),
    };
    let help = Line::from(span(
        " ? help ",
        Style::default().bg(t.faint_text).fg(t.selected_background),
    ));
    let switcher = view_switcher(app);
    let right = running_task(app).unwrap_or_default();
    let used = line_width(&switcher) + line_width(&left) + line_width(&right) + line_width(&help);
    let spacing = Line::from(span(
        " ".repeat(app.width.saturating_sub(used)),
        Style::default().bg(t.selected_background),
    ));
    fit_line(
        &concat(vec![switcher, left, spacing, right, help]),
        app.width,
        Style::default().bg(t.selected_background),
    )
}

/// bubbles/help FullHelpView: columns of keys and descriptions separated by 4 spaces.
pub fn help_lines(app: &App) -> Block {
    let t = &app.theme;
    let groups = app.keys.full_help(app.view, app.notification_subject());
    let mut columns: Vec<Block> = vec![];
    let mut total = 0;
    for g in groups {
        let g: Vec<_> = g.into_iter().filter(|b| !b.keys.is_empty()).collect();
        if g.is_empty() {
            continue;
        }
        let kw = g.iter().map(|b| str_width(&b.help_key)).max().unwrap_or(0);
        let col: Block = g
            .iter()
            .map(|b| {
                Line::from(vec![
                    span(
                        format!("{}{}", b.help_key, " ".repeat(kw - str_width(&b.help_key))),
                        t.primary(),
                    ),
                    span(" ", t.primary()),
                    span(b.help_desc.clone(), t.faint()),
                ])
            })
            .collect();
        let w = block_width(&col) + if columns.is_empty() { 0 } else { 4 };
        if total + w > app.width {
            break;
        }
        total += w;
        if !columns.is_empty() {
            columns.push(vec![Line::from(span("    ", t.secondary()))]);
        }
        columns.push(col);
    }
    hjoin(columns)
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let buf = f.buffer_mut();
    let t = &app.theme;
    let w = area.width as usize;

    // The repo view has no tabs, leaving one empty line.
    let repo_view = app.view == crate::config::ViewType::Repo;
    if !repo_view {
        draw_block(buf, Rect::new(0, 0, area.width, 1), &[tabs_line(app)]);
        draw_block(
            buf,
            Rect::new(0, 1, area.width, 1),
            &[Line::from(span(
                "━".repeat(w),
                Style::default().fg(t.primary_border),
            ))],
        );
    }

    let l = app.layout();
    let top = if repo_view { 1 } else { TABS_HEIGHT as u16 };
    // lipgloss never shrinks the search box and table header, so a tall help
    // footer pushes everything down and gets cut at the bottom of the screen.
    let mut sec_h = l.main_h;
    match app.section() {
        Some(s) => {
            let rc = RenderCtx {
                cfg: &app.cfg,
                theme: t,
                spinner: app.spinner,
                width: l.main_w,
                height: l.main_h,
            };
            let block = s.view(&rc);
            sec_h = sec_h.max(block.len());
            draw_block(
                buf,
                Rect::new(0, top, l.main_w as u16, sec_h as u16),
                &block,
            );
        }
        None => {
            let msg = align_line(
                Line::from("No sections defined"),
                l.main_w,
                Align::Center,
                Style::default(),
            );
            draw_block(
                buf,
                Rect::new(0, top + (l.main_h / 2) as u16, l.main_w as u16, 1),
                &[msg],
            );
        }
    }

    if app.sidebar_open {
        let content = app
            .sidebar_cache
            .as_ref()
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        let vh = app.sidebar_viewport_height();
        let scroll = app.sidebar_scroll.min(content.len().saturating_sub(vh));
        let visible: Block = content.iter().skip(scroll).take(vh).cloned().collect();
        let percent = if content.len() <= vh {
            100
        } else {
            (scroll as f64 / (content.len() - vh) as f64 * 100.0) as usize
        };
        let pager = Line::from(span(
            format!("{percent}%"),
            t.faint().add_modifier(Modifier::BOLD),
        ));
        let empty = content.is_empty();
        match l.position {
            Position::Bottom => {
                let y = top + sec_h as u16;
                draw_block(
                    buf,
                    Rect::new(0, y, area.width, 1),
                    &[Line::from(span(
                        "━".repeat(w),
                        Style::default().fg(t.primary_border),
                    ))],
                );
                let rect = Rect::new(0, y + 1, l.preview_w as u16, vh as u16);
                if empty {
                    let msg = align_line(
                        Line::from("Nothing selected..."),
                        l.preview_w,
                        Align::Center,
                        Style::default(),
                    );
                    draw_block(
                        buf,
                        Rect::new(0, y + 1 + (l.preview_h / 2) as u16, l.preview_w as u16, 1),
                        &[msg],
                    );
                } else {
                    draw_block(buf, rect, &visible);
                    draw_block(
                        buf,
                        Rect::new(0, y + 1 + vh as u16, l.preview_w as u16, 1),
                        &[pager],
                    );
                }
            }
            Position::Right => {
                let x = l.main_w as u16;
                let border = Style::default().fg(t.primary_border);
                for i in 0..l.main_h {
                    draw_block(
                        buf,
                        Rect::new(x, top + i as u16, 1, 1),
                        &[Line::from(span("│", border))],
                    );
                }
                let cw = l.preview_w.saturating_sub(1) as u16;
                if empty {
                    let msg = align_line(
                        Line::from("Nothing selected..."),
                        cw as usize,
                        Align::Center,
                        Style::default(),
                    );
                    draw_block(
                        buf,
                        Rect::new(x + 1, top + (l.main_h / 2) as u16, cw, 1),
                        &[msg],
                    );
                } else {
                    draw_block(buf, Rect::new(x + 1, top, cw, vh as u16), &visible);
                    draw_block(buf, Rect::new(x + 1, top + vh as u16, cw, 1), &[pager]);
                }
            }
        }
    }

    // Autocomplete popups are layers over the content, like lipgloss.NewLayer in ui.View.
    if let Some(c) = app.section().and_then(|s| s.search_cmp.as_ref()) {
        let popup = c.view(t, l.main_w.saturating_sub(4), app.spinner, false);
        draw_block(
            buf,
            // The popup sits at a fixed row, one row low in the repo view.
            Rect::new(
                1,
                TABS_HEIGHT as u16 + 3,
                l.main_w.saturating_sub(2) as u16,
                popup.len() as u16,
            ),
            &popup,
        );
    }
    let editor = app.pr_editor.as_ref().or(app.issue_editor.as_ref());
    if let (true, Some(ed)) = (app.sidebar_open, editor)
        && let Some(c) = ed.cmp.as_ref().filter(|c| c.visible)
    {
        // y = ScreenHeight - FooterHeight - LineFromBottom - InputBoxHeight - 6, x = preview x + 3.
        let text = ed.area.value();
        let lines = text.split('\n').count();
        let (_, cur) = crate::input::Editable::cursor_pos(&ed.area);
        const TEXTAREA_HEIGHT: usize = 5;
        let from_bottom = if lines < TEXTAREA_HEIGHT {
            TEXTAREA_HEIGHT - lines
        } else {
            lines - cur - 1
        };
        let py = app.height.saturating_sub(1 + from_bottom + 8 + 6);
        let px = match l.position {
            Position::Bottom => 3,
            Position::Right => l.main_w + 3,
        };
        let inner = app.sidebar_content_width().saturating_sub(6);
        let popup = c.view(t, inner, app.spinner, true);
        draw_block(
            buf,
            Rect::new(px as u16, py as u16, (inner + 2) as u16, popup.len() as u16),
            &popup,
        );
    }

    let help = if app.show_help {
        help_lines(app)
    } else {
        vec![]
    };
    let stacked = top as usize
        + sec_h
        + if app.sidebar_open && l.position == Position::Bottom {
            1 + l.preview_h
        } else {
            0
        };
    let footer_y = (stacked as u16).max(area.height.saturating_sub(1 + help.len() as u16));
    draw_block(
        buf,
        Rect::new(0, footer_y, area.width, 1),
        &[footer_line(app)],
    );
    if !help.is_empty() {
        draw_block(
            buf,
            Rect::new(0, footer_y + 1, area.width, help.len() as u16),
            &help,
        );
    }
}
