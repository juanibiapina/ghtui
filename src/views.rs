//! Preview pane content: PR, issue and notification views, and shared pieces (header, title block, pills, labels,
//! the input box).

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};

use crate::icons;
use crate::input::TextArea;
use crate::markdown;
use crate::model::*;
use crate::news::{NewItem, NewKind, News};
use crate::section::{NotifRow, PrRow, render_lines};
use crate::text::*;
use crate::theme::{self, Theme, darken, lighten, parse_color};
use crate::util::time_elapsed;

const CONTENT_PADDING: usize = 2;
const FOLD_BODY_HEIGHT: usize = 8;
const NEWS_LIMIT: usize = 10;
const NEWS_COMMITS_LIMIT: usize = 5;

pub const PR_TABS: [&str; 5] = ["Overview", "Activity", "Commits", "Checks", "Files Changed"];
const PR_TAB_ICONS: [&str; 5] = [
    icons::OVERVIEW,
    icons::DISCUSSION,
    icons::COMMIT,
    icons::CHECKS,
    icons::FILES,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorMode {
    Comment,
    Approve,
    Assign,
    Unassign,
    Label,
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub mode: EditorMode,
    pub prompt: String,
    pub area: TextArea,
    /// Comment and approve ask before throwing away the text.
    pub confirm_discard: bool,
    pub discarding: bool,
    pub completer: Option<crate::complete::Completer>,
}

fn glyph((g, s): (&'static str, Style)) -> Span<'static> {
    span(g, s)
}

pub fn preview_header(t: &Theme, width: usize, text: &str) -> Block {
    let st = Style::default()
        .bg(t.selected_background)
        .fg(t.secondary_text);
    wrap_str(text, width.saturating_sub(1).max(1), st)
        .into_iter()
        .map(|l| fit_line(&concat(vec![Line::from(span(" ", st)), l]), width, st))
        .collect()
}

/// A 3-line highlighted block with the title centered vertically.
pub fn preview_title(t: &Theme, width: usize, title: &str) -> Block {
    let bg = Style::default().bg(t.selected_background);
    let st = t.main_text().bg(t.selected_background);
    let mut lines = wrap_str(title, width.saturating_sub(1).max(1), st);
    let h = lines.len().max(3);
    let top = (h - lines.len()) / 2;
    let mut out: Block = vec![];
    for _ in 0..top {
        out.push(fit_line(&Line::default(), width, bg));
    }
    for l in lines.drain(..) {
        out.push(fit_line(
            &concat(vec![Line::from(span(" ", bg)), l]),
            width,
            bg,
        ));
    }
    while out.len() < h {
        out.push(fit_line(&Line::default(), width, bg));
    }
    out
}

/// Powerline caps around bold text on a colored background.
pub fn pill(text: &str, bg: Color, fg: Color, cap_bg: Option<Color>) -> Vec<Span<'static>> {
    let cap = |s: &str| {
        let mut st = Style::default().fg(bg);
        if let Some(c) = cap_bg {
            st = st.bg(c);
        }
        span(s.to_string(), st)
    };
    vec![
        cap(icons::POWERLINE_LEFT),
        span(
            text.to_string(),
            Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
        ),
        cap(icons::POWERLINE_RIGHT),
    ]
}

/// GitHub emoji shortcodes become their emoji.
fn label_name(name: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r":[[:alnum:]_+-]+:").unwrap());
    re.replace_all(name, |c: &regex::Captures| {
        let token = &c[0];
        match emojis::get_by_shortcode(&token[1..token.len() - 1]) {
            Some(e) => e.as_str().to_string(),
            None => token.to_string(),
        }
    })
    .to_string()
}

fn label_pill(l: &Label) -> Vec<Span<'static>> {
    let base = parse_color(&format!("#{}", l.color)).unwrap_or(Color::Rgb(0, 0, 0));
    let c = darken(base, 0.3);
    pill(&label_name(&l.name), c, lighten(c, 0.5), None)
}

/// `max_rows == 0` wraps freely; otherwise overflow becomes a "+N" pill.
pub fn render_labels(labels: &[Label], width: usize, max_rows: usize) -> Block {
    if width == 0 || labels.is_empty() {
        return vec![];
    }
    let mut rows: Block = vec![];
    let mut cur: Vec<Span<'static>> = vec![];
    let mut cur_w = 0;
    for (idx, l) in labels.iter().enumerate() {
        let p = label_pill(l);
        let pw: usize = p.iter().map(|s| str_width(&s.content)).sum();
        if cur_w > 0 && cur_w + 1 + pw > width {
            if max_rows > 0 && rows.len() + 1 >= max_rows {
                let remaining = labels.len() - idx;
                let overflow = pill(
                    &format!("+{remaining}"),
                    Color::Indexed(244),
                    Color::Indexed(15),
                    None,
                );
                let ow: usize = overflow.iter().map(|s| str_width(&s.content)).sum();
                if cur_w + 1 + ow <= width {
                    cur.push(raw(" "));
                    cur.extend(overflow);
                }
                rows.push(Line::from(std::mem::take(&mut cur)));
                return rows;
            }
            rows.push(Line::from(std::mem::take(&mut cur)));
            cur_w = 0;
        }
        if cur_w > 0 {
            cur.push(raw(" "));
            cur_w += 1;
        }
        if pw > width {
            cur.push(Span::raw(truncate_str(&l.name, width, icons::ELLIPSIS)));
            cur_w += width;
        } else {
            cur.extend(p);
            cur_w += pw;
        }
    }
    if !cur.is_empty() {
        rows.push(Line::from(cur));
    }
    rows
}

fn heading(t: &Theme, text: String) -> Line<'static> {
    Line::from(span(text, t.main_text().add_modifier(Modifier::UNDERLINED)))
}

fn author_line(
    t: &Theme,
    login: &str,
    assoc: AuthorAssociation,
    created: chrono::DateTime<chrono::Utc>,
) -> Line<'static> {
    let assoc_label = assoc.label();
    let (icon, color) = t.author_role_icon(assoc);
    Line::from(vec![
        raw(" by "),
        span(
            format!("@{login}"),
            t.primary().add_modifier(Modifier::BOLD),
        ),
        span(format!(" ⋅ {} ago ⋅ ", time_elapsed(created)), t.faint()),
        span(icon, Style::default().fg(color)),
        span(" ", t.faint()),
        span(assoc_label, t.faint()),
    ])
}

/// Lines framed by `set` in `border`, with content `inner_w` wide.
fn boxed(lines: Block, inner_w: usize, border: Color, set: border::Set) -> Block {
    let b = Style::default().fg(border);
    let edge = |left: &str, line: &str, right: &str| {
        Line::from(span(format!("{left}{}{right}", line.repeat(inner_w)), b))
    };
    let mut out = vec![edge(set.top_left, set.horizontal_top, set.top_right)];
    for l in lines {
        let mut row = vec![span(set.vertical_left, b)];
        row.extend(fit_line(&l, inner_w, Style::default()).spans);
        row.push(span(set.vertical_right, b));
        out.push(Line::from(row));
    }
    out.push(edge(
        set.bottom_left,
        set.horizontal_bottom,
        set.bottom_right,
    ));
    out
}

/// A padded block with an optional bottom divider.
fn check_category(
    t: &Theme,
    icon: Span<'static>,
    title: &str,
    subtitle: &str,
    is_last: bool,
    w: usize,
) -> Block {
    let mut out: Block = vec![Line::default()];
    out.push(Line::from(vec![
        raw(" "),
        icon,
        raw(" "),
        span(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    if !subtitle.is_empty() {
        for l in wrap_str(subtitle, w.saturating_sub(4).max(1), t.faint()) {
            out.push(concat(vec![Line::from(raw("   ")), l]));
        }
    }
    out.push(Line::default());
    if !is_last {
        out.push(Line::from(span(
            "─".repeat(w),
            Style::default().fg(t.faint_border),
        )));
    }
    out
}

#[derive(Default)]
struct ChecksStats {
    succeeded: i64,
    neutral: i64,
    failed: i64,
    skipped: i64,
    in_progress: i64,
    awaiting_approval: i64,
}

fn add_check_counts(stats: &mut ChecksStats, counts: &[CountByState]) {
    for c in counts {
        let s = c.state;
        if s.is_waiting() {
            stats.in_progress += c.count;
        } else if s.is_failure() {
            stats.failed += c.count;
        } else if s.is_skip() {
            stats.skipped += c.count;
        } else if s.is_neutral() {
            stats.neutral += c.count;
        } else if s.is_success() {
            stats.succeeded += c.count;
        }
    }
}

fn checks_stats(e: &PullRequestDetails) -> ChecksStats {
    let mut s = ChecksStats::default();
    let Some(last) = e.head() else {
        return s;
    };
    if let Some(rollup) = &last.status_check_rollup {
        add_check_counts(&mut s, &rollup.contexts.counts.check_run_counts_by_state);
        add_check_counts(
            &mut s,
            &rollup.contexts.counts.status_context_counts_by_state,
        );
    }
    for suite in &last.check_suites.nodes {
        if suite.is_awaiting_approval() {
            s.awaiting_approval += 1;
        } else if suite.is_pending() {
            s.in_progress += 1;
        }
    }
    s
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Status {
    Success,
    Failure,
    Waiting,
    NonRequested,
}

pub struct PrViewState<'a> {
    pub tab: usize,
    pub summary_more: bool,
    pub editor: Option<&'a Editor>,
    pub approve_workflows_key: String,
    /// What changed since a notification was last read, when previewing one.
    pub news: Option<&'a News>,
}

/// What changed since the notification was last read, newest first, the
/// newest `NEWS_LIMIT` items in full.
fn news_block(t: &Theme, news: &News, noun: &str, earlier: &str, w: usize) -> Block {
    let title = if news.since.is_some() {
        "What's new since you last read"
    } else {
        "What's new"
    };
    let mut out = vec![
        heading(t, format!("{} {title}", icons::UPDATED_AT)),
        Line::default(),
    ];
    if news.items.is_empty() {
        out.push(Line::from(span(
            "No new activity since you last read this",
            t.faint().add_modifier(Modifier::ITALIC),
        )));
        out.push(Line::default());
        return out;
    }
    let gutter = Style::default().fg(t.warning_text);
    let inner = w.saturating_sub(2);
    for item in news.items.iter().take(NEWS_LIMIT) {
        let mut lines = vec![truncate_line(
            &news_header(t, item, noun),
            inner,
            icons::ELLIPSIS,
        )];
        lines.extend(news_body(t, item, inner));
        out.extend(
            lines
                .into_iter()
                .map(|l| concat(vec![Line::from(span("▌ ", gutter)), l])),
        );
        out.push(Line::default());
    }
    let rest = news.items.len().saturating_sub(NEWS_LIMIT);
    if rest > 0 {
        let updates = if rest == 1 { "update" } else { "updates" };
        out.push(Line::from(span(
            format!("{rest} earlier {updates} {earlier}"),
            t.faint(),
        )));
        out.push(Line::default());
    }
    out
}

fn news_header(t: &Theme, item: &NewItem, noun: &str) -> Line<'static> {
    let mut spans = vec![];
    if let NewKind::Review { state, .. } = &item.kind {
        match state {
            ReviewState::Approved => spans.push(glyph(t.success_glyph())),
            ReviewState::ChangesRequested => spans.push(glyph(t.failure_glyph())),
            _ => spans.push(span(icons::REVIEW_COMMENTED, t.faint())),
        }
        spans.push(raw(" "));
    }
    if !item.actor.is_empty() {
        spans.push(span(
            format!("@{}", item.actor),
            t.primary().add_modifier(Modifier::BOLD),
        ));
        spans.push(raw(" "));
    }
    let who = |name: &str| span(format!("@{name}"), t.primary());
    match &item.kind {
        NewKind::Opened => spans.push(raw(format!("opened this {noun}"))),
        NewKind::Comment { .. } => spans.push(raw("commented")),
        NewKind::Review { state, .. } => spans.push(raw(match state {
            ReviewState::Approved => "approved",
            ReviewState::ChangesRequested => "requested changes",
            _ => "reviewed",
        })),
        NewKind::ReviewComment { path, line, .. } => {
            let location = match line {
                Some(line) => format!("{path}#L{line}"),
                None => path.clone(),
            };
            spans.push(raw("commented on "));
            spans.push(span(location, t.secondary()));
        }
        NewKind::Commits { headlines } => {
            let s = if headlines.len() == 1 { "" } else { "s" };
            spans.push(raw(format!("pushed {} commit{s}", headlines.len())));
        }
        NewKind::ForcePushed => spans.push(raw("force-pushed the branch")),
        NewKind::Closed => spans.push(raw(format!("closed this {noun}"))),
        NewKind::Reopened => spans.push(raw(format!("reopened this {noun}"))),
        NewKind::Merged => spans.push(raw(format!("merged this {noun}"))),
        NewKind::ReviewRequested { reviewer } => {
            spans.push(raw("requested review from "));
            spans.push(who(reviewer));
        }
        NewKind::Assigned { assignee } => {
            spans.push(raw("assigned "));
            spans.push(who(assignee));
        }
        NewKind::Unassigned { assignee } => {
            spans.push(raw("unassigned "));
            spans.push(who(assignee));
        }
        NewKind::Labeled { label } => {
            spans.push(raw("added "));
            spans.extend(label_pill(label));
        }
        NewKind::Unlabeled { label } => {
            spans.push(raw("removed "));
            spans.extend(label_pill(label));
        }
        NewKind::Renamed { .. } => spans.push(raw("changed the title")),
        NewKind::ReadyForReview => spans.push(raw(format!("marked this {noun} ready for review"))),
        NewKind::ConvertedToDraft => spans.push(raw(format!("converted this {noun} to a draft"))),
    }
    spans.push(span(format!(" · {} ago", time_elapsed(item.at)), t.faint()));
    Line::from(spans)
}

fn news_body(t: &Theme, item: &NewItem, w: usize) -> Block {
    match &item.kind {
        NewKind::Comment { body }
        | NewKind::Review { body, .. }
        | NewKind::ReviewComment { body, .. } => {
            let body = markdown::clean_body(body);
            let body = body.trim();
            let mut out = if body.is_empty() {
                vec![]
            } else {
                markdown::render(body, w)
            };
            while out.last().is_some_and(|l| l.width() == 0) {
                out.pop();
            }
            out
        }
        NewKind::Commits { headlines } => {
            let mut out: Block = headlines
                .iter()
                .rev()
                .take(NEWS_COMMITS_LIMIT)
                .map(|h| {
                    let l = Line::from(vec![
                        span(icons::VERTICAL_COMMIT, t.faint()),
                        raw(" "),
                        span(h.clone(), t.main_text()),
                    ]);
                    truncate_line(&l, w, icons::ELLIPSIS)
                })
                .collect();
            let rest = headlines.len().saturating_sub(NEWS_COMMITS_LIMIT);
            if rest > 0 {
                out.push(Line::from(span(format!("and {rest} more"), t.faint())));
            }
            out
        }
        NewKind::Renamed { from, to } => {
            let mut out = wrap_str(
                from,
                w.max(1),
                t.faint().add_modifier(Modifier::CROSSED_OUT),
            );
            out.extend(wrap_str(to, w.max(1), t.main_text()));
            out
        }
        _ => vec![],
    }
}

/// Each PR tab that fits in `width`: its label, start column and width.
fn pr_tabs(width: usize) -> Vec<(String, usize, usize)> {
    let mut tabs = vec![];
    let mut used = 0;
    for (i, name) in PR_TABS.iter().enumerate() {
        let label = format!(" {} {} ", PR_TAB_ICONS[i], name);
        let w = str_width(&label);
        if used + w > width {
            break;
        }
        tabs.push((label, used, w));
        used += w;
    }
    tabs
}

/// The start column and width of each PR tab that fits in `width`.
pub fn pr_tab_ranges(width: usize) -> Vec<(usize, usize)> {
    pr_tabs(width).into_iter().map(|(_, x, w)| (x, w)).collect()
}

/// A PR preview and the index of its tabs line.
pub fn pr_view(t: &Theme, row: &PrRow, width: usize, st: &PrViewState) -> (Block, usize) {
    let pr = &row.primary;
    let mut out: Block = vec![];
    out.extend(preview_header(
        t,
        width,
        &format!("{} · #{}", pr.repository.name_with_owner, pr.number),
    ));
    out.extend(preview_title(t, width, &pr.title));
    out.push(Line::default());

    let (state_text, pill_bg) = match pr.state {
        State::Open if pr.is_in_merge_queue => {
            (format!("{} Queued", icons::MERGE_QUEUE), theme::OPEN_PR)
        }
        State::Open if pr.is_draft => (format!("{} Draft", icons::DRAFT), t.faint_text),
        State::Open => (format!("{} Open", icons::OPEN), theme::OPEN_PR),
        State::Closed => (format!("{} Closed", icons::CLOSED), theme::CLOSED_PR),
        State::Merged => (format!("{} Merged", icons::MERGED), theme::MERGED_PR),
        State::Unknown => (String::new(), t.faint_text),
    };
    let mut branches = vec![raw(" ")];
    branches.extend(pill(&state_text, pill_bg, t.inverted_text, None));
    branches.push(raw(" "));
    branches.push(span(
        format!(
            "{} {} {}",
            pr.base_ref_name,
            icons::BRANCH_ARROW,
            pr.head_ref_name
        ),
        t.secondary(),
    ));
    out.push(Line::from(branches));
    out.push(Line::default());
    out.push(author_line(
        t,
        Login::of(&pr.author),
        pr.author_association,
        pr.created_at,
    ));
    out.push(Line::default());
    let w = width.saturating_sub(2 * CONTENT_PADDING);
    if let Some(news) = st.news {
        out.extend(indent(
            news_block(t, news, "pull request", "in Activity", w),
            CONTENT_PADDING,
        ));
    }

    let tab_spans: Vec<Span<'static>> = pr_tabs(width)
        .into_iter()
        .enumerate()
        .map(|(i, (label, _, _))| {
            let style = if i == st.tab {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                t.faint()
            };
            span(label, style)
        })
        .collect();
    let tabs_line = out.len();
    out.push(fit_line(&Line::from(tab_spans), width, Style::default()));
    out.push(Line::from(span(
        "─".repeat(width),
        Style::default().fg(t.faint_border),
    )));
    out.push(Line::default());

    let body = match st.tab {
        0 => pr_overview(t, row, w, st),
        1 => pr_activity(t, row, w, st.news),
        2 => pr_commits(t, row, w),
        3 => {
            let mut b = checks_overview(t, row, w, &st.approve_workflows_key);
            b.push(Line::default());
            b.push(Line::default());
            b.extend(all_checks(t, row, w));
            b
        }
        _ => changed_files(t, row, w),
    };
    out.extend(indent(body, CONTENT_PADDING));
    (out, tabs_line)
}

fn pr_overview(t: &Theme, row: &PrRow, w: usize, st: &PrViewState) -> Block {
    let mut b: Block = vec![];
    let reviewers = requested_reviewers(t, row, w);
    if !reviewers.is_empty() {
        b.extend(reviewers);
        b.push(Line::default());
    }
    let labels = &row.primary.labels.nodes;
    if !labels.is_empty() {
        b.push(heading(t, format!("{} Labels", icons::LABELS)));
        b.push(Line::default());
        b.extend(render_labels(labels, w, 0));
        b.push(Line::default());
    }
    b.extend(summary(t, row, w, st.summary_more));
    b.push(Line::default());
    b.push(heading(t, format!("{} Changes", icons::FILES)));
    b.push(Line::default());
    b.extend(changes_overview(t, row, w));
    b.push(Line::default());
    b.push(heading(t, format!("{} Checks", icons::CHECKS)));
    b.push(Line::default());
    b.extend(checks_overview(t, row, w, &st.approve_workflows_key));
    if let Some(ed) = st.editor {
        b.extend(editor_view(t, ed, w));
    }
    b
}

fn requested_reviewers(t: &Theme, row: &PrRow, w: usize) -> Block {
    let title = heading(t, format!("{} Reviewers", icons::CODE_REVIEW));
    let Some(e) = &row.details else {
        return vec![
            title,
            Line::default(),
            Line::from(vec![
                glyph(t.waiting_glyph()),
                raw(" "),
                span("Loading...", t.faint()),
            ]),
        ];
    };
    if e.review_requests.nodes.is_empty()
        && e.reviews.nodes.is_empty()
        && e.suggested_reviewers.is_empty()
    {
        return vec![];
    }
    let mut states: Vec<(String, ReviewState)> = vec![];
    for r in &e.reviews.nodes {
        let login = Login::of(&r.author).to_string();
        match states.iter_mut().find(|(l, _)| *l == login) {
            Some((_, s)) => {
                if r.state == ReviewState::Commented
                    && matches!(s, ReviewState::Approved | ReviewState::ChangesRequested)
                {
                    continue;
                }
                *s = r.state;
            }
            None => states.push((login, r.state)),
        }
    }
    let faint = t.faint();
    let mut items: Vec<Vec<Span<'static>>> = vec![];
    let mut shown: Vec<String> = vec![];
    for req in &e.review_requests.nodes {
        let Some(name) = req.display_name().map(str::to_string) else {
            continue;
        };
        shown.push(name.clone());
        let commented = states
            .iter()
            .any(|(l, s)| *l == name && *s == ReviewState::Commented);
        let icon = if commented {
            glyph(t.comment_glyph())
        } else {
            glyph(t.waiting_dot_glyph())
        };
        let mut item = vec![icon, raw(" ")];
        if req.as_code_owner {
            item.push(span(icons::OWNER, faint));
            item.push(raw(" "));
        }
        item.push(span(
            if req.is_team() {
                name
            } else {
                format!("@{name}")
            },
            faint,
        ));
        items.push(item);
    }
    for (login, state) in &states {
        if shown.contains(login)
            || !matches!(
                state,
                ReviewState::Approved | ReviewState::ChangesRequested | ReviewState::Commented
            )
        {
            continue;
        }
        shown.push(login.clone());
        let icon = match state {
            ReviewState::Approved => span(icons::APPROVED, Style::default().fg(t.success_text)),
            ReviewState::ChangesRequested => {
                span(icons::CHANGES_REQUESTED, Style::default().fg(t.error_text))
            }
            _ => glyph(t.comment_glyph()),
        };
        items.push(vec![icon, raw(" "), span(format!("@{login}"), faint)]);
    }
    for s in &e.suggested_reviewers {
        let login = &s.reviewer.login;
        if shown.contains(login) || s.is_author {
            continue;
        }
        shown.push(login.clone());
        items.push(vec![
            span(icons::OWNER, faint),
            raw(" "),
            span(format!("@{login}"), faint),
        ]);
    }
    if items.is_empty() {
        return vec![];
    }
    let mut rows: Block = vec![];
    let mut cur: Vec<Span<'static>> = vec![];
    let mut cur_w = 0;
    let n = items.len();
    for (i, item) in items.into_iter().enumerate() {
        let iw: usize = item.iter().map(|s| str_width(&s.content)).sum();
        let sep = i < n - 1;
        let total = iw + if sep { 2 } else { 0 };
        if cur_w > 0 && cur_w + total > w {
            rows.push(Line::from(std::mem::take(&mut cur)));
            cur_w = 0;
        }
        cur.extend(item);
        cur_w += iw;
        if sep {
            cur.push(raw(", "));
            cur_w += 2;
        }
    }
    if !cur.is_empty() {
        rows.push(Line::from(cur));
    }
    let mut out = vec![title, Line::default()];
    out.extend(rows);
    out
}

fn summary(t: &Theme, row: &PrRow, w: usize, more: bool) -> Block {
    let body = row
        .details
        .as_ref()
        .map(|e| e.body.clone())
        .unwrap_or_default();
    let body = markdown::clean_body(&body);
    let mut out = vec![
        Line::from(span(
            format!("{} Summary", icons::SUMMARY),
            t.main_text().add_modifier(Modifier::UNDERLINED),
        )),
        Line::default(),
    ];
    let body = body.trim();
    if body.is_empty() {
        out.push(Line::from(span(
            "No description provided.",
            t.faint().add_modifier(Modifier::ITALIC),
        )));
        return out;
    }
    let mut rendered = markdown::render(body, w);
    if !more && rendered.len() > FOLD_BODY_HEIGHT {
        rendered.truncate(FOLD_BODY_HEIGHT);
        rendered.push(Line::default());
        let hint = Line::from(vec![
            span(
                "Press ",
                Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ),
            span(
                "e",
                Style::default()
                    .bg(t.selected_background)
                    .fg(t.primary_text),
            ),
            span(
                " to read more...",
                Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ),
        ]);
        rendered.push(align_line(
            hint,
            w,
            ratatui::layout::Alignment::Center,
            Style::default(),
        ));
    }
    out.extend(rendered);
    out
}

fn changes_overview(t: &Theme, row: &PrRow, w: usize) -> Block {
    let pr = &row.primary;
    let (files, commits) = row
        .details
        .as_ref()
        .map(|e| (e.files.total_count, e.commits.total_count))
        .unwrap_or((0, 0));
    let inner = w.saturating_sub(2);
    let mut lines: Block = vec![Line::default()];
    let mut l1 = vec![
        raw(" "),
        span(format!("{} ", icons::FILES), t.faint()),
        raw(format!("{files} files changed ")),
    ];
    l1.extend(render_lines(pr, t));
    lines.push(Line::from(l1));
    lines.push(Line::default());
    lines.push(Line::from(span(
        "─".repeat(inner),
        Style::default().fg(t.faint_border),
    )));
    lines.push(Line::default());
    lines.push(Line::from(vec![
        raw(" "),
        span(format!("{} ", icons::COMMITS_HEADER), t.faint()),
        raw(format!("{commits} commits ")),
        span(format!("{} ago", time_elapsed(pr.updated_at)), t.faint()),
    ]));
    lines.push(Line::default());
    boxed(lines, inner, t.faint_border, border::ROUNDED)
}

fn checks_overview(t: &Theme, row: &PrRow, w: usize, approve_key: &str) -> Block {
    let pr = &row.primary;
    let inner = w.saturating_sub(2);
    if pr.state == State::Merged {
        let c = check_category(
            t,
            glyph(t.merged_glyph()),
            "Pull request successfully merged and closed",
            "The branch has been merged",
            true,
            inner,
        );
        return boxed(c, inner, theme::MERGED_PR, border::ROUNDED);
    }
    if pr.state == State::Closed {
        let c = check_category(
            t,
            raw(""),
            "Closed with unmerged commits",
            "This pull request is closed",
            true,
            inner,
        );
        return boxed(c, inner, t.faint_border, border::ROUNDED);
    }
    let (review, rs) = review_status(t, row, inner);
    let (checks, cs) = checks_status(t, row, inner, approve_key);
    let (merge, ms) = merge_status(t, row, inner);
    let border = if rs == Status::Failure || cs == Status::Failure || ms == Status::Failure {
        t.error_text
    } else if rs == Status::Success && cs == Status::Success && ms == Status::Success {
        t.success_text
    } else {
        t.faint_border
    };
    let mut parts = vec![];
    parts.extend(review);
    parts.extend(checks);
    parts.extend(merge);
    boxed(parts, inner, border, border::ROUNDED)
}

fn num_code_owner_requests(row: &PrRow) -> i64 {
    row.details
        .as_ref()
        .map(|e| {
            e.review_requests
                .nodes
                .iter()
                .filter(|r| r.as_code_owner)
                .count() as i64
        })
        .unwrap_or(0)
}

fn review_status(t: &Theme, row: &PrRow, w: usize) -> (Block, Status) {
    let pr = &row.primary;
    let reviews = row
        .details
        .as_ref()
        .map(|e| e.reviews.nodes.clone())
        .unwrap_or_default();
    let count = |s: ReviewState| reviews.iter().filter(|r| r.state == s).count() as i64;
    let (approving, changes, commented) = (
        count(ReviewState::Approved),
        count(ReviewState::ChangesRequested),
        count(ReviewState::Commented),
    );
    let owners = num_code_owner_requests(row);
    let rules = &pr.repository.branch_protection_rules.nodes;
    let (icon, title, subtitle, status) = match pr.review_decision {
        Some(ReviewDecision::Approved) => (
            glyph(t.success_glyph()),
            "Changes approved",
            format!("{approving} approving reviews"),
            Status::Success,
        ),
        Some(ReviewDecision::ChangesRequested) => (
            glyph(t.failure_glyph()),
            "Changes requested",
            format!("{changes} requested changes"),
            Status::Failure,
        ),
        Some(ReviewDecision::ReviewRequired) => {
            let code_owner_required = rules
                .first()
                .map(|r| r.requires_code_owner_reviews)
                .unwrap_or(false);
            let (sub, st) = if (code_owner_required && approving < 1) || approving < owners {
                ("Code owner review required".to_string(), Status::Failure)
            } else if let Some(required) = rules
                .first()
                .and_then(|r| r.required_approving_review_count)
                .filter(|required| approving < *required)
            {
                (
                    format!("Need {} more approval", required - approving),
                    Status::Waiting,
                )
            } else if commented > 0 {
                (
                    format!("{commented} reviewers left comments"),
                    Status::Waiting,
                )
            } else {
                (String::new(), Status::Success)
            };
            (glyph(t.waiting_glyph()), "Review Required", sub, st)
        }
        _ => (
            glyph(t.person_glyph()),
            "Reviews",
            "None requested".to_string(),
            Status::NonRequested,
        ),
    };
    (check_category(t, icon, title, &subtitle, false, w), status)
}

fn checks_bar(t: &Theme, s: &ChecksStats, w: usize) -> Line<'static> {
    let total =
        (s.failed + s.skipped + s.neutral + s.succeeded + s.in_progress + s.awaiting_approval)
            as f64;
    let parts: Vec<(i64, Color)> = vec![
        (s.failed, t.error_text),
        (s.awaiting_approval, t.warning_text),
        (s.in_progress, t.warning_text),
        (s.skipped + s.neutral, t.faint_text),
        (s.succeeded, t.success_text),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .collect();
    let mut w = w as i64 - 6 - (parts.len() as i64 - 1);
    if w < 0 {
        w = 0;
    }
    let mut spans = vec![];
    for (i, (n, c)) in parts.iter().enumerate() {
        let pw = ((*n as f64 / total) * w as f64).floor() as usize;
        if i > 0 {
            spans.push(raw(" "));
        }
        spans.push(span("▃".repeat(pw), Style::default().fg(*c)));
    }
    Line::from(spans)
}

fn checks_status(t: &Theme, row: &PrRow, w: usize, approve_key: &str) -> (Block, Status) {
    let Some(e) = &row.details else {
        return (
            check_category(t, glyph(t.waiting_glyph()), "Loading...", "", false, w),
            Status::Waiting,
        );
    };
    let s = checks_stats(e);
    let (icon, title, status) = if s.failed > 0 {
        (
            glyph(t.failure_glyph()),
            "Some checks were not successful",
            Status::Failure,
        )
    } else if s.awaiting_approval > 0 {
        (
            glyph(t.action_required_glyph()),
            "Workflows awaiting approval",
            Status::Waiting,
        )
    } else if s.in_progress > 0 {
        (
            glyph(t.waiting_glyph()),
            "Some checks haven’t completed yet",
            Status::Waiting,
        )
    } else if s.succeeded > 0 {
        (
            glyph(t.success_glyph()),
            "All checks have passed",
            Status::Success,
        )
    } else {
        return (vec![], Status::Waiting);
    };
    let mut stats = vec![];
    if s.failed > 0 {
        stats.push(format!("{} failing", s.failed));
    }
    if s.awaiting_approval > 0 {
        stats.push(format!(
            "{} awaiting approval. Press {} to run.",
            s.awaiting_approval, approve_key
        ));
    }
    if s.in_progress > 0 {
        stats.push(format!("{} in progress", s.in_progress));
    }
    if s.skipped > 0 {
        stats.push(format!("{} skipped", s.skipped));
    }
    if s.neutral > 0 {
        stats.push(format!("{} neutral", s.neutral));
    }
    if s.succeeded > 0 {
        stats.push(format!("{} successful", s.succeeded));
    }
    let mut block = check_category(t, icon, title, &stats.join(", "), false, w);
    // Insert the bar under the subtitle (before the trailing blank + divider).
    let bar = concat(vec![Line::from(raw("   ")), checks_bar(t, &s, w + 2)]);
    let at = block.len().saturating_sub(2);
    block.insert(at, bar);
    (block, status)
}

fn merge_status(t: &Theme, row: &PrRow, w: usize) -> (Block, Status) {
    let pr = &row.primary;
    let owners = num_code_owner_requests(row);
    let (icon, title, subtitle, status) = if matches!(
        pr.merge_state_status,
        MergeStateStatus::Clean | MergeStateStatus::Unstable
    ) {
        (
            glyph(t.success_glyph()),
            "No conflicts with base branch",
            "Changes can be cleanly merged",
            Status::Success,
        )
    } else if pr.is_draft {
        (
            glyph(t.draft_glyph()),
            "This pull request is still a work in progress",
            "Draft pull requests cannot be merged",
            Status::Waiting,
        )
    } else if pr.merge_state_status == MergeStateStatus::Blocked {
        let sub = if owners > 0 {
            "Waiting on code owner review"
        } else {
            ""
        };
        (
            glyph(t.failure_glyph()),
            "Merging is blocked",
            sub,
            Status::Failure,
        )
    } else if pr.mergeable == MergeableState::Conflicting {
        (
            glyph(t.failure_glyph()),
            "This branch has conflicts that must be resolved",
            "",
            Status::Failure,
        )
    } else {
        (raw(""), "", "", Status::Success)
    };
    (check_category(t, icon, title, subtitle, true, w), status)
}

fn all_checks(t: &Theme, row: &PrRow, w: usize) -> Block {
    let title = heading(t, format!("{} All Checks", icons::CHECKS));
    let mut out = vec![title, Line::default()];
    let Some(last) = row.details.as_ref().and_then(|e| e.head()) else {
        out.push(Line::from("Loading..."));
        return out;
    };
    let mut failures = vec![];
    let mut waiting = vec![];
    let mut rest = vec![];
    let mut awaiting = vec![];
    let mut pending = vec![];
    for suite in &last.check_suites.nodes {
        let name = [
            suite.workflow_run.as_ref().map(|r| r.workflow.name.trim()),
            suite.app.as_ref().map(|a| a.name.trim()),
        ]
        .into_iter()
        .flatten()
        .find(|n| !n.is_empty())
        .unwrap_or("Workflow")
        .to_string();
        if suite.is_awaiting_approval() {
            awaiting.push(Line::from(vec![
                glyph(t.action_required_glyph()),
                raw(" "),
                raw(name),
            ]));
        } else if suite.is_pending() {
            pending.push(Line::from(vec![
                glyph(t.waiting_glyph()),
                raw(" "),
                raw(name),
            ]));
        }
    }
    let mut reported = vec![];
    let contexts = last
        .status_check_rollup
        .as_ref()
        .map(|r| r.contexts.nodes.as_slice())
        .unwrap_or_default();
    for node in contexts {
        let (state_for_wait, conclusion, name, check_name) = match node {
            CheckContext::CheckRun(run) => {
                let suite = &run.check_suite;
                let parts: Vec<&str> = [
                    suite.creator.as_ref().map(|c| c.login.trim()),
                    suite.workflow_run.as_ref().map(|r| r.workflow.name.trim()),
                    Some(run.name.trim()),
                ]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect();
                (
                    run.status,
                    run.conclusion,
                    parts.join("/"),
                    run.name.clone(),
                )
            }
            CheckContext::StatusContext(status) => {
                let ctx = status.context.trim();
                let mut parts: Vec<&str> = vec![];
                if let Some(creator) = status.creator.as_ref().map(|c| c.login.trim())
                    && !creator.is_empty()
                {
                    parts.push(creator);
                }
                if !ctx.is_empty() && ctx != "/" {
                    parts.push(ctx);
                }
                (
                    status.state,
                    Some(status.state),
                    parts.join("/"),
                    status.context.clone(),
                )
            }
            CheckContext::Unknown => continue,
        };
        reported.push(check_name);
        let line =
            |g: (&'static str, Style)| Line::from(vec![glyph(g), raw(" "), raw(name.clone())]);
        if state_for_wait.is_waiting() {
            waiting.push(line(t.waiting_glyph()));
        } else if conclusion.is_some_and(CheckState::is_failure) {
            failures.push(line(t.failure_glyph()));
        } else {
            rest.push(line(t.success_glyph()));
        }
    }
    if let Some(rule) = row.primary.repository.branch_protection_rules.nodes.first() {
        for ctx in &rule.required_status_check_contexts {
            if !reported.contains(ctx) {
                pending.push(Line::from(vec![
                    glyph(t.waiting_glyph()),
                    raw(" "),
                    raw(ctx.clone()),
                ]));
            }
        }
    }
    if awaiting.len() + pending.len() + waiting.len() + failures.len() + rest.len() == 0 {
        out.push(Line::from(span(
            "  No checks to display...",
            Style::default().add_modifier(Modifier::ITALIC),
        )));
        return out;
    }
    let mut parts: Block = vec![];
    let warn = Style::default()
        .fg(t.warning_text)
        .add_modifier(Modifier::BOLD);
    if !awaiting.is_empty() {
        parts.push(Line::from(span(
            format!("Awaiting Approval ({})", awaiting.len()),
            warn,
        )));
        parts.extend(awaiting);
        parts.push(Line::default());
    }
    if !pending.is_empty() {
        parts.push(Line::from(span(
            format!("Pending ({})", pending.len()),
            warn,
        )));
        parts.extend(pending);
        parts.push(Line::default());
    }
    parts.extend(failures);
    parts.extend(waiting);
    parts.extend(rest);
    let parts: Block = parts
        .into_iter()
        .map(|l| truncate_line(&l, w.saturating_sub(2), icons::ELLIPSIS))
        .collect();
    out.extend(indent(parts, 2));
    out
}

fn pr_activity(t: &Theme, row: &PrRow, w: usize, news: Option<&News>) -> Block {
    let Some(e) = &row.details else {
        return vec![Line::from("Loading...")];
    };
    struct Activity {
        at: chrono::DateTime<chrono::Utc>,
        block: Block,
    }
    let is_new = |at| news.is_some_and(|n| n.is_new(at));
    let mut acts: Vec<Activity> = vec![];
    let comment_header = |author: &str, at: chrono::DateTime<chrono::Utc>| -> Block {
        let content = Line::from(vec![
            span(author.to_string(), t.main_text()),
            raw(" "),
            span(time_elapsed(at), t.faint()),
        ]);
        boxed(
            vec![content],
            w.saturating_sub(2),
            new_border(t, is_new(at)),
            border::ROUNDED,
        )
    };
    for th in &e.review_threads.nodes {
        for c in &th.comments.nodes {
            let mut b = comment_header(Login::of(&c.author), c.created_at);
            let location = match th.line {
                Some(line) => format!("{}#l{line}", th.path),
                None => th.path.clone(),
            };
            b.push(Line::from(span(location, t.faint())));
            b.push(Line::default());
            b.extend(markdown::render(&markdown::clean_body(&c.body), w));
            acts.push(Activity {
                at: c.created_at,
                block: b,
            });
        }
    }
    for c in &e.comments.nodes {
        let mut b = comment_header(Login::of(&c.author), c.created_at);
        b.extend(markdown::render(&markdown::clean_body(&c.body), w));
        acts.push(Activity {
            at: c.created_at,
            block: b,
        });
    }
    for r in &e.reviews.nodes {
        let decision = match r.state {
            ReviewState::Pending => glyph(t.waiting_glyph()),
            ReviewState::Commented => span(icons::REVIEW_COMMENTED, t.faint()),
            ReviewState::Approved => glyph(t.success_glyph()),
            ReviewState::ChangesRequested => glyph(t.failure_glyph()),
            ReviewState::Dismissed | ReviewState::Unknown => raw(""),
        };
        let at = r.submitted_at.unwrap_or(r.updated_at);
        let mut header = vec![
            decision,
            raw(" "),
            span(Login::of(&r.author).to_string(), t.main_text()),
            raw(" "),
            span(format!("reviewed {}", time_elapsed(at)), t.faint()),
        ];
        if is_new(at) {
            header.push(span(" • new", Style::default().fg(t.warning_text)));
        }
        let mut b = vec![Line::from(header)];
        b.extend(markdown::render(&r.body, w));
        acts.push(Activity { at, block: b });
    }
    if acts.is_empty() {
        return vec![Line::from(span(
            "No comments...",
            Style::default().add_modifier(Modifier::ITALIC),
        ))];
    }
    acts.sort_by_key(|a| a.at);
    let mut out = vec![
        heading(t, format!("{}  {} comments", icons::COMMENTS, acts.len())),
        Line::default(),
    ];
    for a in acts {
        out.extend(a.block);
    }
    out
}

fn pr_commits(t: &Theme, row: &PrRow, w: usize) -> Block {
    let faint = t.faint();
    let fainter = Style::default().fg(t.faint_border);
    let Some(e) = &row.details else {
        return vec![Line::from(vec![
            glyph(t.waiting_glyph()),
            raw(" "),
            span("Loading...", faint),
        ])];
    };
    let commits = &e.all_commits.nodes;
    let mut out = vec![
        heading(t, format!("{}  {} commits", icons::COMMIT, commits.len())),
        Line::default(),
    ];
    for (i, c) in commits.iter().enumerate() {
        let c = &c.commit;
        let name = c
            .author
            .as_ref()
            .and_then(|a| {
                a.user
                    .as_ref()
                    .map(|u| u.login.clone())
                    .or_else(|| a.name.clone())
            })
            .unwrap_or_default();
        let right = c.abbreviated_oid.clone();
        let rw = str_width(&right);
        let left = Line::from(vec![
            span(icons::VERTICAL_COMMIT, faint),
            raw(" "),
            span(c.message_headline.clone(), t.main_text()),
        ]);
        let left = truncate_line(&left, w.saturating_sub(rw + 1), icons::ELLIPSIS);
        let dashes = w.saturating_sub(left.width() + rw).max(1) - 1;
        let mut title = left.spans.clone();
        title.push(span(
            format!(" {} ", icons::HORIZONTAL_LINE.repeat(dashes)),
            fainter,
        ));
        title.push(span(right, faint));
        out.push(Line::from(title));
        let mut desc = vec![
            span("│ ", fainter),
            span(
                format!("@{name} committed {} ago", time_elapsed(c.committed_date)),
                faint,
            ),
        ];
        if let Some(rollup) = &c.status_check_rollup
            && rollup.contexts.total_count > 0
        {
            let ctx = &rollup.contexts;
            let mut s = ChecksStats::default();
            add_check_counts(&mut s, &ctx.check_run_counts_by_state);
            add_check_counts(&mut s, &ctx.status_context_counts_by_state);
            let sign = match rollup.state {
                CheckState::Error | CheckState::Failure => glyph(t.failure_glyph()),
                CheckState::Success => glyph(t.success_glyph()),
                _ => glyph(t.waiting_glyph()),
            };
            desc.extend([
                raw(" "),
                span(icons::SMALL_DOT, faint),
                raw(" "),
                sign,
                raw(" "),
                span(format!("{}/{}", s.succeeded, ctx.total_count), faint),
            ]);
        }
        out.push(Line::from(desc));
        if i + 1 < commits.len() {
            out.push(Line::from(span("│", fainter)));
        }
    }
    out
}

fn changed_files(t: &Theme, row: &PrRow, w: usize) -> Block {
    let Some(e) = &row.details else {
        return vec![];
    };
    let mut out = vec![];
    for f in &e.files.nodes {
        let icon = match f.change_type {
            PatchStatus::Added => span(icons::FILE_ADDED, Style::default().fg(t.success_text)),
            PatchStatus::Deleted => span(icons::FILE_DELETED, Style::default().fg(t.error_text)),
            PatchStatus::Renamed => span(icons::FILE_RENAMED, Style::default().fg(t.warning_text)),
            PatchStatus::Copied => span(icons::FILE_COPIED, Style::default().fg(t.warning_text)),
            PatchStatus::Modified | PatchStatus::Changed => {
                span(icons::FILE_MODIFIED, Style::default().fg(t.warning_text))
            }
            PatchStatus::Unknown => raw(""),
        };
        let prefix = vec![
            span(
                format!("{:<5}", format!("+{}", f.additions)),
                Style::default().fg(t.success_text),
            ),
            span(
                format!("{:<5}", format!("-{}", f.deletions)),
                Style::default().fg(t.error_text),
            ),
            raw(" "),
            icon,
            raw(" "),
        ];
        let pw: usize = prefix.iter().map(|s| str_width(&s.content)).sum();
        let remaining = w.saturating_sub(pw).max(1);
        let chars: Vec<char> = f.path.chars().collect();
        let first: String = chars.iter().take(remaining).collect();
        let mut l = prefix;
        l.push(raw(first));
        out.push(Line::from(l));
        if chars.len() > remaining {
            let rest: String = chars[remaining..].iter().collect();
            out.push(Line::from(vec![
                raw(" ".repeat(pw)),
                raw(format!(" {rest}")),
            ]));
        }
    }
    out
}

/// The prompt, text area and a short help line, in a bordered box.
pub fn editor_view(t: &Theme, ed: &Editor, w: usize) -> Block {
    let inner = w.saturating_sub(2);
    let prompt = if ed.discarding {
        span("Discard comment? (y/N)", Style::default().fg(t.error_text))
    } else {
        span(ed.prompt.clone(), t.secondary())
    };
    let mut lines: Block = vec![Line::from(prompt), Line::default()];
    lines.extend(ed.area.view(inner, 6, t.primary(), t.faint()));
    lines.push(Line::from(vec![
        span("Ctrl+d", t.primary()),
        span(" submit", t.faint()),
        span(" • ", Style::default().fg(t.secondary_border)),
        span("Ctrl+c/esc", t.primary()),
        span(" cancel", t.faint()),
        span(" • ", Style::default().fg(t.secondary_border)),
        span("ctrl+h", t.primary()),
        span(" toggle", t.faint()),
    ]));
    boxed(lines, inner, t.primary_border, border::PLAIN)
}

/// The border of a comment, highlighted when it is new.
fn new_border(t: &Theme, is_new: bool) -> Color {
    if is_new {
        t.warning_text
    } else {
        t.faint_border
    }
}

pub fn issue_view(
    t: &Theme,
    issue: &Issue,
    width: usize,
    editor: Option<&Editor>,
    news: Option<&News>,
) -> Block {
    let w = width.saturating_sub(2 * CONTENT_PADDING);
    let content_w = width.saturating_sub(6);
    let mut b: Block = vec![];
    b.extend(preview_header(
        t,
        w,
        &format!("#{} · {}", issue.number, issue.repository.name_with_owner),
    ));
    b.extend(preview_title(t, w, &issue.title));
    b.push(Line::default());
    let (text, bg) = if issue.state == State::Open {
        (format!("{} Open", icons::ISSUE_OPEN), theme::OPEN_ISSUE)
    } else {
        (
            format!("{} Closed", icons::ISSUE_CLOSED),
            theme::CLOSED_ISSUE,
        )
    };
    b.push(Line::from(pill(&text, bg, t.inverted_text, None)));
    b.push(Line::default());
    b.push(author_line(
        t,
        Login::of(&issue.author),
        issue.author_association,
        issue.created_at,
    ));
    b.push(Line::default());
    if let Some(news) = news {
        b.extend(news_block(t, news, "issue", "below", w));
    }
    let labels = render_labels(&issue.labels.nodes, content_w, 0);
    if !labels.is_empty() {
        b.extend(labels);
        b.push(Line::default());
    }
    let body = markdown::clean_body(&issue.body);
    let body = body.trim();
    if body.is_empty() {
        b.push(Line::from(span(
            "No description provided.",
            t.faint().add_modifier(Modifier::ITALIC),
        )));
    } else {
        b.extend(markdown::render(body, content_w));
    }
    b.push(Line::default());
    b.push(heading(t, format!("{} Comments", icons::DISCUSSION)));
    b.push(Line::default());
    let mut comments = issue.comments.nodes.clone();
    comments.sort_by_key(|c| c.created_at);
    let cw = content_w.saturating_sub(2);
    let mut cb: Block = vec![];
    if comments.is_empty() {
        cb.push(Line::from(span(
            "No comments...",
            Style::default().add_modifier(Modifier::ITALIC),
        )));
    }
    for c in comments {
        cb.extend(boxed(
            vec![Line::from(vec![
                span(Login::of(&c.author).to_string(), t.main_text()),
                raw(" "),
                span(time_elapsed(c.created_at), t.faint()),
            ])],
            cw.saturating_sub(2),
            new_border(t, news.is_some_and(|n| n.is_new(c.created_at))),
            border::ROUNDED,
        ));
        cb.extend(markdown::render(&markdown::clean_body(&c.body), cw));
    }
    b.extend(indent(cb, 2));
    if let Some(ed) = editor {
        b.push(Line::default());
        b.extend(editor_view(t, ed, w));
    }
    indent(b, CONTENT_PADDING)
}

/// Shown while a PR/issue subject loads.
pub fn notification_loading(t: &Theme, row: &NotifRow, width: usize) -> Block {
    let mut b = preview_title(t, width, &row.n.subject.title);
    b.push(Line::default());
    b.push(Line::from(span("Loading...", t.faint())));
    b
}

/// Shown for subjects that can't be previewed.
pub fn notification_prompt(t: &Theme, row: &NotifRow) -> Block {
    let margin = "      ";
    let kind = row.subject_type();
    let (type_name, enter_action) = match kind {
        SubjectType::PullRequest => ("PR".to_string(), "view"),
        SubjectType::Issue => ("Issue".to_string(), "view"),
        other => (format!("{other:?}"), "open in browser"),
    };
    let normal = t.primary();
    let action = Style::default().fg(t.success_text);
    let key = t.primary();
    let mut b: Block = vec![Line::default()];
    let tail = if enter_action == "view" {
        format!(" to {enter_action} the {type_name}")
    } else {
        format!(" to {enter_action}")
    };
    b.push(Line::from(vec![
        raw(margin),
        span("Press ", normal),
        span(
            " Enter ",
            Style::default().fg(t.primary_text).bg(t.faint_border),
        ),
        span(tail, normal),
    ]));
    b.push(Line::default());
    b.push(Line::from(vec![
        raw(margin),
        span("Other Actions", t.primary().add_modifier(Modifier::BOLD)),
    ]));
    b.push(Line::default());
    let actions = [
        ("d", "mark as done"),
        ("u", "unsubscribe"),
        ("b", "toggle bookmark"),
        ("t", "toggle filtering"),
        ("S", "sort by repo"),
        ("o", "open in browser"),
        ("Enter", enter_action),
    ];
    for (k, a) in actions {
        b.push(Line::from(vec![
            raw(margin),
            span(format!("{k:>7}"), key),
            raw("  "),
            span(a.to_string(), action),
        ]));
    }
    b
}

/// Working tree status, then the branch and its PR.
pub fn branch_view(
    t: &Theme,
    b: &crate::section::BranchRow,
    status: Option<&crate::git::NameStatus>,
    rule_w: usize,
) -> Block {
    let mut out: Block = vec![
        Line::from(span(
            "STATUS",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];
    match status {
        None => out.push(Line::from("Loading...")),
        Some(st) => {
            if st.added.is_empty() && st.removed.is_empty() && st.modified.is_empty() {
                out.push(Line::from("No changes"));
            }
            out.extend(st.added.iter().map(|f| Line::from(format!("A {f}"))));
            out.extend(st.removed.iter().map(|f| Line::from(format!("D {f}"))));
            out.extend(st.modified.iter().map(|f| Line::from(format!("M {f}"))));
        }
    }
    out.push(Line::default());
    out.push(Line::from(span(
        "─".repeat(rule_w),
        Style::default().fg(t.faint_border),
    )));
    out.push(Line::default());
    out.push(Line::from(b.b.name.clone()));
    if let Some(pr) = &b.pr {
        out.push(Line::from(format!("#{} {}", pr.number, pr.title)));
    }
    out
}
