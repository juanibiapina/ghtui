//! Drawing: tabs, sections, preview, footer and popups.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, Position, TaskState};
use crate::config::ViewType;
use crate::icons;
use crate::mouse::{HitMap, Target};
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

/// What each cell of a line belongs to.
type Owners = Vec<Option<Target>>;

fn fit_owners(mut owners: Owners, width: usize) -> Owners {
    owners.resize(width, None);
    owners
}

/// Records each run of cells with the same owner, on the row `area`, starting at its left edge.
fn record_owners(hits: &mut HitMap, area: Rect, owners: &[Option<Target>]) {
    let mut start = 0;
    while start < owners.len() {
        let end = owners[start..]
            .iter()
            .position(|o| *o != owners[start])
            .map_or(owners.len(), |n| start + n);
        if let Some(t) = owners[start] {
            let r = Rect::new(area.x + start as u16, area.y, (end - start) as u16, 1);
            hits.push(r.intersection(area), t);
        }
        start = end;
    }
}

/// The section tabs carousel, and the section each cell selects.
fn carousel(items: &[String], cursor: usize, width: usize, app: &App) -> (Line<'static>, Owners) {
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
        return (Line::from(raw(" ".repeat(width))), vec![None; width]);
    }
    let item_owners = |i: usize, l: &Line<'static>| -> Owners {
        let mut o = vec![Some(Target::SectionTab(i)); l.width()];
        if i != n - 1
            && let Some(last) = o.last_mut()
        {
            *last = None;
        }
        o
    };
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
        let wants = r.width() as i64 + sep.as_ref().map(|_| 1).unwrap_or(0);
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
    let mut owners: Owners = vec![];
    let mut dir: i64 = -1;
    let mut left = cursor as i64;
    let mut last_left = left;
    let mut right = (cursor + 1).min(n) as i64;
    let mut last_right = right;
    while leftover > 0 && (left >= 0 || right < n as i64) {
        if dir < 0 && left >= 0 {
            let it = render_item(left as usize, leftover);
            leftover -= it.width() as i64;
            let mut o = item_owners(left as usize, &it);
            o.extend(owners);
            owners = o;
            let mut s = it.spans;
            s.extend(content);
            content = s;
            last_left = left;
            left -= 1;
        } else if dir > 0 && right < n as i64 {
            let it = render_item(right as usize, leftover);
            leftover -= it.width() as i64;
            owners.extend(item_owners(right as usize, &it));
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
        let cut = items_line.width() as i64 - leftover + 1;
        if cut > 0 {
            items_line = truncate_left(&items_line, cut as usize, Some(ellipsis()));
            let mut o = vec![None];
            o.extend(owners.into_iter().skip(cut as usize));
            owners = fit_owners(o, items_line.width());
        }
    } else if items_line.width() as i64 > leftover {
        let mut l = truncate_line(&items_line, leftover.max(0) as usize, "");
        owners.truncate(l.width());
        l.spans.push(ellipsis());
        items_line = l;
        owners = fit_owners(owners, items_line.width());
    }
    let mut cells: Owners = vec![];
    let mut spans = vec![];
    if let Some(s) = lo {
        cells.extend(vec![Some(Target::PrevSectionArrow); s.width()]);
        spans.push(s);
    }
    cells.extend(fit_owners(owners, items_line.width()));
    spans.extend(items_line.spans);
    if let Some(s) = ro {
        cells.extend(vec![Some(Target::NextSectionArrow); s.width()]);
        spans.push(s);
    }
    (
        pad_line(Line::from(spans), width, Style::default()),
        fit_owners(cells, width),
    )
}

fn tabs_line(app: &App) -> (Line<'static>, Owners) {
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
    let cw = app.width.saturating_sub(logo.width() + button.width());
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
    let (car, mut owners) = carousel(&titles, app.curr, cw, app);
    owners = fit_owners(owners, car.width());
    owners.extend(vec![Some(Target::NewSection); button.width()]);
    let line = Line::from_iter([car, button, logo].into_iter().flat_map(|l| l.spans));
    (line, owners)
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

fn view_switcher(app: &App) -> (Line<'static>, Owners) {
    let t = &app.theme;
    let sep = Style::default()
        .bg(t.faint_border)
        .fg(t.primary_border)
        .add_modifier(Modifier::BOLD);
    let footer = Style::default()
        .bg(t.selected_background)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![span(" ", sep)];
    let mut owners: Owners = vec![None];
    for (i, view) in [ViewType::Notifications, ViewType::Prs, ViewType::Issues]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            spans.push(span(" │ ", sep));
            owners.extend([None; 3]);
        }
        let button = view_button(app, view);
        let w: usize = button.iter().map(|s| s.width()).sum();
        owners.extend(vec![Some(Target::View(view)); w]);
        spans.extend(button);
    }
    spans.push(span(
        format!("{} ", icons::POWERLINE_RIGHT),
        footer.fg(t.faint_border),
    ));
    if let Some(p) = &app.repo_path {
        // The origin's short name when known.
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
    (Line::from(spans), owners)
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

fn render_footer(app: &App, area: Rect, buf: &mut Buffer, hits: &mut HitMap) {
    let t = &app.theme;
    if let Some(e) = &app.error {
        let first = e.lines().next().unwrap_or_default().to_string();
        return span(
            format!("{} {first}", icons::FAILURE),
            Style::default().fg(t.error_text),
        )
        .render(area, buf);
    }
    if app.confirm_quit_shown {
        return Line::from("Really quit? (Press y/enter to confirm, any other key to cancel)")
            .render(area, buf);
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
    let (switcher, owners) = view_switcher(app);
    let right = running_task(app).unwrap_or_default();
    let left = Line::from_iter(switcher.spans.into_iter().chain(left.spans));
    let help_w = help.width();
    let right = Line::from_iter(right.spans.into_iter().chain(help.spans));
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right.width() as u16),
    ])
    .areas(area);
    buf.set_style(left_area, Style::default().bg(t.selected_background));
    left.render(left_area, buf);
    right.render(right_area, buf);
    record_owners(hits, left_area, &owners);
    let help_x = right_area.right().saturating_sub(help_w as u16);
    hits.push(
        Rect {
            x: help_x,
            width: help_w as u16,
            ..right_area
        }
        .intersection(right_area),
        Target::Help,
    );
}

fn middle_row(area: Rect) -> Rect {
    Rect {
        y: area.y + area.height / 2,
        height: area.height.min(1),
        ..area
    }
}

/// Draws the screen and returns the regions the mouse can act on.
pub fn draw(f: &mut Frame, app: &App) -> HitMap {
    let mut hits = HitMap::default();
    let area = f.area();
    let buf = f.buffer_mut();
    let t = &app.theme;
    let areas = app.areas();
    let border = Style::default().fg(t.primary_border);
    let thick_rule = Block::new()
        .borders(Borders::TOP)
        .border_type(BorderType::Thick)
        .border_style(border);

    if app.view != ViewType::Repo {
        let [tabs, rule] = Layout::vertical([Constraint::Length(1); 2]).areas(areas.tabs);
        let (line, owners) = tabs_line(app);
        line.render(tabs, buf);
        hits.push(tabs, Target::TabsRow);
        record_owners(&mut hits, tabs, &owners);
        thick_rule.clone().render(rule, buf);
    }

    match app.section() {
        Some(s) => {
            let rc = RenderCtx {
                cfg: &app.cfg,
                theme: t,
                spinner: app.spinner,
            };
            s.render(&rc, areas.sections, buf, &mut hits);
        }
        None => Line::from("No sections defined")
            .centered()
            .render(middle_row(areas.sections), buf),
    }

    if let Some(p) = areas.preview {
        let content = app
            .preview_cache
            .as_ref()
            .map(|(_, b)| b.as_slice())
            .unwrap_or_default();
        let vh = p.content.height as usize;
        let scroll = app.preview_scroll.min(content.len().saturating_sub(vh));
        hits.push(p.body(), Target::Preview);
        if let Some(line) = app
            .preview_tabs_line
            .filter(|l| (scroll..scroll + vh).contains(l))
        {
            let y = p.content.y + (line - scroll) as u16;
            for (i, (x, w)) in crate::views::pr_tab_ranges(p.content.width as usize)
                .into_iter()
                .enumerate()
            {
                let r = Rect::new(p.content.x + x as u16, y, w as u16, 1);
                hits.push(r.intersection(p.content), Target::PreviewTab(i));
            }
        }
        match areas.position {
            Position::Bottom => thick_rule.render(p.rule, buf),
            Position::Right => Block::new()
                .borders(Borders::LEFT)
                .border_style(border)
                .render(p.rule, buf),
        }
        if content.is_empty() {
            Line::from("Nothing selected...")
                .centered()
                .render(middle_row(p.body()), buf);
        } else {
            let percent = if content.len() <= vh {
                100
            } else {
                (scroll as f64 / (content.len() - vh) as f64 * 100.0) as usize
            };
            Text::from(
                content[scroll..]
                    .iter()
                    .take(vh)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .render(p.content, buf);
            Line::from(span(
                format!("{percent}%"),
                t.faint().add_modifier(Modifier::BOLD),
            ))
            .render(p.pager, buf);
        }
    }

    // Autocomplete popups are layers over the content.
    let sections = areas.sections;
    if let Some(c) = app.section().and_then(|s| s.search_cmp.as_ref()) {
        let below_search = Rect::new(
            sections.x + 1,
            sections.y + 2,
            sections.width.saturating_sub(2),
            sections.height.saturating_sub(2),
        );
        c.render(t, app.spinner, below_search, buf);
        c.record_hits(below_search, &mut hits);
    }
    let editor = app.pr_editor.as_ref().or(app.issue_editor.as_ref());
    if let (Some(p), Some(ed)) = (areas.preview, editor)
        && let Some(c) = ed.cmp.as_ref().filter(|c| c.visible)
    {
        let text = ed.area.value();
        let lines = text.split('\n').count();
        let (_, cur) = crate::input::Editable::cursor_pos(&ed.area);
        const TEXTAREA_HEIGHT: usize = 5;
        let from_bottom = if lines < TEXTAREA_HEIGHT {
            TEXTAREA_HEIGHT - lines
        } else {
            lines - cur - 1
        };
        let py = app.height.saturating_sub(1 + from_bottom + 8 + 6) as u16;
        let width = p.content.width.saturating_sub(4);
        let rect = Rect::new(p.outer().x + 3, py, width, c.height()).intersection(area);
        Clear.render(rect, buf);
        c.render(t, app.spinner, rect, buf);
        c.record_hits(rect, &mut hits);
    }

    render_footer(app, areas.footer, buf, &mut hits);
    if let Some(help) = &app.help {
        help.render(area, buf, t);
        hits.push(area, Target::Backdrop);
        hits.push(help.popup(area), Target::HelpPopup);
    }
    hits
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::*;
    use crate::complete::{Completer, FetchState, SourceKind, Suggestion};
    use crate::config::Config;
    use crate::input::{TextArea, TextInput};
    use crate::keys::Keys;
    use crate::model::{Count, Label, Login, Nodes, Notification, PullRequest, Repository};
    use crate::section::{NotifRow, PrRow};
    use crate::views::{Editor, EditorMode};

    fn pr(number: i64, title: &str, author: &str, days: i64) -> PrRow {
        let repo = Repository {
            name: "ghtui".into(),
            name_with_owner: "juanibiapina/ghtui".into(),
            ..Default::default()
        };
        PrRow {
            primary: PullRequest {
                number,
                title: title.into(),
                author: Login {
                    login: author.into(),
                },
                updated_at: Utc::now() - Duration::days(days),
                created_at: Utc::now() - Duration::days(days + 2),
                url: format!("https://github.com/juanibiapina/ghtui/pull/{number}"),
                state: "OPEN".into(),
                review_decision: "REVIEW_REQUIRED".into(),
                additions: 120 * number,
                deletions: 7 * number,
                head_ref_name: format!("feature-{number}"),
                base_ref_name: "main".into(),
                repository: repo,
                comments: Count { total_count: 3 },
                labels: Nodes {
                    nodes: vec![Label {
                        color: "a2eeef".into(),
                        name: "enhancement".into(),
                        description: String::new(),
                    }],
                    total_count: 1,
                },
                ..Default::default()
            },
            details: None,
        }
    }

    fn app(width: u16, height: u16, cfg: Config) -> App {
        let (pool, _rx) = crate::msg::Pool::new(1);
        let mut app = App::new(
            cfg,
            Keys::default(),
            pool,
            None,
            None,
            width as usize,
            height as usize,
        );
        app.version = "v0.0.0".into();
        app.user = "juan".into();
        app
    }

    fn with_prs(mut app: App) -> App {
        let rows = vec![
            pr(1, "Render the dashboard with ratatui widgets", "juan", 1),
            pr(
                2,
                "A pull request with a title long enough to wrap onto a second line in the table",
                "octocat",
                3,
            ),
            pr(3, "Fix the footer", "dependabot", 9),
        ];
        let s = &mut app.prs[0];
        s.total_count = rows.len();
        s.prs = rows;
        s.last_fetch_at = Some(std::time::Instant::now());
        app
    }

    fn screen(mut app: App) -> String {
        let (w, h) = (app.width as u16, app.height as u16);
        app.ensure_preview_cache();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                draw(f, &app);
            })
            .unwrap();
        format!("{:?}", terminal.backend().buffer())
    }

    fn frame(app: &mut App) -> (Buffer, HitMap) {
        let (w, h) = (app.width as u16, app.height as u16);
        app.ensure_preview_cache();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut hits = HitMap::default();
        terminal.draw(|f| hits = draw(f, app)).unwrap();
        (terminal.backend().buffer().clone(), hits)
    }

    /// The first cell of the first row, from the top, that contains `text`.
    fn find(buf: &Buffer, text: &str) -> (u16, u16) {
        for y in 0..buf.area.height {
            let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            for x in 0..cells.len() {
                if cells[x..].concat().starts_with(text) {
                    return (x as u16, y);
                }
            }
        }
        panic!("{text:?} is not on screen");
    }

    fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> bool {
        let (_, hits) = frame(app);
        let ev = MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(&ev, &hits)
    }

    fn click(app: &mut App, at: (u16, u16)) -> bool {
        mouse(app, MouseEventKind::Down(MouseButton::Left), at)
    }

    fn right_preview(width: u16, height: u16) -> App {
        let mut cfg = Config::default();
        cfg.defaults.preview.position = "right".into();
        with_prs(app(width, height, cfg))
    }

    #[test]
    fn clicking_a_row_selects_it() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        assert!(click(&mut app, find(&buf, "Fix the footer")));
        assert_eq!(app.section().unwrap().cursor, 2);
    }

    #[test]
    fn the_wheel_moves_the_selection_over_the_list() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        let at = find(&buf, "Fix the footer");
        assert!(mouse(&mut app, MouseEventKind::ScrollDown, at));
        assert_eq!(app.section().unwrap().cursor, 1);
        assert!(mouse(&mut app, MouseEventKind::ScrollUp, at));
        assert!(!mouse(&mut app, MouseEventKind::ScrollUp, at));
        assert_eq!(app.section().unwrap().cursor, 0);
    }

    #[test]
    fn the_wheel_scrolls_the_preview_under_the_pointer() {
        let mut app = right_preview(160, 12);
        let (buf, _) = frame(&mut app);
        let at = find(&buf, "· #1");
        assert!(mouse(&mut app, MouseEventKind::ScrollDown, at));
        app.ensure_preview_cache();
        assert!(app.preview_scroll > 0);
        assert_eq!(app.section().unwrap().cursor, 0);
    }

    #[test]
    fn clicking_a_section_tab_switches_to_it() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        let title = app.sections()[1].title.clone();
        assert!(click(&mut app, find(&buf, &title)));
        assert_eq!(app.curr, 1);
    }

    #[test]
    fn clicking_a_preview_tab_shows_it() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        assert!(click(&mut app, find(&buf, "Activity")));
        assert_eq!(app.pr_tab, 1);
    }

    #[test]
    fn clicking_outside_help_closes_it() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        assert!(click(&mut app, find(&buf, "? help")));
        assert!(app.help.is_some());
        let (buf, _) = frame(&mut app);
        assert!(!click(&mut app, find(&buf, "Keybindings")));
        assert!(app.help.is_some());
        assert!(click(&mut app, (0, 0)));
        assert!(app.help.is_none());
    }

    #[test]
    fn clicks_do_nothing_while_a_confirmation_is_open() {
        let mut app = right_preview(160, 30);
        app.confirm_quit_shown = true;
        let (buf, _) = frame(&mut app);
        assert!(!click(&mut app, find(&buf, "Fix the footer")));
        assert_eq!(app.section().unwrap().cursor, 0);
    }

    #[test]
    fn pointer_motion_changes_nothing() {
        let mut app = right_preview(160, 30);
        let (buf, _) = frame(&mut app);
        let at = find(&buf, "Fix the footer");
        assert!(!mouse(&mut app, MouseEventKind::Moved, at));
    }

    #[test]
    fn prs_with_right_preview() {
        let mut cfg = Config::default();
        cfg.defaults.preview.position = "right".into();
        insta::assert_snapshot!(screen(with_prs(app(160, 30, cfg))));
    }

    #[test]
    fn prs_with_bottom_preview() {
        let mut cfg = Config::default();
        cfg.defaults.preview.position = "bottom".into();
        insta::assert_snapshot!(screen(with_prs(app(120, 40, cfg))));
    }

    #[test]
    fn prs_on_a_narrow_terminal() {
        insta::assert_snapshot!(screen(with_prs(app(70, 30, Config::default()))));
    }

    #[test]
    fn prs_without_preview() {
        let mut app = with_prs(app(100, 20, Config::default()));
        app.preview_open = false;
        insta::assert_snapshot!(screen(app));
    }

    #[test]
    fn section_loading() {
        let mut app = app(100, 20, Config::default());
        app.prs[0].is_loading = true;
        insta::assert_snapshot!(screen(app));
    }

    #[test]
    fn section_with_no_results() {
        let mut app = app(100, 20, Config::default());
        app.prs[0].last_fetch_at = Some(std::time::Instant::now());
        insta::assert_snapshot!(screen(app));
    }

    #[test]
    fn search_with_completions() {
        let mut cfg = Config::default();
        cfg.defaults.preview.position = "right".into();
        let mut app = with_prs(app(160, 30, cfg));
        let s = &mut app.prs[0];
        s.search = Some(TextInput::new("is:open auth"));
        let mut c = Completer::new(SourceKind::Search, None, false);
        c.filtered = vec![
            Suggestion {
                value: "author:".into(),
                detail: "PRs by a user".into(),
            },
            Suggestion {
                value: "assignee:".into(),
                detail: "PRs assigned to a user".into(),
            },
        ];
        c.visible = true;
        c.fetch = FetchState::Success;
        s.search_cmp = Some(c);
        insta::assert_snapshot!(screen(app));
    }

    #[test]
    fn comment_editor_with_mentions() {
        let mut app = with_prs(app(140, 40, Config::default()));
        let mut c = Completer::new(SourceKind::Mention { with_at: true }, None, true);
        c.filtered = vec![Suggestion {
            value: "octocat".into(),
            detail: "The Octocat".into(),
        }];
        c.visible = true;
        c.fetch = FetchState::Loading;
        app.pr_editor = Some(Editor {
            mode: EditorMode::Comment,
            prompt: "Leave a comment…".into(),
            area: TextArea::new("Looks good, thanks @oct"),
            confirm_discard: true,
            discarding: false,
            cmp: Some(c),
        });
        insta::assert_snapshot!(screen(app));
    }

    #[test]
    fn notifications() {
        let mut app = app(140, 24, Config::default());
        app.view = ViewType::Notifications;
        let n = |id: &str, title: &str, kind: &str, reason: &str| {
            NotifRow::new(Notification {
                id: id.into(),
                reason: reason.into(),
                updated_at: Utc::now() - Duration::hours(5),
                subject: crate::model::NotificationSubject {
                    title: title.into(),
                    url: format!("https://api.github.com/repos/juanibiapina/ghtui/pulls/{id}"),
                    kind: kind.into(),
                    ..Default::default()
                },
                repository: crate::model::NotificationRepository {
                    name: "ghtui".into(),
                    full_name: "juanibiapina/ghtui".into(),
                    ..Default::default()
                },
                ..Default::default()
            })
        };
        let rows = vec![
            n(
                "7",
                "Draw tables with ratatui",
                "PullRequest",
                "review_requested",
            ),
            n("8", "Crash on resize", "Issue", "mention"),
        ];
        let s = &mut app.notifs[0];
        s.total_count = rows.len();
        s.notifs = rows;
        s.last_fetch_at = Some(std::time::Instant::now());
        s.last_updated =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2026, 1, 1, 12, 0, 0).unwrap();
        insta::assert_snapshot!(screen(app));
    }
}
