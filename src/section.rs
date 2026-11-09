//! A section is one tab of a view: a search query, its fetched rows and the
//! table that shows them.

use std::collections::HashSet;
use std::time::Instant;

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{self, BorderType, Cell, Row, StatefulWidget, Table, TableState, Widget};

use crate::config::{Config, IssuesLayoutConfig, PrsLayoutConfig};
use crate::icons;
use crate::input::TextInput;
use crate::model::*;
use crate::text::*;
use crate::theme::{self, Theme};
use crate::util::{format_number, time_elapsed};

pub const SPINNER: [&str; 8] = ["⣾ ", "⣽ ", "⣻ ", "⢿ ", "⡿ ", "⣟ ", "⣯ ", "⣷ "];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pr,
    Issue,
    Notification,
    /// The local branches section of the repo view (FF_REPO_VIEW).
    Branch,
}

impl Kind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Kind::Pr => "pr",
            Kind::Issue => "issue",
            Kind::Notification => "notification",
            Kind::Branch => "repo",
        }
    }
    pub fn singular(&self) -> &'static str {
        match self {
            Kind::Pr => "PR",
            Kind::Issue => "Issue",
            Kind::Notification => "Notification",
            Kind::Branch => "branch",
        }
    }
    pub fn plural(&self) -> &'static str {
        match self {
            Kind::Pr => "PRs",
            Kind::Issue => "Issues",
            Kind::Notification => "Notifications",
            Kind::Branch => "branches",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PrRow {
    pub primary: PullRequest,
    pub enriched: Option<EnrichedPullRequest>,
}

#[derive(Clone, Debug, Default)]
pub struct NotifRow {
    pub n: Notification,
    pub new_comments: i64,
    pub subject_state: String,
    pub is_draft: bool,
    pub actor: String,
    pub activity: String,
    pub resolved_url: String,
}

fn last_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

impl NotifRow {
    pub fn new(n: Notification) -> Self {
        let activity = activity_description(&n.reason, &n.subject.kind, "");
        NotifRow {
            n,
            activity,
            ..Default::default()
        }
    }
    pub fn id(&self) -> &str {
        &self.n.id
    }
    pub fn subject_type(&self) -> &str {
        &self.n.subject.kind
    }
    pub fn title(&self) -> String {
        self.n
            .subject
            .title
            .replace('\r', "")
            .replace('\n', " ")
            .trim()
            .to_string()
    }
    pub fn repo(&self) -> &str {
        &self.n.repository.full_name
    }
    pub fn number(&self) -> i64 {
        match self.subject_type() {
            "PullRequest" | "Issue" => last_segment(&self.n.subject.url).parse().unwrap_or(0),
            _ => 0,
        }
    }
    fn base_url(&self) -> String {
        if !self.n.repository.html_url.is_empty() {
            self.n.repository.html_url.trim_end_matches('/').to_string()
        } else {
            format!("https://github.com/{}", self.n.repository.full_name)
        }
    }
    pub fn url(&self) -> String {
        let base = self.base_url();
        let num = last_segment(&self.n.subject.url);
        match self.subject_type() {
            "PullRequest" => format!("{base}/pull/{num}"),
            "Issue" => format!("{base}/issues/{num}"),
            "Discussion" if !num.is_empty() => format!("{base}/discussions/{num}"),
            "Discussion" => format!("{base}/discussions"),
            "Release" => format!("{base}/releases"),
            "Commit" => format!("{base}/commits"),
            "CheckSuite" if !self.resolved_url.is_empty() => self.resolved_url.clone(),
            "CheckSuite" => format!("{base}/actions"),
            _ => base,
        }
    }
}

pub fn activity_description(reason: &str, subject_type: &str, actor: &str) -> String {
    let has = !actor.is_empty();
    match reason {
        "comment" if has => match subject_type {
            "PullRequest" => format!("@{actor} commented on this pull request"),
            "Issue" => format!("@{actor} commented on this issue"),
            _ => format!("@{actor} commented"),
        },
        "comment" => "New comment".into(),
        "review_requested" if has => format!("@{actor} requested your review"),
        "review_requested" => "Review requested".into(),
        "mention" if has => format!("@{actor} mentioned you"),
        "mention" => "You were mentioned".into(),
        "author" => "Activity on your thread".into(),
        "assign" => "You were assigned".into(),
        "state_change" => match subject_type {
            "PullRequest" => "Pull request state changed".into(),
            "Issue" => "Issue state changed".into(),
            _ => "State changed".into(),
        },
        "ci_activity" => "CI activity".into(),
        "subscribed" if has => match subject_type {
            "PullRequest" => format!("@{actor} commented on this pull request"),
            "Issue" => format!("@{actor} commented on this issue"),
            _ => "Activity on subscribed thread".into(),
        },
        "subscribed" => "Activity on subscribed thread".into(),
        "team_mention" => "Your team was mentioned".into(),
        "security_alert" => "Security vulnerability detected".into(),
        _ if has => format!("@{actor} triggered this notification"),
        _ => String::new(),
    }
}

/// Selected row data, handed to the preview and to actions.
#[derive(Clone, Debug)]
pub enum RowRef {
    Pr(Box<PrRow>),
    Issue(Box<Issue>),
    Notification(Box<NotifRow>),
    Branch(Box<BranchRow>),
}

/// A local branch and the PR whose head it is.
#[derive(Clone, Debug, Default)]
pub struct BranchRow {
    pub b: crate::git::Branch,
    pub pr: Option<PullRequest>,
}

impl RowRef {
    pub fn number(&self) -> i64 {
        match self {
            RowRef::Pr(p) => p.primary.number,
            RowRef::Issue(i) => i.number,
            RowRef::Notification(n) => n.number(),
            RowRef::Branch(b) => b.pr.as_ref().map(|p| p.number).unwrap_or(0),
        }
    }
    pub fn url(&self) -> String {
        match self {
            RowRef::Pr(p) => p.primary.url.clone(),
            RowRef::Issue(i) => i.url.clone(),
            RowRef::Notification(n) => n.url(),
            RowRef::Branch(b) => b.pr.as_ref().map(|p| p.url.clone()).unwrap_or_default(),
        }
    }
    pub fn repo(&self) -> String {
        match self {
            RowRef::Pr(p) => p.primary.repository.name_with_owner.clone(),
            RowRef::Issue(i) => i.repository.name_with_owner.clone(),
            RowRef::Notification(n) => n.repo().to_string(),
            RowRef::Branch(b) => b.b.remotes.first().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Column {
    pub title: String,
    pub width: Option<usize>,
    pub grow: bool,
    pub hidden: bool,
    pub align: Alignment,
}

fn col(title: &str, width: Option<usize>, hidden: bool) -> Column {
    Column {
        title: title.into(),
        width,
        grow: false,
        hidden,
        align: Alignment::Left,
    }
}

pub fn pr_columns(cfg: &Config, section_layout: &PrsLayoutConfig) -> Vec<Column> {
    let d = &cfg.defaults.layout.prs;
    let s = section_layout;
    let m = |a: &crate::config::ColumnConfig, b: &crate::config::ColumnConfig| a.merged(b);
    let updated = m(&d.updated_at, &s.updated_at);
    let created = m(&d.created_at, &s.created_at);
    let repo = m(&d.repo, &s.repo);
    let title = m(&d.title, &s.title);
    let author = m(&d.author, &s.author);
    let assignees = m(&d.assignees, &s.assignees);
    let base = m(&d.base, &s.base);
    let comments = m(&d.num_comments, &s.num_comments);
    let review = m(&d.review_status, &s.review_status);
    let state = m(&d.state, &s.state);
    let ci = m(&d.ci, &s.ci);
    let labels = m(&d.labels, &s.labels);
    let lines = m(&d.lines, &s.lines);
    let mut title_col = col("Title", None, title.is_hidden());
    title_col.grow = true;
    let compact = cfg.theme.ui.table.compact;
    let mut cols = vec![col(icons::STATE_COL, Some(3), state.is_hidden())];
    if compact {
        cols.push(col(icons::REPO, repo.width, repo.is_hidden()));
    }
    cols.push(title_col);
    if compact {
        cols.push(col("Author", author.width, author.is_hidden()));
    }
    cols.extend([
        col(icons::LABELS, labels.width, labels.is_hidden()),
        col("Assignees", assignees.width, assignees.is_hidden()),
        col("Base", base.width, base.is_hidden()),
        col(icons::COMMENTS, Some(4), comments.is_hidden()),
        col(icons::REVIEW_STATUS, Some(4), review.is_hidden()),
        col(icons::CHECKS, Some(" CI ".len()), ci.is_hidden()),
        col(icons::LINES, lines.width, lines.is_hidden()),
        col(icons::UPDATED_AT, updated.width, updated.is_hidden()),
        col(icons::CREATED_AT, created.width, created.is_hidden()),
    ]);
    cols
}

pub fn issue_columns(cfg: &Config, section_layout: &IssuesLayoutConfig) -> Vec<Column> {
    let d = &cfg.defaults.layout.issues;
    let s = section_layout;
    let state = d.state.merged(&s.state);
    let repo = d.repo.merged(&s.repo);
    let title = d.title.merged(&s.title);
    let creator = d.creator.merged(&s.creator);
    let assignees = d.assignees.merged(&s.assignees);
    let comments = d.comments.merged(&s.comments);
    let reactions = d.reactions.merged(&s.reactions);
    let updated = d.updated_at.merged(&s.updated_at);
    let created = d.created_at.merged(&s.created_at);
    let mut title_col = col("Title", None, title.is_hidden());
    title_col.grow = true;
    vec![
        col(icons::ISSUE_OPEN, state.width, state.is_hidden()),
        col(icons::REPO, repo.width, repo.is_hidden()),
        title_col,
        col("Creator", creator.width, creator.is_hidden()),
        col("Assignees", assignees.width, assignees.is_hidden()),
        col(icons::COMMENTS, Some(6), comments.is_hidden()),
        col(icons::REACTIONS, Some(6), reactions.is_hidden()),
        col(icons::UPDATED_AT, updated.width, updated.is_hidden()),
        col(icons::CREATED_AT, created.width, created.is_hidden()),
    ]
}

pub fn notification_columns() -> Vec<Column> {
    vec![
        Column {
            title: "Type".into(),
            width: Some(6),
            grow: false,
            hidden: false,
            align: Alignment::Center,
        },
        Column {
            title: "Title".into(),
            width: None,
            grow: true,
            hidden: false,
            align: Alignment::Left,
        },
        Column {
            title: "Activity".into(),
            width: Some(10),
            grow: false,
            hidden: false,
            align: Alignment::Right,
        },
        Column {
            title: format!("{}    ", icons::UPDATED_AT),
            width: Some(12),
            grow: false,
            hidden: false,
            align: Alignment::Right,
        },
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SortOrder {
    #[default]
    Updated,
    Repo,
}

pub struct Section {
    pub id: usize,
    pub kind: Kind,
    pub title: String,
    pub limit: Option<usize>,
    pub columns: Vec<Column>,
    pub search_value: String,
    pub search: Option<TextInput>,
    pub search_cmp: Option<crate::complete::Completer>,
    pub search_bar: TextInput,
    pub filtered_by_current_remote: bool,
    pub prs: Vec<PrRow>,
    pub issues: Vec<Issue>,
    pub notifs: Vec<NotifRow>,
    pub branches: Vec<BranchRow>,
    pub repo_status: crate::git::NameStatus,
    pub total_count: usize,
    pub page_info: Option<PageInfo>,
    pub last_fetch_task: String,
    pub last_fetch_at: Option<Instant>,
    pub is_loading: bool,
    pub cursor: usize,
    pub top: usize,
    pub prompt: Option<(String, TextInput)>,
    pub last_updated: DateTime<Local>,
    pub sort: SortOrder,
    pub session_read: HashSet<String>,
    pub session_done: HashSet<String>,
}

pub struct RenderCtx<'a> {
    pub cfg: &'a Config,
    pub theme: &'a Theme,
    pub spinner: usize,
}

/// The configured title, filters and page size of a section.
pub struct SectionSpec<'a> {
    pub title: &'a str,
    pub filters: &'a str,
    pub limit: Option<usize>,
}

impl Section {
    pub fn new(
        id: usize,
        kind: Kind,
        spec: SectionSpec,
        columns: Vec<Column>,
        cfg: &Config,
        gh_repo: Option<&(String, String)>,
    ) -> Section {
        let SectionSpec {
            title,
            filters,
            limit,
        } = spec;
        let mut s = Section {
            id,
            kind,
            title: title.to_string(),
            limit,
            columns,
            search_value: filters.to_string(),
            search: None,
            search_cmp: None,
            search_bar: TextInput::new(""),
            filtered_by_current_remote: false,
            prs: vec![],
            issues: vec![],
            notifs: vec![],
            branches: vec![],
            repo_status: Default::default(),
            total_count: 0,
            page_info: None,
            last_fetch_task: String::new(),
            last_fetch_at: None,
            is_loading: false,
            cursor: 0,
            top: 0,
            prompt: None,
            last_updated: Local::now(),
            sort: SortOrder::Updated,
            session_read: HashSet::new(),
            session_done: HashSet::new(),
        };
        if kind == Kind::Notification {
            // Scopes to the current repo when smart filtering is on.
            s.filtered_by_current_remote = cfg.smart_filtering_at_launch;
            s.search_value = s.search_value_for_fetch(gh_repo, false);
        } else {
            // Smart filtering adds the current repo to the filters.
            let mut filters = filters.to_string();
            if cfg.smart_filtering_at_launch
                && let Some((owner, name)) = gh_repo
                && !filters.split_whitespace().any(|t| t.starts_with("repo:"))
            {
                filters = format!("repo:{owner}/{name} {filters}");
            }
            if let Some((owner, name)) = gh_repo {
                let f = format!("repo:{owner}/{name}");
                s.filtered_by_current_remote = filters.split_whitespace().any(|t| t == f);
            }
            s.search_value = filters;
        }
        s.search_bar.set_value(&s.search_value.clone());
        s
    }

    pub fn num_rows(&self) -> usize {
        match self.kind {
            Kind::Pr => self.prs.len(),
            Kind::Issue => self.issues.len(),
            Kind::Notification => self.notifs.len(),
            Kind::Branch => self.filtered_branches().len(),
        }
    }

    /// Branch names containing the search text.
    pub fn filtered_branches(&self) -> Vec<&BranchRow> {
        self.branches
            .iter()
            .filter(|b| b.b.name.contains(&self.search_value))
            .collect()
    }

    pub fn current(&self) -> Option<RowRef> {
        match self.kind {
            Kind::Pr => self
                .prs
                .get(self.cursor)
                .cloned()
                .map(|p| RowRef::Pr(Box::new(p))),
            Kind::Issue => self
                .issues
                .get(self.cursor)
                .cloned()
                .map(|i| RowRef::Issue(Box::new(i))),
            Kind::Notification => self
                .notifs
                .get(self.cursor)
                .cloned()
                .map(|n| RowRef::Notification(Box::new(n))),
            Kind::Branch => self
                .filtered_branches()
                .get(self.cursor)
                .map(|b| RowRef::Branch(Box::new((*b).clone()))),
        }
    }

    pub fn current_notif(&self) -> Option<&NotifRow> {
        self.notifs.get(self.cursor)
    }

    pub fn has_repo_filter(&self) -> bool {
        self.search_value
            .split_whitespace()
            .any(|t| t.starts_with("repo:"))
    }

    pub fn has_current_repo_filter(&self, gh_repo: Option<&(String, String)>) -> bool {
        match gh_repo {
            Some((o, n)) => {
                let f = format!("repo:{o}/{n}");
                self.search_value.split_whitespace().any(|t| t == f)
            }
            None => false,
        }
    }

    /// BaseModel.GetSearchValue: expands templates and adds/removes the current-repo filter.
    pub fn search_value_for_fetch(
        &self,
        gh_repo: Option<&(String, String)>,
        expand: bool,
    ) -> String {
        let value = if expand {
            crate::template::render(&self.search_value, &Default::default())
                .unwrap_or_else(|_| self.search_value.clone())
        } else {
            self.search_value.clone()
        };
        let Some((o, n)) = gh_repo else { return value };
        let f = format!("repo:{o}/{n}");
        let rest: Vec<&str> = value.split_whitespace().filter(|t| *t != f).collect();
        if self.filtered_by_current_remote {
            format!("{f} {}", rest.join(" "))
        } else {
            rest.join(" ")
        }
    }

    pub fn reset_rows(&mut self) {
        self.prs.clear();
        self.issues.clear();
        self.notifs.clear();
        if self.kind == Kind::Notification {
            self.session_read.clear();
            self.session_done.clear();
        }
        self.page_info = None;
        self.cursor = 0;
        self.top = 0;
    }

    // ---- cursor and scrolling ----

    pub fn item_height(&self, cfg: &Config) -> usize {
        let content = match self.kind {
            Kind::Notification => 3,
            _ if cfg.theme.ui.table.compact => 1,
            _ => 2,
        };
        content + usize::from(cfg.theme.ui.table.show_separator)
    }

    pub fn per_page(&self, cfg: &Config, body_height: usize) -> usize {
        let h = self.item_height(cfg);
        body_height.checked_div(h).map_or(0, |n| n.max(1))
    }

    pub fn clamp_cursor(&mut self) {
        let n = self.num_rows();
        if n == 0 {
            self.cursor = 0;
            self.top = 0;
        } else if self.cursor >= n {
            self.cursor = n - 1;
        }
        if self.top > self.cursor {
            self.top = self.cursor;
        }
    }

    pub fn next_row(&mut self, cfg: &Config, body_height: usize) {
        let n = self.num_rows();
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor + 1).min(n - 1);
        let per = self.per_page(cfg, body_height);
        if self.cursor >= self.top + per {
            self.top = self.cursor + 1 - per;
        }
    }

    pub fn prev_row(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        if self.cursor < self.top {
            self.top = self.cursor;
        }
    }

    pub fn first_row(&mut self) {
        self.cursor = 0;
        self.top = 0;
    }

    pub fn last_row(&mut self, cfg: &Config, body_height: usize) {
        let n = self.num_rows();
        if n == 0 {
            return;
        }
        self.cursor = n - 1;
        let per = self.per_page(cfg, body_height);
        self.top = n.saturating_sub(per);
    }

    // ---- notifications ----

    pub fn remove_notification(
        &mut self,
        id: &str,
        cfg: &Config,
        body_height: usize,
    ) -> Option<NotifRow> {
        let i = self.notifs.iter().position(|n| n.id() == id)?;
        let row = self.notifs.remove(i);
        self.session_done.insert(id.to_string());
        self.session_read.remove(id);
        self.total_count = self.notifs.len();
        if !self.notifs.is_empty() && self.cursor >= self.notifs.len() {
            self.last_row(cfg, body_height);
        }
        self.clamp_cursor();
        Some(row)
    }

    pub fn restore_notification(&mut self, row: NotifRow) {
        self.session_done.remove(row.id());
        if !self.notifs.iter().any(|n| n.id() == row.id()) {
            self.notifs.push(row);
            self.sort_notifications();
        }
        self.total_count = self.notifs.len();
        self.clamp_cursor();
    }

    pub fn sort_notifications(&mut self) {
        match self.sort {
            SortOrder::Repo => self.notifs.sort_by(|a, b| {
                a.repo()
                    .cmp(b.repo())
                    .then(b.n.updated_at.cmp(&a.n.updated_at))
            }),
            SortOrder::Updated => self
                .notifs
                .sort_by_key(|n| std::cmp::Reverse(n.n.updated_at)),
        }
    }

    // ---- footer pager ----

    pub fn pager_content(&self) -> String {
        if self.kind == Kind::Branch {
            return String::new();
        }
        if self.total_count == 0 {
            return String::new();
        }
        let pos = self.cursor + 1;
        match self.kind {
            Kind::Branch => String::new(),
            Kind::Pr => {
                let el = time_elapsed(self.last_updated.with_timezone(&chrono::Utc));
                let el = if el == "now" {
                    "just now".to_string()
                } else {
                    format!("~{el} ago")
                };
                format!(
                    "{} Updated {} • {} {}/{} (fetched {})",
                    icons::WAITING,
                    el,
                    self.kind.singular(),
                    pos,
                    self.total_count,
                    self.num_rows()
                )
            }
            Kind::Issue => format!(
                "{} {} • {} {}/{} • Fetched {}",
                icons::WAITING,
                self.last_updated.format("%m/%d %H:%M:%S"),
                self.kind.singular(),
                pos,
                self.total_count,
                self.num_rows()
            ),
            Kind::Notification => format!(
                "{} {} • {} {}/{}",
                icons::WAITING,
                self.last_updated.format("%m/%d %H:%M:%S"),
                self.kind.singular(),
                pos,
                self.total_count
            ),
        }
    }

    /// GetPromptConfirmation text for the current action.
    pub fn prompt_text(&self) -> Option<String> {
        let (action, _) = self.prompt.as_ref()?;
        let what = match self.kind {
            Kind::Pr => "PR",
            Kind::Issue => "issue",
            Kind::Notification | Kind::Branch => "",
        };
        Some(match (self.kind, action.as_str()) {
            (Kind::Pr | Kind::Issue, "close") => {
                format!("Are you sure you want to close this {what}? (y/N) ")
            }
            (Kind::Pr | Kind::Issue, "reopen") => {
                format!("Are you sure you want to reopen this {what}? (y/N) ")
            }
            (Kind::Pr, "ready") => "Are you sure you want to mark this PR as ready? (y/N) ".into(),
            (Kind::Pr, "merge") => "Are you sure you want to merge this PR? (y/N) ".into(),
            (Kind::Pr, "update") => "Are you sure you want to update this PR? (y/N) ".into(),
            (Kind::Pr, "approveWorkflows") => {
                "Are you sure you want to approve all workflows? (y/N) ".into()
            }
            (Kind::Branch, "delete") => {
                "Are you sure you want to delete this branch? (y/N) ".into()
            }
            (Kind::Branch, "new") => "Enter branch name: ".into(),
            (Kind::Branch, "create_pr") => "Enter PR title: ".into(),
            (Kind::Notification, "done_all") => {
                "Are you sure you want to mark all as done? (y/N) ".into()
            }
            _ => String::new(),
        })
    }

    // ---- rendering ----

    /// Section View: search box followed by the table, inside 1 cell of horizontal padding.
    /// Draws the search box and the table of rows.
    pub fn render(&self, rc: &RenderCtx, area: Rect, buf: &mut Buffer) {
        let t = rc.theme;
        let [search_area, table_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)])
                .horizontal_margin(1)
                .areas(area);
        let focused = self.search.is_some();
        let search_box = widgets::Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if focused {
                theme::OPEN_ISSUE
            } else {
                t.primary_border
            }));
        let input_area = search_box.inner(search_area);
        search_box.render(search_area, buf);
        self.search_line(t, input_area.width as usize)
            .render(input_area, buf);
        self.render_table(rc, table_area, buf);
    }

    fn search_line(&self, t: &Theme, width: usize) -> Line<'static> {
        let focused = self.search.is_some();
        let prompt = if self.kind == Kind::Branch {
            format!("{}  ", icons::SEARCH)
        } else {
            format!("{} is:{} ", icons::SEARCH, self.kind.type_name())
        };
        let prompt_w = str_width(&prompt);
        let input = self.search.as_ref().unwrap_or(&self.search_bar);
        let mut value_line = input.view(width.saturating_sub(prompt_w), t.primary(), focused);
        if self.kind == Kind::Branch && input.value().is_empty() {
            // The branch search shows its placeholder while empty.
            let ph = "Search branches...";
            value_line = if focused {
                Line::from(vec![
                    span(&ph[..1], t.faint().add_modifier(Modifier::REVERSED)),
                    span(&ph[1..], t.faint()),
                ])
            } else {
                Line::from(span(ph, t.faint()))
            };
        }
        let mut spans = vec![Span::styled(prompt, t.secondary())];
        spans.extend(value_line.spans);
        Line::from(spans)
    }

    /// Each shown column's width and header cell; grow columns share what the others leave.
    fn header_cells(&self, rc: &RenderCtx, width: usize) -> Vec<(usize, Line<'static>)> {
        let t = rc.theme;
        let title_style = Style::default()
            .fg(t.primary_text)
            .add_modifier(Modifier::BOLD);
        let padded = |title: &str, w: usize| {
            Line::from(vec![
                raw(" "),
                span(truncate_str(title, w.saturating_sub(2), ""), title_style),
                raw(" "),
            ])
        };
        let shown: Vec<&Column> = self.columns.iter().filter(|c| !c.hidden).collect();
        let fixed = |c: &Column| match c.width {
            _ if c.grow => None,
            Some(w) if c.align == Alignment::Right => {
                let a = if str_width(&c.title) <= 2 {
                    Alignment::Center
                } else {
                    Alignment::Right
                };
                Some((
                    w,
                    Line::from(span(c.title.clone(), title_style)).alignment(a),
                ))
            }
            Some(w) => Some((w, padded(&c.title, w))),
            None => Some((str_width(&c.title) + 2, padded(&c.title, usize::MAX))),
        };
        let cells: Vec<_> = shown.iter().map(|c| fixed(c)).collect();
        let taken: usize = cells.iter().flatten().map(|(w, _)| w).sum();
        let grow = shown.iter().filter(|c| c.grow).count();
        let grow_w = width.saturating_sub(taken).checked_div(grow).unwrap_or(0);
        shown
            .iter()
            .zip(cells)
            .map(|(c, cell)| cell.unwrap_or_else(|| (grow_w, padded(&c.title, grow_w))))
            .collect()
    }

    fn render_table(&self, rc: &RenderCtx, area: Rect, buf: &mut Buffer) {
        let t = rc.theme;
        let (widths, header): (Vec<usize>, Vec<Line<'static>>) = self
            .header_cells(rc, area.width as usize)
            .into_iter()
            .unzip();
        let total = widths.iter().sum::<usize>().min(area.width as usize) as u16;
        let header = Row::new(header).bottom_margin(1);
        let constraints = widths.iter().map(|w| Constraint::Length(*w as u16));
        let table = Table::default()
            .widths(constraints)
            .column_spacing(0)
            .header(header)
            .row_highlight_style(Style::default().bg(t.selected_background));
        let [_, body] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);

        if self.num_rows() == 0 {
            Widget::render(
                table,
                Rect {
                    width: total,
                    ..area
                },
                buf,
            );
            if self.is_loading {
                Line::from(vec![
                    span(SPINNER[rc.spinner % SPINNER.len()], t.secondary()),
                    raw("Loading..."),
                ])
                .centered()
                .render(
                    Rect {
                        y: body.y + body.height / 2,
                        height: body.height.min(1),
                        ..body
                    },
                    buf,
                );
            } else if self.last_fetch_at.is_some() {
                Line::from(vec![
                    raw(" "),
                    span(
                        format!(
                            "No {} were found that match the given filters",
                            self.kind.plural()
                        ),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                ])
                .render(body, buf);
            }
            return;
        }

        let item_h = self.item_height(rc.cfg);
        let per = (body.height as usize)
            .checked_div(item_h)
            .map_or(0, |n| n.max(1));
        let content_h = match self.kind {
            Kind::Notification => 3,
            _ if rc.cfg.theme.ui.table.compact => 1,
            _ => 2,
        };
        let separator = rc.cfg.theme.ui.table.show_separator;
        let end = (self.top + per + 1).min(self.num_rows());
        let mut rows = vec![];
        let mut selected = None;
        for i in self.top..end {
            if i == self.cursor {
                selected = Some(rows.len());
            }
            let cells = self.row_cells(i, rc, &widths, i == self.cursor);
            rows.push(self.table_row(cells, &widths, content_h));
            if separator {
                let style = Style::default().fg(t.faint_border);
                rows.push(Row::new(widths.iter().map(|w| span("─".repeat(*w), style))));
            }
        }
        let mut state = TableState::default().with_selected(selected);
        StatefulWidget::render(
            table.rows(rows),
            Rect {
                width: total,
                ..area
            },
            buf,
            &mut state,
        );
    }

    /// One row of cells, each padded by a space on both sides and cut to `content_h` lines.
    fn table_row(&self, cells: Vec<Block>, widths: &[usize], content_h: usize) -> Row<'static> {
        let shown = self.columns.iter().filter(|c| !c.hidden);
        let cells = cells
            .into_iter()
            .zip(widths)
            .zip(shown)
            .map(|((cell, w), c)| {
                let inner_w = w.saturating_sub(2).max(1);
                let lines: Block = if cell.len() > 1 {
                    cell.iter().map(|l| truncate_line(l, inner_w, "")).collect()
                } else {
                    let spans = cell.into_iter().next().map(|l| l.spans).unwrap_or_default();
                    wrap_spans(&spans, inner_w)
                };
                let lines: Vec<Line<'static>> = lines
                    .into_iter()
                    .take(content_h)
                    .map(|l| {
                        let mut spans = vec![raw(" ")];
                        spans.extend(l.spans);
                        spans.push(raw(" "));
                        Line::from(spans).alignment(c.align)
                    })
                    .collect();
                Cell::from(Text::from(lines))
            });
        Row::new(cells).height(content_h as u16)
    }

    fn row_cells(&self, i: usize, rc: &RenderCtx, widths: &[usize], selected: bool) -> Vec<Block> {
        let shown_idx: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.hidden)
            .map(|(i, _)| i)
            .collect();
        let all = match self.kind {
            Kind::Pr => pr_row(&self.prs[i], rc, &self.columns, widths, selected),
            Kind::Issue => issue_row(&self.issues[i], rc),
            Kind::Notification => notification_row(&self.notifs[i], rc),
            Kind::Branch => {
                let rows = self.filtered_branches();
                branch_row(rows[i], rc, &self.columns, widths)
            }
        };
        shown_idx
            .into_iter()
            .map(|i| all.get(i).cloned().unwrap_or_default())
            .collect()
    }
}

fn one(spans: Vec<Span<'static>>) -> Block {
    vec![Line::from(spans)]
}

fn author_with_icon(login: &str, assoc: &str, t: &Theme, show: bool) -> Vec<Span<'static>> {
    let mut v = vec![span(login.to_string(), t.primary())];
    if show {
        let (icon, color) = t.author_role_icon(assoc);
        v.push(span(" ", t.primary()));
        v.push(span(icon, Style::default().fg(color)));
    }
    v
}

fn time_cell(t: chrono::DateTime<chrono::Utc>, cfg: &Config) -> String {
    let fmt = &cfg.defaults.date_format;
    if fmt.is_empty() || fmt == "relative" {
        time_elapsed(t)
    } else {
        crate::util_time::go_format(t, fmt)
    }
}

fn issue_title(
    cfg: &Config,
    t: &Theme,
    state: &str,
    title: &str,
    number: i64,
) -> Vec<Span<'static>> {
    let mut v = vec![];
    if cfg.theme.ui.table.compact {
        let fg = if state != "OPEN" {
            t.faint_text
        } else {
            t.secondary_text
        };
        v.push(span(format!("#{number} "), Style::default().fg(fg)));
    }
    v.push(span(
        title.to_string(),
        t.primary().add_modifier(Modifier::BOLD),
    ));
    v
}

pub fn pr_state_icon(pr: &PullRequest, t: &Theme) -> Span<'static> {
    let (icon, color) = match pr.state.as_str() {
        "OPEN" if pr.is_in_merge_queue => (icons::MERGE_QUEUE, t.warning_text),
        "OPEN" if pr.is_draft => (icons::DRAFT, t.faint_text),
        "OPEN" => (icons::OPEN, theme::OPEN_PR),
        "CLOSED" => (icons::CLOSED, theme::CLOSED_PR),
        "MERGED" => (icons::MERGED, theme::MERGED_PR),
        _ => ("-", t.faint_text),
    };
    span(icon, Style::default().fg(color))
}

/// The lines column: "+adds -dels", unpadded.
pub fn render_lines(pr: &PullRequest, t: &Theme) -> Vec<Span<'static>> {
    vec![
        span(
            format!("+{}", format_number(pr.additions)),
            Style::default().fg(t.success_text),
        ),
        raw(" "),
        span(
            format!("-{}", format_number(pr.deletions.max(0))),
            Style::default().fg(t.error_text),
        ),
    ]
}

fn pr_row(
    row: &PrRow,
    rc: &RenderCtx,
    columns: &[Column],
    widths: &[usize],
    selected: bool,
) -> Vec<Block> {
    let t = rc.theme;
    let cfg = rc.cfg;
    let pr = &row.primary;
    let show_icon = cfg.show_author_icons;

    let review = if pr.review_decision == "APPROVED" {
        span(icons::APPROVED, Style::default().fg(t.success_text))
    } else if pr.review_decision == "CHANGES_REQUESTED" {
        span(icons::CHANGES_REQUESTED, Style::default().fg(t.error_text))
    } else if pr.reviews.total_count > 0 {
        let (g, s) = t.comment_glyph();
        span(g, s)
    } else {
        let (g, s) = t.waiting_glyph();
        span(g, s)
    };
    let rollup = pr
        .commits
        .nodes
        .first()
        .map(|c| c.commit.status_check_rollup.state.as_str())
        .unwrap_or("");
    let ci = match rollup {
        "SUCCESS" => span(icons::SUCCESS, Style::default().fg(t.success_text)),
        "EXPECTED" | "PENDING" => {
            let (g, s) = t.waiting_glyph();
            span(g, s)
        }
        "ERROR" | "FAILURE" => span(icons::FAILURE, Style::default().fg(t.error_text)),
        _ => span(icons::EMPTY, Style::default().fg(t.faint_text)),
    };
    let comments = span(
        (pr.comments.total_count + pr.review_threads.total_count).to_string(),
        t.faint(),
    );
    let assignees = span(
        pr.assignees
            .nodes
            .iter()
            .map(|a| a.login.clone())
            .collect::<Vec<_>>()
            .join(","),
        t.primary(),
    );
    let labels_w = columns
        .iter()
        .filter(|c| !c.hidden)
        .zip(widths)
        .find(|(c, _)| c.title == icons::LABELS)
        .map(|(_, w)| *w)
        .unwrap_or(0);
    let max_rows = if cfg.theme.ui.table.compact { 1 } else { 2 };
    let labels = if labels_w > 2 {
        crate::views::render_labels(&pr.labels.nodes, labels_w - 2, max_rows)
    } else {
        vec![]
    };
    let updated = span(time_cell(pr.updated_at, cfg), t.faint());
    let created = span(time_cell(pr.created_at, cfg), t.faint());

    if !cfg.theme.ui.table.compact {
        // renderExtendedTitle
        let title_w = columns
            .iter()
            .filter(|c| !c.hidden)
            .zip(widths)
            .find(|(c, _)| c.grow)
            .map(|(_, w)| *w)
            .unwrap_or(0)
            .saturating_sub(1);
        let sec = if selected {
            t.secondary()
        } else {
            Style::default()
        };
        let mut top = vec![
            span(
                pr.repository.name_with_owner.clone(),
                Style::default().fg(t.secondary_text),
            ),
            span(
                format!(" #{} by ", pr.number),
                Style::default().fg(t.secondary_text),
            ),
            span("@", sec.add_modifier(Modifier::BOLD)),
        ];
        for s in author_with_icon(&pr.author.login, &pr.author_association, t, show_icon) {
            top.push(Span::styled(
                s.content,
                s.style.add_modifier(Modifier::BOLD),
            ));
        }
        if !cfg.defaults.layout.prs.base.is_hidden() {
            top.push(span(" · ", sec));
            top.push(span(pr.head_ref_name.clone(), sec));
        }
        let top = fit_line(&Line::from(top), title_w, Style::default());
        let title = truncate_line(
            &Line::from(span(
                pr.title.clone(),
                t.primary().add_modifier(Modifier::BOLD),
            )),
            title_w.saturating_sub(1),
            icons::ELLIPSIS,
        );
        return vec![
            one(vec![pr_state_icon(pr, t)]),
            vec![top, title],
            labels,
            one(vec![assignees]),
            one(vec![span(pr.base_ref_name.clone(), t.primary())]),
            one(vec![comments]),
            one(vec![review]),
            one(vec![ci]),
            one(render_lines(pr, t)),
            one(vec![updated]),
            one(vec![created]),
        ];
    }

    vec![
        one(vec![pr_state_icon(pr, t)]),
        one(vec![span(pr.head_repository.name.clone(), t.faint())]),
        one(issue_title(cfg, t, &pr.state, &pr.title, pr.number)),
        one(author_with_icon(
            &pr.author.login,
            &pr.author_association,
            t,
            show_icon,
        )),
        labels,
        one(vec![assignees]),
        one(vec![span(pr.base_ref_name.clone(), t.primary())]),
        one(vec![comments]),
        one(vec![review]),
        one(vec![ci]),
        one(render_lines(pr, t)),
        one(vec![updated]),
        one(vec![created]),
    ]
}

fn issue_row(issue: &Issue, rc: &RenderCtx) -> Vec<Block> {
    let t = rc.theme;
    let cfg = rc.cfg;
    let status = if issue.state == "OPEN" {
        span(icons::ISSUE_OPEN, Style::default().fg(theme::OPEN_ISSUE))
    } else {
        span(icons::ISSUE_CLOSED, t.primary())
    };
    vec![
        one(vec![status]),
        one(vec![span(issue.repository.name.clone(), t.primary())]),
        one(issue_title(
            cfg,
            t,
            &issue.state,
            &issue.title,
            issue.number,
        )),
        one(author_with_icon(
            &issue.author.login,
            &issue.author_association,
            t,
            cfg.show_author_icons,
        )),
        one(vec![span(
            issue
                .assignees
                .nodes
                .iter()
                .map(|a| a.login.clone())
                .collect::<Vec<_>>()
                .join(","),
            t.primary(),
        )]),
        one(vec![span(
            issue.comments.total_count.to_string(),
            t.primary(),
        )]),
        one(vec![span(
            issue.reactions.total_count.to_string(),
            t.primary(),
        )]),
        one(vec![span(time_cell(issue.updated_at, cfg), t.primary())]),
        one(vec![span(time_cell(issue.created_at, cfg), t.primary())]),
    ]
}

fn reason_description(reason: &str, subject_type: &str) -> String {
    match reason {
        "review_requested" => "Review requested".into(),
        "subscribed" => "Activity on subscribed thread".into(),
        "mention" => "You were mentioned".into(),
        "author" => "Activity on your thread".into(),
        "comment" => match subject_type {
            "PullRequest" => "New comment on pull request".into(),
            "Issue" => "New comment on issue".into(),
            _ => "New comment".into(),
        },
        "assign" => "You were assigned".into(),
        "state_change" => "State changed".into(),
        "ci_activity" => "CI activity".into(),
        _ => String::new(),
    }
}

fn notification_row(row: &NotifRow, rc: &RenderCtx) -> Vec<Block> {
    let t = rc.theme;
    let cfg = rc.cfg;
    let fg = |c: Color| Style::default().fg(c);
    let (icon, style) = match row.subject_type() {
        "PullRequest" => match row.subject_state.as_str() {
            "MERGED" => (icons::MERGED.to_string(), fg(theme::MERGED_PR)),
            "CLOSED" => (icons::CLOSED.to_string(), fg(theme::CLOSED_PR)),
            _ if row.is_draft => (icons::DRAFT.to_string(), fg(t.faint_text)),
            _ => (icons::OPEN.to_string(), fg(theme::OPEN_PR)),
        },
        "Issue" if row.subject_state == "CLOSED" => {
            (icons::ISSUE_CLOSED.to_string(), fg(theme::CLOSED_PR))
        }
        "Issue" => (icons::ISSUE_OPEN.to_string(), fg(theme::OPEN_ISSUE)),
        "Discussion" => (icons::DISCUSSION.to_string(), fg(Color::Rgb(255, 255, 255))),
        "Release" => (icons::RELEASE.to_string(), fg(Color::Rgb(0x58, 0xa6, 0xff))),
        "Commit" => (icons::COMMIT_TYPE.to_string(), fg(t.secondary_text)),
        "CheckSuite" => {
            let title = row.title().to_lowercase();
            if title.contains("failed") {
                (icons::FAILURE.to_string(), fg(t.error_text))
            } else if title.contains("succeeded") {
                (icons::SUCCESS.to_string(), fg(t.success_text))
            } else if title.contains("cancelled")
                || title.contains("canceled")
                || title.contains("skipped")
            {
                (icons::WORKFLOW_RUN.to_string(), fg(t.faint_text))
            } else {
                (icons::WORKFLOW_RUN.to_string(), fg(t.warning_text))
            }
        }
        "RepositoryVulnerabilityAlert" => (icons::SECURITY.to_string(), fg(t.error_text)),
        _ => (icons::NOTIFICATION.to_string(), fg(t.secondary_text)),
    };
    let type_cell = vec![
        Line::from(span(icon, style)),
        if row.n.unread {
            Line::from(span(icons::DOT, fg(Color::Indexed(33))))
        } else {
            Line::default()
        },
        Line::default(),
    ];

    let mut line1 = row.repo().to_string();
    if row.number() > 0 {
        line1 = format!("{line1} #{}", row.number());
    }
    let mut l1 = vec![span(line1, t.secondary())];
    if crate::bookmarks::store().has(row.id()) {
        l1.push(span(" ", t.secondary()));
        l1.push(span(icons::BOOKMARK, fg(t.warning_text)));
    }
    let mut title_style = t.primary();
    if row.n.unread {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    let activity = if row.activity.is_empty() {
        reason_description(&row.n.reason, row.subject_type())
    } else {
        row.activity.clone()
    };
    let l3 = if activity.starts_with('@') {
        match activity.find(' ') {
            Some(idx) => vec![
                span(activity[..idx].to_string(), fg(t.actor_text)),
                span(activity[idx..].to_string(), t.faint()),
            ],
            None => vec![span(activity, fg(t.actor_text))],
        }
    } else {
        vec![span(activity, t.faint())]
    };
    let title_cell = vec![
        Line::from(l1),
        Line::from(span(row.title(), title_style)),
        Line::from(l3),
    ];

    let activity_cell = if row.new_comments > 0 {
        vec![
            Line::from(vec![
                span(format!("+{} ", row.new_comments), fg(Color::Indexed(15))),
                span(icons::COMMENTS, fg(Color::Indexed(2))),
            ]),
            Line::default(),
            Line::default(),
        ]
    } else {
        vec![Line::default(), Line::default(), Line::default()]
    };
    let fmt = &cfg.defaults.date_format;
    let updated = if fmt.is_empty() || fmt == "relative" {
        format!("{}\u{a0}ago", time_elapsed(row.n.updated_at))
    } else {
        crate::util_time::go_format(row.n.updated_at, fmt)
    };
    let updated_cell = vec![
        Line::from(span(updated, t.primary())),
        Line::default(),
        Line::default(),
    ];
    vec![type_cell, title_cell, activity_cell, updated_cell]
}

pub fn branch_columns(cfg: &Config) -> Vec<Column> {
    let d = &cfg.defaults.layout.prs;
    let mut title_col = col("Title", None, d.title.is_hidden());
    title_col.grow = true;
    let ci = col(icons::CHECKS, Some(" CI ".len()), d.ci.is_hidden());
    if !cfg.theme.ui.table.compact {
        return vec![
            col(icons::STATE_COL, Some(3), d.state.is_hidden()),
            title_col,
            col("Assignees", d.assignees.width, d.assignees.is_hidden()),
            col("Base", d.base.width, d.base.is_hidden()),
            col(icons::REVIEW_STATUS, Some(4), d.review_status.is_hidden()),
            ci,
            col(icons::LINES, d.lines.width, d.lines.is_hidden()),
            col(
                icons::PENDING_CHECKS,
                d.updated_at.width,
                d.updated_at.is_hidden(),
            ),
        ];
    }
    vec![
        col(icons::STATE_COL, Some(3), d.state.is_hidden()),
        col(icons::REPO, d.repo.width, d.repo.is_hidden()),
        title_col,
        col("Author", d.author.width, d.author.is_hidden()),
        col("Assignees", d.assignees.width, d.assignees.is_hidden()),
        col("Base", d.base.width, d.base.is_hidden()),
        col(icons::REVIEW_STATUS, Some(4), d.review_status.is_hidden()),
        ci,
        col(icons::LINES, d.lines.width, d.lines.is_hidden()),
        col(
            icons::PENDING_CHECKS,
            d.updated_at.width,
            d.updated_at.is_hidden(),
        ),
    ]
}

fn branch_row(row: &BranchRow, rc: &RenderCtx, columns: &[Column], widths: &[usize]) -> Vec<Block> {
    let t = rc.theme;
    let cfg = rc.cfg;
    let pr = row.pr.as_ref();
    let dash = || one(vec![raw("-")]);
    let state = match pr {
        None => one(vec![span(
            icons::NO_DATA,
            Style::default().fg(t.success_text),
        )]),
        Some(p) => {
            let (icon, color) = match p.state.as_str() {
                "OPEN" if p.is_draft => (icons::DRAFT, t.faint_text),
                "OPEN" => (icons::OPEN, theme::OPEN_PR),
                "CLOSED" => (icons::CLOSED, theme::CLOSED_PR),
                "MERGED" => (icons::MERGED, theme::MERGED_PR),
                _ => ("-", t.faint_text),
            };
            one(vec![span(icon, Style::default().fg(color))])
        }
    };
    let review = match pr {
        None => dash(),
        Some(p) if p.review_decision == "APPROVED" => one(vec![span(
            icons::APPROVED,
            Style::default().fg(t.success_text),
        )]),
        Some(p) if p.review_decision == "CHANGES_REQUESTED" => one(vec![span(
            icons::CHANGES_REQUESTED,
            Style::default().fg(t.error_text),
        )]),
        Some(_) => {
            let (g, s) = t.waiting_glyph();
            one(vec![span(g, s)])
        }
    };
    let ci = match pr {
        None => dash(),
        Some(p) => {
            let rollup = p
                .commits
                .nodes
                .first()
                .map(|c| c.commit.status_check_rollup.state.as_str())
                .unwrap_or("UNKNOWN");
            match rollup {
                "SUCCESS" => one(vec![span(
                    icons::SUCCESS,
                    Style::default().fg(t.success_text),
                )]),
                "PENDING" => {
                    let (g, s) = t.waiting_glyph();
                    one(vec![span(g, s)])
                }
                _ => one(vec![span(
                    icons::FAILURE,
                    Style::default().fg(t.error_text),
                )]),
            }
        }
    };
    let lines = match pr {
        None => dash(),
        Some(p) => one(render_lines(p, t)),
    };
    let updated = {
        let at = pr.map(|p| p.updated_at).or(row.b.last_updated_at);
        match at {
            Some(at) => one(vec![span(time_cell(at, cfg), t.faint())]),
            None => one(vec![]),
        }
    };
    let base = one(vec![span(
        pr.map(|p| p.base_ref_name.clone()).unwrap_or_default(),
        t.primary(),
    )]);
    let assignees = one(vec![span(
        pr.map(|p| {
            p.assignees
                .nodes
                .iter()
                .map(|a| a.login.clone())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default(),
        t.primary(),
    )]);

    if !cfg.theme.ui.table.compact {
        // renderExtendedTitle: the branch with ahead/behind counts over its last commit.
        let width = columns
            .iter()
            .filter(|c| !c.hidden)
            .zip(widths)
            .find(|(c, _)| c.grow)
            .map(|(_, w)| *w)
            .unwrap_or(0)
            .saturating_sub(2);
        let name_color = if row.b.is_checked_out {
            t.success_text
        } else {
            t.primary_text
        };
        let mut top = vec![span(row.b.name.clone(), Style::default().fg(name_color))];
        if row.b.commits_ahead > 0 {
            top.push(span(
                format!(" ↑{}", row.b.commits_ahead),
                Style::default().fg(t.warning_text),
            ));
        }
        if row.b.commits_behind > 0 {
            top.push(span(
                format!(" ↓{}", row.b.commits_behind),
                Style::default().fg(t.warning_text),
            ));
        }
        let msg = row.b.last_commit_msg.clone().unwrap_or_else(|| "-".into());
        let msg_line = wrap_spans(&[span(msg, t.secondary())], width.max(1))
            .into_iter()
            .next()
            .unwrap_or_default();
        let title = vec![
            fit_line(&Line::from(top), width, Style::default()),
            msg_line,
        ];
        return vec![state, title, base, assignees, review, ci, lines, updated];
    }
    let (repo, title, author) = match pr {
        Some(p) => (
            one(vec![span(p.head_repository.name.clone(), t.faint())]),
            one(issue_title(cfg, t, &p.state, &p.title, p.number)),
            one(vec![span(p.author.login.clone(), t.primary())]),
        ),
        None => (
            one(vec![]),
            one(vec![span(row.b.name.clone(), t.primary())]),
            one(vec![]),
        ),
    };
    vec![
        state, repo, title, author, base, assignees, review, ci, lines, updated,
    ]
}

/// Added, removed and modified file counts.
pub fn branch_pager(s: &Section, t: &Theme) -> Line<'static> {
    let bg = Style::default().bg(t.selected_background);
    let st = &s.repo_status;
    Line::from(vec![
        span(" ", bg.fg(t.faint_text)),
        span(
            format!("\u{eadc} {}", st.added.len()),
            bg.fg(t.success_text),
        ),
        span(" ", bg),
        span(
            format!("\u{f458} {}", st.removed.len()),
            bg.fg(t.error_text),
        ),
        span(" ", bg),
        span(
            format!("\u{f459} {}", st.modified.len()),
            bg.fg(Color::Rgb(0xe0, 0xaf, 0x68)),
        ),
        span(" ", bg.fg(t.faint_text)),
    ])
}
