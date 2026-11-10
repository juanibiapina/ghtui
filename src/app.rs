//! Application state and update logic: key handling, custom keybindings and
//! background tasks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use chrono::Local;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::bookmarks;
use crate::complete::{Completer, FetchState, SourceKind};
use crate::config::{Config, IssuesSectionConfig, PrsSectionConfig, ViewType};
use crate::github;
use crate::help::Help;
use crate::input::{TextArea, TextInput};
use crate::keys::{Keys, NotificationSubject};
use crate::model::*;
use crate::msg::*;
use crate::section::{
    self, Kind, NotifRow, PrRow, RowRef, Section, SectionSpec, issue_columns, notification_columns,
    pr_columns,
};
use crate::tasks;
use crate::text::Block;
use crate::theme::Theme;
use crate::util;
use crate::views::{self, Editor, EditorMode};

mod live;
use live::Live;

pub const TABS_HEIGHT: usize = 2;
pub const FOOTER_HEIGHT: usize = 1;
const MIN_TABLE_WIDTH_FOR_RIGHT_PREVIEW: usize = 80;
const SPINNER_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskState {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub start_text: String,
    pub finished_text: String,
    pub state: TaskState,
    pub error: Option<String>,
    pub started: Instant,
    pub finished: Option<Instant>,
}

pub struct ExecRequest {
    pub cmd: Command,
    pub on_done: Box<dyn FnOnce(std::io::Result<ExitStatus>) -> Vec<Msg>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Right,
    Bottom,
}

/// The screen regions, in cells.
#[derive(Clone, Copy, Debug)]
pub struct Areas {
    pub position: Position,
    pub tabs: Rect,
    pub sections: Rect,
    pub preview: Option<PreviewAreas>,
    pub footer: Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewAreas {
    /// The rule above a bottom preview, or the column left of a right preview.
    pub rule: Rect,
    pub content: Rect,
    pub pager: Rect,
}

impl PreviewAreas {
    /// The preview without its rule.
    pub fn body(&self) -> Rect {
        self.content.union(self.pager)
    }

    pub fn outer(&self) -> Rect {
        self.rule.union(self.body())
    }
}

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    pub keys: Keys,
    pub pool: Pool,
    pub view: ViewType,
    pub prs: Vec<Section>,
    pub issues: Vec<Section>,
    pub notifs: Vec<Section>,
    /// The repo view's single "Local Branches" section (FF_REPO_VIEW).
    pub repo: Vec<Section>,
    pub repo_url: String,
    pub repo_head: String,
    pub branch_status: Option<crate::git::NameStatus>,
    pub repo_refresh_id: u64,
    pub curr: usize,
    pub sidebar_open: bool,
    pub sidebar_scroll: usize,
    pub sidebar_cache: Option<(usize, Block)>,
    pub position_override: Option<Position>,
    pub help: Option<Help>,
    pub confirm_quit_shown: bool,
    pub error: Option<String>,
    pub tasks: HashMap<String, Task>,
    pub spinner: usize,
    spinner_at: Instant,
    pub user: String,
    pub gh_repo: Option<(String, String)>,
    pub repo_path: Option<PathBuf>,
    pub pr_tab: usize,
    pub summary_more: bool,
    pub pr_editor: Option<Editor>,
    pub issue_editor: Option<Editor>,
    pub subject_id: String,
    pub subject_pr: Option<PrRow>,
    pub subject_issue: Option<Issue>,
    pub pending_notif_action: Option<String>,
    pub notif_prompt: Option<String>,
    pub section_mode: bool,
    pub width: usize,
    pub height: usize,
    pub exec: Option<ExecRequest>,
    pub quit: bool,
    pub timers: Vec<(Instant, Msg)>,
    pub last_refresh: Instant,
    pub version: String,
    pub latest_version: String,
    pub live: Live,
}

/// A config size: a fraction of `total` when between 0 and 1, otherwise cells, capped at `total`.
fn fraction_of(size: f64, total: usize) -> usize {
    let cells = if size > 0.0 && size < 1.0 {
        size * total as f64
    } else {
        size
    };
    (cells as usize).min(total)
}

fn task_id(prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

impl App {
    pub fn new(
        cfg: Config,
        keys: Keys,
        pool: Pool,
        gh_repo: Option<(String, String)>,
        repo_path: Option<PathBuf>,
        width: usize,
        height: usize,
    ) -> App {
        let theme = Theme::from_config(&cfg);
        let view = cfg.defaults.view;
        let sidebar_open = cfg.defaults.preview.open;
        let live = Live::new(&pool);
        let mut app = App {
            theme,
            keys,
            pool,
            view,
            prs: vec![],
            issues: vec![],
            notifs: vec![],
            repo: vec![],
            repo_url: String::new(),
            repo_head: String::new(),
            branch_status: None,
            repo_refresh_id: 0,
            curr: 0,
            sidebar_open,
            sidebar_scroll: 0,
            sidebar_cache: None,
            position_override: None,
            help: None,
            confirm_quit_shown: false,
            error: None,
            tasks: HashMap::new(),
            spinner: 0,
            spinner_at: Instant::now(),
            user: String::new(),
            gh_repo,
            repo_path,
            pr_tab: 0,
            summary_more: false,
            pr_editor: None,
            issue_editor: None,
            subject_id: String::new(),
            subject_pr: None,
            subject_issue: None,
            pending_notif_action: None,
            notif_prompt: None,
            section_mode: false,
            width,
            height,
            exec: None,
            quit: false,
            timers: vec![],
            last_refresh: Instant::now(),
            version: concat!("v", env!("CARGO_PKG_VERSION")).to_string(),
            latest_version: String::new(),
            live,
            cfg,
        };
        app.init_sections();
        app
    }

    fn init_sections(&mut self) {
        let gh = self.gh_repo.clone();
        let cfg = &self.cfg;
        self.prs = cfg
            .pr_sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Section::new(
                    i,
                    Kind::Pr,
                    SectionSpec {
                        title: &s.title,
                        filters: &s.filters,
                        limit: s.limit,
                    },
                    pr_columns(cfg, &s.layout),
                    cfg,
                    gh.as_ref(),
                )
            })
            .collect();
        self.issues = cfg
            .issues_sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Section::new(
                    i,
                    Kind::Issue,
                    SectionSpec {
                        title: &s.title,
                        filters: &s.filters,
                        limit: s.limit,
                    },
                    issue_columns(cfg, &s.layout),
                    cfg,
                    gh.as_ref(),
                )
            })
            .collect();
        self.notifs = cfg
            .notifications_sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Section::new(
                    i,
                    Kind::Notification,
                    SectionSpec {
                        title: &s.title,
                        filters: &s.filters,
                        limit: s.limit,
                    },
                    notification_columns(),
                    cfg,
                    gh.as_ref(),
                )
            })
            .collect();
        if crate::config::feature_enabled("FF_REPO_VIEW") {
            let mut s = Section::new(
                0,
                Kind::Branch,
                SectionSpec {
                    title: "Local Branches",
                    filters: "",
                    limit: None,
                },
                section::branch_columns(cfg),
                cfg,
                None,
            );
            s.search_value.clear();
            s.search_bar.set_value("");
            self.repo = vec![s];
        }
    }

    // ---------------- accessors ----------------

    pub fn sections(&self) -> &Vec<Section> {
        match self.view {
            ViewType::Notifications => &self.notifs,
            ViewType::Prs => &self.prs,
            ViewType::Repo => &self.repo,
            ViewType::Issues => &self.issues,
        }
    }

    fn sections_mut(&mut self) -> &mut Vec<Section> {
        match self.view {
            ViewType::Notifications => &mut self.notifs,
            ViewType::Prs => &mut self.prs,
            ViewType::Repo => &mut self.repo,
            ViewType::Issues => &mut self.issues,
        }
    }

    fn sections_for(&mut self, view: ViewType) -> &mut Vec<Section> {
        match view {
            ViewType::Notifications => &mut self.notifs,
            ViewType::Prs => &mut self.prs,
            ViewType::Repo => &mut self.repo,
            ViewType::Issues => &mut self.issues,
        }
    }

    pub fn section(&self) -> Option<&Section> {
        self.sections().get(self.curr)
    }

    fn section_mut(&mut self) -> Option<&mut Section> {
        let c = self.curr;
        self.sections_mut().get_mut(c)
    }

    pub fn current_row(&self) -> Option<RowRef> {
        self.section().and_then(|s| s.current())
    }

    fn selected_notification_id(&self) -> String {
        match self.current_row() {
            Some(RowRef::Notification(n)) => n.id().to_string(),
            _ => String::new(),
        }
    }

    pub fn notification_subject(&self) -> NotificationSubject {
        if self.subject_pr.is_some() {
            NotificationSubject::Pr
        } else if self.subject_issue.is_some() {
            NotificationSubject::Issue
        } else {
            NotificationSubject::None
        }
    }

    // ---------------- layout ----------------

    fn preview_position(&self) -> Position {
        let preview = &self.cfg.defaults.preview;
        self.position_override
            .unwrap_or_else(|| match preview.position.as_str() {
                "right" => Position::Right,
                "bottom" => Position::Bottom,
                _ => {
                    let pw = fraction_of(preview.width, self.width);
                    if self.width.saturating_sub(pw) < MIN_TABLE_WIDTH_FOR_RIGHT_PREVIEW {
                        Position::Bottom
                    } else {
                        Position::Right
                    }
                }
            })
    }

    pub fn areas(&self) -> Areas {
        use Constraint::{Fill, Length};
        let screen = Rect::new(0, 0, self.width as u16, self.height as u16);
        let [tabs, main, footer] = Layout::vertical([
            Length(self.tabs_height() as u16),
            Fill(1),
            Length(FOOTER_HEIGHT as u16),
        ])
        .areas(screen);
        let position = self.preview_position();
        let mut areas = Areas {
            position,
            tabs,
            sections: main,
            preview: None,
            footer,
        };
        if !self.sidebar_open {
            return areas;
        }
        let preview = &self.cfg.defaults.preview;
        let (sections, rule, body) = match position {
            Position::Bottom => {
                let avail = main.height.saturating_sub(1);
                let ph = fraction_of(preview.height, avail as usize) as u16;
                let [sections, rule, body] =
                    Layout::vertical([Fill(1), Length(1), Length(ph)]).areas(main);
                (sections, rule, body)
            }
            Position::Right => {
                let pw = fraction_of(preview.width, self.width) as u16;
                let [sections, outer] = Layout::horizontal([Fill(1), Length(pw)]).areas(main);
                let [rule, body] = Layout::horizontal([Length(1), Fill(1)]).areas(outer);
                (sections, rule, body)
            }
        };
        let [content, pager] = Layout::vertical([Fill(1), Length(1)]).areas(body);
        areas.sections = sections;
        areas.preview = Some(PreviewAreas {
            rule,
            content,
            pager,
        });
        areas
    }

    pub fn sidebar_content_width(&self) -> usize {
        self.areas().preview.map_or(0, |p| p.content.width as usize)
    }

    pub fn sidebar_viewport_height(&self) -> usize {
        self.areas()
            .preview
            .map_or(0, |p| p.content.height as usize)
    }

    fn body_height(&self) -> usize {
        (self.areas().sections.height as usize).saturating_sub(5)
    }

    /// Rows above the sections: the tabs, or one empty row in the repo view, which has none.
    pub fn tabs_height(&self) -> usize {
        if self.view == ViewType::Repo {
            1
        } else {
            TABS_HEIGHT
        }
    }

    // ---------------- tasks / footer ----------------

    pub fn start_task(&mut self, id: &str, start: &str, finished: &str) {
        crate::info!("Starting task id={id}");
        self.tasks.insert(
            id.to_string(),
            Task {
                start_text: start.into(),
                finished_text: finished.into(),
                state: TaskState::Running,
                error: None,
                started: Instant::now(),
                finished: None,
            },
        );
    }

    fn finish_task(&mut self, id: &str, err: Option<String>) {
        match &err {
            Some(e) => crate::error!("Task finished with error id={id} err={e}"),
            None => crate::info!("Task finished id={id}"),
        }
        if let Some(t) = self.tasks.get_mut(id) {
            t.state = if err.is_some() {
                TaskState::Failed
            } else {
                TaskState::Done
            };
            t.error = err;
            t.finished = Some(Instant::now());
        }
    }

    fn notify(&mut self, text: &str) {
        let id = task_id("notify");
        self.start_task(&id, text, text);
        self.finish_task(&id, None);
    }

    fn notify_err(&mut self, text: &str) {
        let id = task_id("notify");
        self.start_task(&id, text, text);
        self.finish_task(&id, Some(text.to_string()));
    }

    pub fn is_animating(&self) -> bool {
        let cmp_loading =
            |c: Option<&Completer>| c.map(|c| c.fetch == FetchState::Loading).unwrap_or(false);
        self.tasks.values().any(|t| t.state == TaskState::Running)
            || self
                .sections()
                .iter()
                .any(|s| s.is_loading || cmp_loading(s.search_cmp.as_ref()))
            || cmp_loading(self.pr_editor.as_ref().and_then(|e| e.cmp.as_ref()))
            || cmp_loading(self.issue_editor.as_ref().and_then(|e| e.cmp.as_ref()))
    }

    /// Housekeeping run on every tick: spinner, clearing tasks, timers, refetch interval.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.is_animating() && self.spinner_at.elapsed() >= SPINNER_INTERVAL {
            self.spinner = self.spinner.wrapping_add(1);
            self.spinner_at = Instant::now();
            changed = true;
        }
        let before = self.tasks.len();
        self.tasks.retain(|_, t| {
            t.finished
                .map(|f| f.elapsed() < Duration::from_secs(2))
                .unwrap_or(true)
        });
        changed |= before != self.tasks.len();
        let now = Instant::now();
        let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timers)
            .into_iter()
            .partition(|(t, _)| *t <= now);
        self.timers = rest;
        for (_, m) in due {
            self.handle_msg(m);
            changed = true;
        }
        changed |= self.live_tick();
        changed
    }

    // ---------------- fetching ----------------

    /// Full searches for every section of the current view, now.
    fn fetch_all_view_sections(&mut self) {
        self.last_refresh = Instant::now();
        let view = self.view;
        let n = self.sections_for(view).len();
        for i in 0..n {
            let s = &mut self.sections_for(view)[i];
            s.page_info = None;
            s.last_updated = Local::now();
            self.fetch_next_page(view, i);
        }
    }

    pub fn fetch_next_page(&mut self, view: ViewType, id: usize) {
        self.fetch_page(view, id, false);
    }

    /// Fetches a section's next page, or its first after `page_info` was reset.
    /// `background` requests stop while GitHub rate limits us. Returns the task ID.
    fn fetch_page(&mut self, view: ViewType, id: usize, background: bool) -> Option<String> {
        let gh = self.gh_repo.clone();
        let cfg_limit = match view {
            ViewType::Notifications => self.cfg.defaults.notifications_limit,
            ViewType::Issues => self.cfg.defaults.issues_limit,
            _ => self.cfg.defaults.prs_limit,
        };
        let s = self.sections_for(view).get_mut(id)?;
        if let Some(p) = &s.page_info
            && !p.has_next_page
        {
            return None;
        }
        let filters = s.search_value_for_fetch(gh.as_ref(), true);
        let limit = s.limit.unwrap_or(cfg_limit);
        let page = s.page_info.clone();
        let key = SectionKey { view, id };
        let title = s.title.clone();
        let kind = s.kind;

        if kind == Kind::Notification {
            let nf = parse_notification_filters(&filters);
            if nf.is_done {
                s.notifs.clear();
                s.total_count = 0;
                s.is_loading = false;
                s.last_fetch_at = Some(Instant::now());
                self.start_task("done_filter", "Fetching notifications", "");
                self.finish_task(
                    "done_filter",
                    Some("done notifications cannot be retrieved".into()),
                );
                return None;
            }
            let tid = task_id(&format!("fetching_notifications_{id}"));
            s.last_fetch_task = tid.clone();
            s.last_fetch_at = Some(Instant::now());
            s.is_loading = true;
            let session_done = s.session_done.clone();
            let limit = self.cfg.defaults.notifications_limit;
            self.start_task(
                &tid,
                "Fetching notifications",
                "Notifications have been fetched",
            );
            let task = tid.clone();
            self.pool.spawn(move || {
                let fetch = || tasks::fetch_notifications(&nf, limit, page, &session_done);
                let res = if background {
                    github::background(fetch)
                } else {
                    fetch()
                };
                Some(match res {
                    Ok((rows, page_info)) => Msg::TaskFinished {
                        task_id: tid.clone(),
                        section: Some(key),
                        err: None,
                        payload: Payload::Notifications {
                            task_id: tid,
                            rows,
                            page_info,
                        },
                    },
                    Err(e) => Msg::TaskFinished {
                        task_id: tid,
                        section: Some(key),
                        err: Some(e.to_string()),
                        payload: Payload::None,
                    },
                })
            });
            return Some(task);
        }

        let prefix = if kind == Kind::Pr {
            "fetching_prs"
        } else {
            "fetching_issues"
        };
        let tid = task_id(&format!("{prefix}_{id}"));
        s.last_fetch_task = tid.clone();
        s.last_fetch_at = Some(Instant::now());
        s.is_loading = true;
        let (start, done) = if kind == Kind::Pr {
            (
                format!("Fetching PRs for \"{title}\""),
                format!("PRs for \"{title}\" have been fetched"),
            )
        } else {
            (
                format!("Fetching issues for \"{title}\""),
                format!("Issues for \"{title}\" have been fetched"),
            )
        };
        self.start_task(&tid, &start, &done);
        let task = tid.clone();
        self.pool.spawn(move || {
            let run = || {
                if kind == Kind::Pr {
                    github::fetch_pull_requests(&filters, limit, page.as_ref()).map(|p| {
                        Payload::Prs {
                            task_id: tid.clone(),
                            page: p,
                        }
                    })
                } else {
                    github::fetch_issues(&filters, limit, page.as_ref()).map(|p| Payload::Issues {
                        task_id: tid.clone(),
                        page: p,
                    })
                }
            };
            let res = if background {
                github::background(run)
            } else {
                run()
            };
            Some(match res {
                Ok(payload) => Msg::TaskFinished {
                    task_id: tid.clone(),
                    section: Some(key),
                    err: None,
                    payload,
                },
                Err(e) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: Some(e.to_string()),
                    payload: Payload::None,
                },
            })
        });
        Some(task)
    }

    /// CheckSuite notifications link to their workflow run.
    fn fetch_notification_extras(&self, rows: &[NotifRow]) {
        for r in rows.iter().filter(|r| r.subject_type() == "CheckSuite") {
            let id = r.id().to_string();
            let repo = r.repo().to_string();
            let at = r.n.updated_at;
            self.pool.spawn(move || {
                let url =
                    github::background(|| github::fetch_recent_workflow_run(&repo, at)).ok()?;
                if url.is_empty() {
                    return None;
                }
                Some(Msg::NotifUrl { id, url })
            });
        }
    }

    // ---------------- sidebar ----------------

    pub fn invalidate_sidebar(&mut self) {
        self.sidebar_cache = None;
    }

    fn scroll_sidebar_top(&mut self) {
        self.sidebar_scroll = 0;
    }

    fn scroll_sidebar_bottom(&mut self) {
        self.sidebar_scroll = usize::MAX;
    }

    /// onViewedRowChanged
    fn on_viewed_row_changed(&mut self) {
        self.summary_more = false;
        self.pr_tab = 0;
        self.scroll_sidebar_top();
        self.subject_id.clear();
        self.subject_pr = None;
        self.subject_issue = None;
        self.live.subject_wanted = None;
        self.invalidate_sidebar();
        self.enrich_current_row();
        self.schedule_notification_preview();
        if self.view == ViewType::Repo {
            self.refresh_branch_status();
        }
        self.refresh_prefetch();
    }

    /// syncSidebar for notifications: a cached subject shows at once; others load
    /// once the selection rests.
    fn schedule_notification_preview(&mut self) {
        if !self.sidebar_open {
            return;
        }
        if let Some(RowRef::Notification(n)) = self.current_row() {
            if self.subject_id == n.id() {
                return;
            }
            if self.show_cached_subject(&n) {
                return;
            }
            if matches!(n.subject_type(), "PullRequest" | "Issue") {
                self.timers.push((
                    Instant::now() + Duration::from_millis(150),
                    Msg::NotifPreview {
                        notif_id: n.id().to_string(),
                    },
                ));
            }
        }
    }

    fn preview_notification(&mut self) {
        let Some(RowRef::Notification(n)) = self.current_row() else {
            return;
        };
        self.want_subject(&n);
    }

    fn request_quit(&mut self) {
        if self.cfg.confirm_quit {
            self.confirm_quit_shown = true;
        } else {
            self.quit = true;
        }
    }

    // ---------------- messages ----------------

    pub fn handle_msg(&mut self, msg: Msg) {
        self.invalidate_sidebar();
        match msg {
            Msg::TaskFinished {
                task_id,
                section,
                err,
                payload,
            } => {
                self.finish_task(&task_id, err.clone());
                if let Some(key) = section {
                    self.apply_payload(key, payload, err.is_some());
                } else {
                    self.apply_global_payload(payload);
                }
                if self.live.full_running.remove(&task_id) {
                    self.pump_full();
                }
                if let Some(e) = &err {
                    self.note_error(e);
                }
            }
            Msg::PrDetail { url, result } => self.on_pr_detail(url, result),
            Msg::IssueDetail { url, result } => self.on_issue_detail(url, result),
            Msg::CacheLoaded { lists, cache } => self.on_cache_loaded(*lists, cache),
            Msg::DetailsLoaded(d) => self.on_details_loaded(*d),
            Msg::Synced(results) => self.on_synced(results),
            Msg::Checked(Ok(versions)) => self.on_checked(versions),
            Msg::Checked(Err(e)) => self.note_error(&format!("checking PRs failed: {e}")),
            Msg::CheckSelected { url } => self.check_selected(&url),
            Msg::NotifUrl { id, url } => {
                for s in self.notifs.iter_mut() {
                    if let Some(n) = s.notifs.iter_mut().find(|n| n.id() == id) {
                        n.resolved_url = url.clone();
                    }
                }
            }
            Msg::NotifPreview { notif_id } => {
                if notif_id == self.selected_notification_id() && self.subject_id != notif_id {
                    self.preview_notification();
                }
            }
            Msg::Suggestions {
                target,
                users,
                labels,
                err,
            } => {
                let text_pos: Option<(String, (usize, usize))> = match target {
                    CmpTarget::Search(k) => self
                        .sections_for(k.view)
                        .get(k.id)
                        .and_then(|s| s.search.as_ref())
                        .map(|i| (i.value(), crate::input::Editable::cursor_pos(i))),
                    CmpTarget::PrEditor => self
                        .pr_editor
                        .as_ref()
                        .map(|e| (e.area.value(), crate::input::Editable::cursor_pos(&e.area))),
                    CmpTarget::IssueEditor => self
                        .issue_editor
                        .as_ref()
                        .map(|e| (e.area.value(), crate::input::Editable::cursor_pos(&e.area))),
                };
                let failed = err.is_some();
                if let (Some(c), Some((text, pos))) = (self.completer_mut(target), text_pos) {
                    if let Some(u) = users {
                        c.users = u;
                    }
                    if let Some(l) = labels {
                        c.labels = l;
                    }
                    c.filter(&text, pos);
                    c.fetch = match err {
                        Some(e) => FetchState::Error(e),
                        None => FetchState::Success,
                    };
                }
                if failed {
                    self.timers.push((
                        Instant::now() + Duration::from_secs(2),
                        Msg::CmpClearStatus(target),
                    ));
                } else if let Some(c) = self.completer_mut(target) {
                    c.fetch = FetchState::Idle;
                }
            }
            Msg::CmpClearStatus(target) => {
                if let Some(c) = self.completer_mut(target)
                    && matches!(c.fetch, FetchState::Error(_) | FetchState::Success)
                {
                    c.fetch = FetchState::Idle;
                }
            }
            Msg::User(u) => {
                self.user = u;
                self.live.login_at = Some(chrono::Utc::now());
                self.live.dirty = true;
            }
            Msg::RepoTick { id, prs } => {
                if id == self.repo_refresh_id {
                    let cfg = &self.cfg.repo;
                    let secs = if prs {
                        cfg.prs_refetch_interval_seconds
                    } else {
                        cfg.branches_refetch_interval_seconds
                    };
                    if !self.live.focused {
                        // Skipped while unfocused; catch_up syncs on return.
                    } else if prs {
                        self.sync_views(&[ViewType::Repo]);
                    } else {
                        self.read_repo(false);
                    }
                    self.timers.push((
                        Instant::now() + Duration::from_secs(secs),
                        Msg::RepoTick { id, prs },
                    ));
                }
            }
            Msg::BranchStatus(st) => self.branch_status = Some(st),
            Msg::FetchBranchPr { branch, short, key } => self.fetch_branch_pr(branch, short, key),
            Msg::LatestVersion(v) => {
                self.latest_version = v;
                self.live.latest_version_at = Some(chrono::Utc::now());
                self.live.dirty = true;
            }
            Msg::Error(e) => {
                crate::error!("{e}");
                self.error = Some(e)
            }
        }
    }

    fn apply_global_payload(&mut self, payload: Payload) {
        if let Payload::UpdatePr(u) = payload {
            let mut urls = vec![];
            for s in self.prs.iter_mut() {
                urls.extend(pr_urls(s, u.number));
                apply_update_pr(s, &u);
            }
            self.refresh_after_mutation(urls, vec![]);
        }
    }

    fn apply_payload(&mut self, key: SectionKey, payload: Payload, failed: bool) {
        let is_current = key.view == self.view && key.id == self.curr;
        let mut new_notifs: Vec<NotifRow> = vec![];
        let mut row_changed = false;
        let mut refetch = false;
        let mut repo_head: Option<String> = None;
        let mut fill_prs = false;
        let mut mutated_prs: Vec<String> = vec![];
        let mut mutated_issues: Vec<String> = vec![];
        let subject_pr_url = self
            .subject_pr
            .as_ref()
            .map(|p| (p.primary.number, p.primary.url.clone()));
        let subject_issue_url = self
            .subject_issue
            .as_ref()
            .map(|i| (i.number, i.url.clone()));
        {
            let Some(s) = self.sections_for(key.view).get_mut(key.id) else {
                return;
            };
            match payload {
                Payload::None => {
                    if failed {
                        s.is_loading = false;
                    }
                }
                Payload::Prs { task_id, page } => {
                    if s.last_fetch_task == task_id {
                        let rows: Vec<PrRow> = page
                            .items
                            .into_iter()
                            .map(|p| PrRow {
                                primary: p,
                                enriched: None,
                            })
                            .collect();
                        if s.page_info.is_some() {
                            s.prs.extend(rows);
                        } else {
                            s.prs = rows;
                        }
                        s.total_count = page.total_count;
                        s.page_info = Some(page.page_info);
                        s.is_loading = false;
                        s.last_updated = Local::now();
                        s.clamp_cursor();
                        row_changed = true;
                        fill_prs = true;
                    }
                }
                Payload::Issues { task_id, page } => {
                    if s.last_fetch_task == task_id {
                        if s.page_info.is_some() {
                            s.issues.extend(page.items);
                        } else {
                            s.issues = page.items;
                        }
                        s.total_count = page.total_count;
                        s.page_info = Some(page.page_info);
                        s.is_loading = false;
                        s.last_updated = Local::now();
                        s.clamp_cursor();
                        row_changed = true;
                    }
                }
                Payload::Notifications {
                    task_id,
                    rows,
                    page_info,
                } => {
                    if s.last_fetch_task == task_id {
                        let rows: Vec<NotifRow> = rows
                            .into_iter()
                            .filter(|n| !s.session_done.contains(n.id()))
                            .collect();
                        new_notifs = rows.clone();
                        if s.page_info.is_some() {
                            s.notifs.extend(rows);
                        } else {
                            s.notifs = rows;
                        }
                        s.sort_notifications();
                        s.total_count = s.notifs.len();
                        s.page_info = Some(page_info);
                        s.is_loading = false;
                        s.last_updated = Local::now();
                        s.clamp_cursor();
                        row_changed = true;
                    }
                }
                Payload::UpdatePr(u) => {
                    if !failed {
                        mutated_prs.extend(pr_urls(s, u.number));
                        if let Some((n, url)) = &subject_pr_url
                            && *n == u.number
                        {
                            mutated_prs.push(url.clone());
                        }
                    }
                    apply_update_pr(s, &u)
                }
                Payload::UpdateIssue(u) => {
                    if !failed
                        && let Some((n, url)) = &subject_issue_url
                        && *n == u.number
                    {
                        mutated_issues.push(url.clone());
                    }
                    if let Some(i) = s.issues.iter_mut().find(|i| i.number == u.number) {
                        if let Some(c) = u.is_closed {
                            i.state = if c { "CLOSED".into() } else { "OPEN".into() };
                        }
                        if let Some(l) = &u.labels {
                            i.labels.nodes = l.clone();
                        }
                        if let Some(c) = &u.new_comment {
                            i.comments.nodes.push(c.clone());
                        }
                        if let Some(a) = &u.added_assignees {
                            for login in a {
                                if !i.assignees.nodes.iter().any(|x| &x.login == login) {
                                    i.assignees.nodes.push(Login {
                                        login: login.clone(),
                                    });
                                }
                            }
                        }
                        if let Some(r) = &u.removed_assignees {
                            i.assignees.nodes.retain(|x| !r.contains(&x.login));
                        }
                    }
                    s.is_loading = false;
                }
                Payload::NotificationDone(row) => {
                    if failed {
                        s.restore_notification(*row);
                        row_changed = true;
                    }
                }
                Payload::ClearAllNotifications => {
                    s.notifs.clear();
                    s.total_count = 0;
                    s.page_info = None;
                    s.session_done.clear();
                    s.cursor = 0;
                    s.top = 0;
                    refetch = true;
                    row_changed = true;
                }
                Payload::Repo { repo, reset } => {
                    s.branches = repo
                        .branches
                        .iter()
                        .map(|b| section::BranchRow {
                            b: b.clone(),
                            pr: None,
                        })
                        .collect();
                    s.repo_status = repo.status.clone();
                    s.is_loading = false;
                    s.last_updated = Local::now();
                    s.last_fetch_at = Some(Instant::now());
                    if reset {
                        s.cursor = 0;
                        s.top = 0;
                    }
                    s.clamp_cursor();
                    row_changed = true;
                    repo_head = Some(repo.head_branch_name.clone());
                }
                Payload::BranchPrs(prs) => {
                    s.prs = prs
                        .into_iter()
                        .map(|p| PrRow {
                            primary: p,
                            enriched: None,
                        })
                        .collect();
                    s.is_loading = false;
                }
                Payload::BranchPr(pr) => {
                    s.prs.push(PrRow {
                        primary: *pr,
                        enriched: None,
                    });
                }
            }
        }
        if matches!(key.view, ViewType::Notifications) && !new_notifs.is_empty() {
            self.fetch_notification_extras(&new_notifs);
            self.notification_extras(None);
        }
        if fill_prs {
            let store = &self.live.pr_store;
            let section = match key.view {
                ViewType::Prs => self.prs.get_mut(key.id),
                ViewType::Repo => self.repo.get_mut(key.id),
                _ => None,
            };
            if let Some(s) = section {
                live::fill_enriched(&mut s.prs, store);
            }
        }
        if !mutated_prs.is_empty() || !mutated_issues.is_empty() {
            self.refresh_after_mutation(mutated_prs, mutated_issues);
        }
        if row_changed {
            self.live.dirty = true;
        }
        if refetch {
            self.fetch_next_page(key.view, key.id);
        }
        if key.view == ViewType::Repo {
            if let Some(h) = repo_head {
                self.repo_head = h;
            }
            self.relink_branch_prs();
        }
        if is_current && row_changed {
            self.on_viewed_row_changed();
        } else if row_changed {
            self.refresh_prefetch();
        }
    }

    // ---------------- keys ----------------

    pub fn handle_key(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        crate::info!("Key pressed key={key}");
        self.error = None;
        self.invalidate_sidebar();

        if self.section().map(|s| s.search.is_some()).unwrap_or(false) {
            self.handle_search_key(key);
            return;
        }
        if self.section().map(|s| s.prompt.is_some()).unwrap_or(false) {
            self.handle_prompt_key(key);
            return;
        }
        if self.pr_editor.is_some() {
            self.handle_editor_key(key, true);
            return;
        }
        if self.issue_editor.is_some() {
            self.handle_editor_key(key, false);
            return;
        }
        if self.confirm_quit_shown {
            if key == "y" || key == "enter" {
                self.quit = true;
            } else {
                self.confirm_quit_shown = false;
            }
            return;
        }
        if let Some(action) = self.pending_notif_action.take() {
            self.notif_prompt = None;
            if key == "y" || key == "Y" {
                self.execute_notification_action(&action);
            }
            return;
        }
        if let Some(help) = self.help.as_mut() {
            let area = ratatui::layout::Rect::new(0, 0, self.width as u16, self.height as u16);
            if help.handle_key(key, &self.keys, area) {
                self.help = None;
            }
            return;
        }
        if self.is_user_defined_keybinding(key) {
            self.execute_keybinding(key);
            return;
        }
        if self.section_mode {
            if self.keys.u.new_section.matches(key) {
                self.add_new_section();
            }
            if self.keys.u.remove_section.matches(key) {
                self.remove_section();
            }
            self.section_mode = false;
            return;
        }

        let k = self.keys.clone();
        let u = &k.u;
        let body_h = self.body_height();
        let cfg = self.cfg.clone();

        if self.view == ViewType::Repo && k.branch.binds(key) {
            if self.handle_repo_view_key(key) {
                return;
            }
        } else if u.section_mode.matches(key) {
            self.section_mode = true;
        } else if u.prev_section.matches(key) {
            let prev = self.curr.saturating_sub(1);
            if prev < self.sections().len() {
                self.curr = prev;
                self.on_viewed_row_changed();
            }
        } else if u.next_section.matches(key) {
            let n = self.sections().len();
            if n > 0 {
                self.curr = (self.curr + 1).min(n - 1);
                self.on_viewed_row_changed();
            }
        } else if u.down.matches(key) {
            if let Some(s) = self.section_mut() {
                let prev = s.cursor;
                s.next_row(&cfg, body_h);
                let fetch_more =
                    prev != s.cursor && s.cursor + 1 == s.num_rows() && s.kind != Kind::Branch;
                let (view, id) = (self.view, self.curr);
                if fetch_more {
                    self.fetch_next_page(view, id);
                }
                self.on_viewed_row_changed();
            }
        } else if u.up.matches(key) {
            if let Some(s) = self.section_mut() {
                s.prev_row();
                self.on_viewed_row_changed();
            }
        } else if u.first_line.matches(key) {
            if let Some(s) = self.section_mut() {
                s.first_row();
                self.on_viewed_row_changed();
            }
        } else if u.last_line.matches(key) {
            if let Some(s) = self.section_mut() {
                let more = s.cursor + 1 < s.num_rows();
                s.last_row(&cfg, body_h);
                let (view, id) = (self.view, self.curr);
                if more {
                    self.fetch_next_page(view, id);
                }
                self.on_viewed_row_changed();
            }
        } else if u.toggle_preview.matches(key) {
            self.sidebar_open = !self.sidebar_open;
            self.schedule_notification_preview();
        } else if u.toggle_preview_position.matches(key) {
            if self.sidebar_open {
                self.position_override = Some(if self.areas().position == Position::Right {
                    Position::Bottom
                } else {
                    Position::Right
                });
            }
        } else if (u.refresh.matches(key) || u.refresh_all.matches(key))
            && self.view == ViewType::Repo
        {
            self.mark_all_details_stale();
            if let Some(s) = self.section_mut() {
                s.prs.clear();
                s.is_loading = true;
            }
            self.relink_branch_prs();
            self.read_repo(false);
            self.git_fetch();
            self.fetch_branch_prs();
        } else if u.refresh.matches(key) {
            self.mark_all_details_stale();
            let gh = self.gh_repo.clone();
            let (view, id) = (self.view, self.curr);
            if let Some(s) = self.section_mut() {
                s.search_bar
                    .set_value(&s.search_value_for_fetch(gh.as_ref(), false));
                s.reset_rows();
                self.fetch_next_page(view, id);
                self.on_viewed_row_changed();
            }
        } else if u.refresh_all.matches(key) {
            self.mark_all_details_stale();
            self.fetch_all_view_sections();
        } else if u.redraw.matches(key) {
            // The terminal is fully redrawn every frame.
        } else if u.search.matches(key) {
            self.start_search();
            return;
        } else if u.help.matches(key) {
            self.help = Some(Help::new(
                &self.keys,
                self.view,
                self.notification_subject(),
            ));
        } else if u.copy_number.matches(key) {
            match self.current_row() {
                None => self.notify_err("Current selection isn't associated with a PR/Issue"),
                Some(r) => {
                    let n = r.number().to_string();
                    match util::copy_to_clipboard(&n) {
                        Ok(_) => self.notify(&format!("Copied {n} to clipboard")),
                        Err(e) => self.notify_err(&format!("Failed copying to clipboard {e}")),
                    }
                }
            }
            return;
        } else if u.copy_url.matches(key) {
            match self.current_row() {
                None => self.notify_err("Current selection isn't associated with a PR/Issue"),
                Some(r) => {
                    let url = r.url();
                    match util::copy_to_clipboard(&url) {
                        Ok(_) => self.notify(&format!("Copied {url} to clipboard")),
                        Err(e) => self.notify_err(&format!("Failed copying to clipboard {e}")),
                    }
                }
            }
            return;
        } else if u.quit.matches(key) {
            self.request_quit();
            return;
        } else {
            let handled = match self.view {
                ViewType::Prs => self.handle_pr_view_key(key),
                ViewType::Repo => self.handle_repo_view_key(key),
                ViewType::Issues => self.handle_issue_view_key(key),
                ViewType::Notifications => self.handle_notification_view_key(key),
            };
            if handled {
                return;
            }
            if key == "esc" && self.cfg.quit_on_esc {
                self.request_quit();
                return;
            }
        }

        // Page up/down scroll the preview.
        if u.page_down.matches(key) {
            self.sidebar_scroll = self.sidebar_scroll.saturating_add(3);
        } else if u.page_up.matches(key) {
            if self.sidebar_scroll == usize::MAX {
                self.sidebar_scroll = self.sidebar_max_scroll();
            }
            self.sidebar_scroll = self.sidebar_scroll.saturating_sub(3);
        }

        self.section_key(key);
    }

    pub fn sidebar_max_scroll(&self) -> usize {
        let total = self
            .sidebar_cache
            .as_ref()
            .map(|(_, b)| b.len())
            .unwrap_or(0);
        total.saturating_sub(self.sidebar_viewport_height())
    }

    /// Keys that belong to the current section's kind.
    fn section_key(&mut self, key: &str) {
        let k = self.keys.clone();
        let gh = self.gh_repo.clone();
        let (view, id) = (self.view, self.curr);
        let Some(kind) = self.section().map(|s| s.kind) else {
            return;
        };
        let smart = match kind {
            Kind::Pr => k.pr.toggle_smart_filtering.matches(key),
            Kind::Issue => k.issue.toggle_smart_filtering.matches(key),
            Kind::Notification => k.notif.toggle_smart_filtering.matches(key),
            Kind::Branch => false,
        };
        if smart {
            let s = self.section_mut().unwrap();
            if s.has_current_repo_filter(gh.as_ref()) || !s.has_repo_filter() {
                s.filtered_by_current_remote = !s.filtered_by_current_remote;
            }
            let v = s.search_value_for_fetch(gh.as_ref(), false);
            if v != s.search_value {
                s.search_value = v.clone();
                s.search_bar.set_value(&v);
                s.reset_rows();
                self.fetch_next_page(view, id);
                self.on_viewed_row_changed();
            }
            return;
        }
        match kind {
            Kind::Pr => {
                if k.pr.diff.matches(key) {
                    if let Some(RowRef::Pr(pr)) = self.current_row() {
                        self.diff_pr(pr.primary.number, &pr.primary.repository.name_with_owner);
                    }
                } else if k.pr.checkout.matches(key) {
                    if let Some(RowRef::Pr(pr)) = self.current_row() {
                        self.checkout_pr(pr.primary.number, &pr.primary.repository.name_with_owner);
                    }
                } else if k.pr.watch_checks.matches(key)
                    && let Some(RowRef::Pr(pr)) = self.current_row()
                {
                    self.watch_checks(&pr);
                }
            }
            Kind::Issue => {}
            Kind::Notification => self.notification_section_key(key),
            Kind::Branch => self.branch_section_key(key),
        }
    }

    fn notification_section_key(&mut self, key: &str) {
        let k = self.keys.notif.clone();
        let (view, id) = (self.view, self.curr);
        let body_h = self.body_height();
        let cfg = self.cfg.clone();
        let Some(n) = self.section().and_then(|s| s.current_notif().cloned()) else {
            return;
        };
        let nid = n.id().to_string();
        let key_ = SectionKey { view, id };
        if k.mark_as_done.matches(key) {
            let Some(row) = self
                .section_mut()
                .and_then(|s| s.remove_notification(&nid, &cfg, body_h))
            else {
                return;
            };
            self.live.dirty = true;
            self.on_viewed_row_changed();
            let tid = format!("notification_done_{nid}");
            self.start_task(
                &tid,
                "Marking notification as done",
                "Notification marked as done",
            );
            self.pool.spawn(move || {
                let r = tasks::mark_done(&nid);
                Some(Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key_),
                    err: r.err().map(|e| e.to_string()),
                    payload: Payload::NotificationDone(Box::new(row)),
                })
            });
        } else if k.unsubscribe.matches(key) {
            let tid = format!("notification_unsubscribe_{nid}");
            self.start_task(
                &tid,
                "Unsubscribing from thread",
                "Unsubscribed from thread",
            );
            self.pool.spawn(move || {
                let r = github::unsubscribe_thread(&nid);
                Some(Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key_),
                    err: r.err().map(|e| e.to_string()),
                    payload: Payload::None,
                })
            });
        } else if k.toggle_bookmark.matches(key) {
            bookmarks::store().toggle(&nid);
        } else if k.sort_by_repo.matches(key)
            && let Some(s) = self.section_mut()
        {
            s.sort = if s.sort == section::SortOrder::Updated {
                section::SortOrder::Repo
            } else {
                section::SortOrder::Updated
            };
            s.sort_notifications();
        }
    }

    fn handle_search_key(&mut self, key: &str) {
        let (view, id) = (self.view, self.curr);
        let gh = self.gh_repo.clone();
        let s = self.section_mut().unwrap();
        match key {
            "ctrl+c" | "esc" => {
                s.search = None;
                s.search_cmp = None;
            }
            "enter" if s.kind == Kind::Branch => {
                // The branch search filters locally.
                s.search_cmp = None;
                let v = s.search.take().map(|i| i.value()).unwrap_or_default();
                s.search_value = v.clone();
                s.search_bar.set_value(&v);
                s.cursor = 0;
                s.top = 0;
                self.on_viewed_row_changed();
            }
            "enter" => {
                s.search_cmp = None;
                let v = s.search.take().map(|i| i.value()).unwrap_or_default();
                s.search_value = v.clone();
                s.search_bar.set_value(&v);
                s.filtered_by_current_remote = s.has_current_repo_filter(gh.as_ref());
                s.reset_rows();
                self.fetch_next_page(view, id);
                self.on_viewed_row_changed();
            }
            "ctrl+f" => {
                if let Some(c) = s.search_cmp.as_ref()
                    && let Some((o, n)) = c.repo.clone()
                {
                    github::clear_suggestion_cache(&format!("{o}/{n}"));
                    let kind = c.kind;
                    self.load_suggestions(
                        CmpTarget::Search(SectionKey { view, id }),
                        kind,
                        Some((o, n)),
                        true,
                    );
                }
            }
            _ => {
                if let (Some(i), Some(c)) = (s.search.as_mut(), s.search_cmp.as_mut()) {
                    c.handle_key(key, i);
                } else if let Some(i) = s.search.as_mut() {
                    i.handle_key(key);
                }
            }
        }
    }

    /// Autocomplete loads users and labels for a `repo:` in the query.
    fn start_search(&mut self) {
        let (view, id) = (self.view, self.curr);
        let Some(s) = self.section_mut() else { return };
        let value = s.search_bar.value();
        let repo = value.split_whitespace().find_map(|t| {
            let r = t.strip_prefix("repo:")?;
            let mut p = r.split('/');
            Some((p.next()?.to_string(), p.next()?.to_string()))
        });
        let input = TextInput::new(&value);
        let mut cmp = Completer::new(SourceKind::Search, repo.clone(), false);
        cmp.filter(&value, (value.chars().count(), 0));
        cmp.show_completions(&value, (value.chars().count(), 0));
        s.search = Some(input);
        s.search_cmp = Some(cmp);
        if repo.is_some() {
            self.load_suggestions(
                CmpTarget::Search(SectionKey { view, id }),
                SourceKind::Search,
                repo,
                true,
            );
        }
    }

    fn completer_mut(&mut self, target: CmpTarget) -> Option<&mut Completer> {
        match target {
            CmpTarget::Search(k) => self.sections_for(k.view).get_mut(k.id)?.search_cmp.as_mut(),
            CmpTarget::PrEditor => self.pr_editor.as_mut()?.cmp.as_mut(),
            CmpTarget::IssueEditor => self.issue_editor.as_mut()?.cmp.as_mut(),
        }
    }

    fn load_suggestions(
        &mut self,
        target: CmpTarget,
        kind: SourceKind,
        repo: Option<(String, String)>,
        show_loading: bool,
    ) {
        let Some((owner, name)) = repo else { return };
        if show_loading && let Some(c) = self.completer_mut(target) {
            c.fetch = FetchState::Loading;
            c.selected = 0;
            if !c.hidden_by_user && !c.filtered.is_empty() {
                c.visible = true;
            }
        }
        self.pool.spawn(move || {
            let mut err = None;
            let users = if matches!(kind, SourceKind::Search | SourceKind::Mention { .. }) {
                github::fetch_repo_users(&owner, &name)
                    .map_err(|e| err = Some(e.to_string()))
                    .ok()
            } else {
                None
            };
            let labels = if matches!(kind, SourceKind::Search | SourceKind::Labels) {
                github::fetch_repo_labels(&format!("{owner}/{name}"))
                    .map_err(|e| err = Some(e.to_string()))
                    .ok()
            } else {
                None
            };
            Some(Msg::Suggestions {
                target,
                users,
                labels,
                err,
            })
        });
    }

    fn handle_prompt_key(&mut self, key: &str) {
        let (view, id) = (self.view, self.curr);
        let s = self.section_mut().unwrap();
        match key {
            "ctrl+c" | "esc" => s.prompt = None,
            "enter" if s.kind == Kind::Branch => {
                let (action, input) = s.prompt.take().unwrap();
                let v = input.value();
                match action.as_str() {
                    "new" => self.new_branch(&v),
                    "create_pr" => self.create_branch_pr(&v),
                    "delete" if v == "y" || v == "Y" => self.delete_branch(),
                    _ => {}
                }
            }
            "enter" => {
                let (action, input) = s.prompt.take().unwrap();
                let v = input.value();
                if v == "y" || v == "Y" {
                    let row = s.current();
                    let notifs: Vec<String> = s.notifs.iter().map(|n| n.id().to_string()).collect();
                    let key = SectionKey { view, id };
                    match row {
                        Some(RowRef::Pr(pr)) => self.pr_action(&action, &pr, key),
                        Some(RowRef::Issue(i)) => {
                            self.issue_action(&action, i.number, &i.repository.name_with_owner, key)
                        }
                        _ => {}
                    }
                    if action == "done_all" {
                        self.mark_all_done(notifs, key);
                    }
                }
            }
            _ => {
                if let Some((_, i)) = s.prompt.as_mut() {
                    i.handle_key(key);
                }
            }
        }
    }

    fn handle_editor_key(&mut self, key: &str, is_pr: bool) {
        let ed = if is_pr {
            &mut self.pr_editor
        } else {
            &mut self.issue_editor
        };
        match key {
            "esc" | "ctrl+c" => {
                let e = ed.as_mut().unwrap();
                if e.confirm_discard && !e.discarding {
                    e.discarding = true;
                } else {
                    *ed = None;
                }
            }
            "ctrl+d" => {
                let e = ed.take().unwrap();
                self.submit_editor(e, is_pr);
            }
            "ctrl+f" => {
                let target = if is_pr {
                    CmpTarget::PrEditor
                } else {
                    CmpTarget::IssueEditor
                };
                if let Some(c) = ed.as_ref().and_then(|e| e.cmp.as_ref()) {
                    let (kind, repo) = (c.kind, c.repo.clone());
                    if let Some((o, n)) = &repo {
                        github::clear_suggestion_cache(&format!("{o}/{n}"));
                    }
                    self.load_suggestions(target, kind, repo, true);
                }
            }
            _ => {
                let e = ed.as_mut().unwrap();
                if e.discarding {
                    if key == "y" || key == "Y" {
                        *ed = None;
                        return;
                    }
                    e.discarding = false;
                    if key == "n" || key == "N" {
                        return;
                    }
                }
                match e.cmp.as_mut() {
                    Some(c) => {
                        c.handle_key(key, &mut e.area);
                    }
                    None => {
                        e.area.handle_key(key);
                    }
                }
                self.scroll_sidebar_bottom();
            }
        }
    }

    fn editor_target(&self, is_pr: bool) -> Option<RowRef> {
        if self.view == ViewType::Notifications {
            if is_pr {
                return self.subject_pr.clone().map(|p| RowRef::Pr(Box::new(p)));
            }
            return self
                .subject_issue
                .clone()
                .map(|i| RowRef::Issue(Box::new(i)));
        }
        self.current_row()
    }

    fn submit_editor(&mut self, ed: Editor, is_pr: bool) {
        let value = ed.area.value();
        let key = SectionKey {
            view: self.view,
            id: self.curr,
        };
        let Some(row) = self.editor_target(is_pr) else {
            return;
        };
        let (number, repo, labels_now) = match &row {
            RowRef::Pr(p) => (
                p.primary.number,
                p.primary.repository.name_with_owner.clone(),
                p.primary.labels.nodes.clone(),
            ),
            RowRef::Issue(i) => (
                i.number,
                i.repository.name_with_owner.clone(),
                i.labels.nodes.clone(),
            ),
            RowRef::Notification(_) | RowRef::Branch(_) => return,
        };
        let noun = if is_pr { "pr" } else { "issue" };
        let words: Vec<String> = value.split_whitespace().map(String::from).collect();
        match ed.mode {
            EditorMode::Comment => {
                if !value.trim().is_empty() {
                    tasks::comment(self, is_pr, number, &repo, &value, key);
                }
            }
            EditorMode::Approve => tasks::approve(self, number, &repo, value.trim(), key),
            EditorMode::Assign => {
                if !words.is_empty() {
                    tasks::assign(self, noun, number, &repo, words, true, key);
                }
            }
            EditorMode::Unassign => {
                if !words.is_empty() {
                    tasks::assign(self, noun, number, &repo, words, false, key);
                }
            }
            EditorMode::Label => {
                let labels: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !labels.is_empty() || !labels_now.is_empty() {
                    tasks::label(self, noun, number, &repo, labels, &labels_now, key);
                }
            }
        }
    }

    fn open_editor(&mut self, is_pr: bool, mode: EditorMode) {
        let Some(row) = self.editor_target(is_pr) else {
            return;
        };
        let repo = {
            let nwo = row.repo();
            let mut p = nwo.splitn(2, '/');
            match (p.next(), p.next()) {
                (Some(o), Some(n)) => Some((o.to_string(), n.to_string())),
                _ => None,
            }
        };
        let (assignees, labels) = match &row {
            RowRef::Pr(p) => (
                p.primary
                    .assignees
                    .nodes
                    .iter()
                    .map(|a| a.login.clone())
                    .collect::<Vec<_>>(),
                p.primary
                    .labels
                    .nodes
                    .iter()
                    .map(|l| l.name.clone())
                    .collect::<Vec<_>>(),
            ),
            RowRef::Issue(i) => (
                i.assignees.nodes.iter().map(|a| a.login.clone()).collect(),
                i.labels.nodes.iter().map(|l| l.name.clone()).collect(),
            ),
            _ => return,
        };
        let (prompt, initial) = match mode {
            EditorMode::Comment => ("Leave a comment…".to_string(), String::new()),
            EditorMode::Approve => (
                "Approve with comment…".to_string(),
                self.cfg.defaults.pr_approve_comment.clone(),
            ),
            EditorMode::Assign => (
                "Assign users (whitespace-separated)…".to_string(),
                if assignees.contains(&self.user) {
                    String::new()
                } else {
                    self.user.clone()
                },
            ),
            EditorMode::Unassign => (
                "Unassign users (whitespace-separated)…".to_string(),
                assignees.join("\n"),
            ),
            EditorMode::Label => {
                let mut l = labels.clone();
                l.push(String::new());
                (
                    "Add/remove labels (comma-separated)…".to_string(),
                    l.join(", "),
                )
            }
        };
        let cmp = match mode {
            EditorMode::Comment | EditorMode::Approve => Some(Completer::new(
                SourceKind::Mention { with_at: true },
                repo.clone(),
                true,
            )),
            EditorMode::Assign => Some(Completer::new(
                SourceKind::Mention { with_at: false },
                repo.clone(),
                false,
            )),
            EditorMode::Label => Some(Completer::new(SourceKind::Labels, repo.clone(), false)),
            EditorMode::Unassign => None,
        };
        let mut ed = Editor {
            mode,
            prompt,
            area: TextArea::new(&initial),
            confirm_discard: matches!(mode, EditorMode::Comment | EditorMode::Approve),
            discarding: false,
            cmp,
        };
        let kind = ed.cmp.as_ref().map(|c| c.kind);
        if matches!(mode, EditorMode::Assign | EditorMode::Label) {
            let text = ed.area.value();
            let pos = crate::input::Editable::cursor_pos(&ed.area);
            if let Some(c) = ed.cmp.as_mut() {
                c.show_completions(&text, pos);
            }
        }
        let target = if is_pr {
            CmpTarget::PrEditor
        } else {
            CmpTarget::IssueEditor
        };
        if is_pr {
            self.pr_tab = 0;
            self.pr_editor = Some(ed);
        } else {
            self.issue_editor = Some(ed);
        }
        if let Some(kind) = kind {
            self.load_suggestions(target, kind, repo, false);
        }
        self.sidebar_open = true;
        self.scroll_sidebar_bottom();
    }

    fn prompt_confirmation(&mut self, action: &str) {
        if self.current_row().is_none() {
            return;
        }
        if let Some(s) = self.section_mut() {
            s.prompt = Some((action.to_string(), TextInput::new("")));
        }
    }

    fn handle_pr_view_key(&mut self, key: &str) -> bool {
        let p = self.keys.pr.clone();
        if p.prev_sidebar_tab.matches(key) {
            self.pr_tab = self.pr_tab.saturating_sub(1);
            return true;
        }
        if p.next_sidebar_tab.matches(key) {
            self.pr_tab = (self.pr_tab + 1).min(views::PR_TABS.len() - 1);
            return true;
        }
        if self.keys.u.open_github.matches(key) {
            self.open_browser();
            return false;
        }
        let editor = [
            (&p.approve, EditorMode::Approve),
            (&p.assign, EditorMode::Assign),
            (&p.unassign, EditorMode::Unassign),
            (&p.label, EditorMode::Label),
            (&p.comment, EditorMode::Comment),
        ];
        for (b, mode) in editor {
            if b.matches(key) {
                self.open_editor(true, mode);
                return true;
            }
        }
        let confirms = [
            (&p.close, "close"),
            (&p.ready, "ready"),
            (&p.reopen, "reopen"),
            (&p.merge, "merge"),
            (&p.update, "update"),
            (&p.approve_workflows, "approveWorkflows"),
        ];
        for (b, action) in confirms {
            if b.matches(key) {
                self.prompt_confirmation(action);
                return true;
            }
        }
        if p.view_issues.matches(key) {
            self.switch_selected_view();
            return false;
        }
        if p.summary_view_more.matches(key) {
            self.summary_more = true;
            return true;
        }
        false
    }

    fn handle_issue_view_key(&mut self, key: &str) -> bool {
        let i = self.keys.issue.clone();
        if self.keys.u.open_github.matches(key) {
            self.open_browser();
            return false;
        }
        let editor = [
            (&i.label, EditorMode::Label),
            (&i.assign, EditorMode::Assign),
            (&i.unassign, EditorMode::Unassign),
            (&i.comment, EditorMode::Comment),
        ];
        for (b, mode) in editor {
            if b.matches(key) {
                self.open_editor(false, mode);
                return true;
            }
        }
        if i.checkout.matches(key) {
            if let Some(RowRef::Issue(issue)) = self.current_row() {
                self.checkout_issue(issue.number, &issue.repository.name_with_owner);
            }
            return true;
        }
        if i.close.matches(key) {
            self.prompt_confirmation("close");
            return true;
        }
        if i.reopen.matches(key) {
            self.prompt_confirmation("reopen");
            return true;
        }
        if i.view_prs.matches(key) {
            self.switch_selected_view();
            return false;
        }
        false
    }

    fn handle_notification_view_key(&mut self, key: &str) -> bool {
        let k = self.keys.clone();
        if k.u.open_github.matches(key) {
            self.open_browser();
            return true;
        }
        if self.subject_pr.is_some() {
            let p = &k.pr;
            let editor = [
                (&p.approve, EditorMode::Approve),
                (&p.assign, EditorMode::Assign),
                (&p.unassign, EditorMode::Unassign),
                (&p.label, EditorMode::Label),
                (&p.comment, EditorMode::Comment),
            ];
            for (b, mode) in editor {
                if b.matches(key) {
                    self.open_editor(true, mode);
                    return true;
                }
            }
            if p.diff.matches(key) {
                let pr = self.subject_pr.clone().unwrap();
                self.diff_pr(pr.primary.number, &pr.primary.repository.name_with_owner);
                return true;
            }
            if p.checkout.matches(key) {
                let pr = self.subject_pr.clone().unwrap();
                self.checkout_pr(pr.primary.number, &pr.primary.repository.name_with_owner);
                return true;
            }
            let confirms = [
                (&p.close, "close"),
                (&p.ready, "ready"),
                (&p.reopen, "reopen"),
                (&p.merge, "merge"),
                (&p.update, "update"),
                (&p.approve_workflows, "approveWorkflows"),
            ];
            for (b, action) in confirms {
                if b.matches(key) {
                    let n = self.subject_pr.as_ref().unwrap().primary.number;
                    let display = match action {
                        "ready" => "mark as ready",
                        "approveWorkflows" => "approve all workflows for",
                        a => a,
                    };
                    self.pending_notif_action = Some(format!("pr_{action}"));
                    self.notif_prompt =
                        Some(format!("Are you sure you want to {display} PR #{n}? (y/N)"));
                    return true;
                }
            }
            if p.summary_view_more.matches(key) {
                self.summary_more = true;
                return true;
            }
            if p.view_issues.matches(key) {
                self.switch_selected_view();
                return true;
            }
            if p.prev_sidebar_tab.matches(key) {
                self.pr_tab = self.pr_tab.saturating_sub(1);
                return true;
            }
            if p.next_sidebar_tab.matches(key) {
                self.pr_tab = (self.pr_tab + 1).min(views::PR_TABS.len() - 1);
                return true;
            }
            return false;
        }
        if self.subject_issue.is_some() {
            let i = &k.issue;
            let editor = [
                (&i.label, EditorMode::Label),
                (&i.assign, EditorMode::Assign),
                (&i.unassign, EditorMode::Unassign),
                (&i.comment, EditorMode::Comment),
            ];
            for (b, mode) in editor {
                if b.matches(key) {
                    self.open_editor(false, mode);
                    return true;
                }
            }
            if i.checkout.matches(key) {
                let issue = self.subject_issue.clone().unwrap();
                self.checkout_issue(issue.number, &issue.repository.name_with_owner);
                return true;
            }
            for (b, action) in [(&i.close, "close"), (&i.reopen, "reopen")] {
                if b.matches(key) {
                    let n = self.subject_issue.as_ref().unwrap().number;
                    self.pending_notif_action = Some(format!("issue_{action}"));
                    self.notif_prompt = Some(format!(
                        "Are you sure you want to {action} Issue #{n}? (y/N)"
                    ));
                    return true;
                }
            }
            if i.view_prs.matches(key) {
                self.switch_selected_view();
                return true;
            }
            return false;
        }
        if k.notif.mark_all_as_done.matches(key) {
            if let Some(s) = self.section_mut() {
                s.prompt = Some(("done_all".into(), TextInput::new("")));
            }
            return true;
        }
        if k.pr.view_issues.matches(key) {
            self.switch_selected_view();
            return true;
        }
        false
    }

    fn execute_notification_action(&mut self, action: &str) {
        let key = SectionKey {
            view: self.view,
            id: self.curr,
        };
        if let Some(a) = action.strip_prefix("pr_") {
            if let Some(pr) = self.subject_pr.clone() {
                self.pr_action(a, &pr, key);
            }
        } else if let Some(a) = action.strip_prefix("issue_")
            && let Some(i) = self.subject_issue.clone()
        {
            self.issue_action(a, i.number, &i.repository.name_with_owner, key);
        }
    }

    fn pr_action(&mut self, action: &str, pr: &PrRow, key: SectionKey) {
        let n = pr.primary.number;
        let repo = pr.primary.repository.name_with_owner.clone();
        match action {
            "close" => tasks::simple_pr(self, "close", n, &repo, key),
            "reopen" => tasks::simple_pr(self, "reopen", n, &repo, key),
            "ready" => tasks::simple_pr(self, "ready", n, &repo, key),
            "update" => tasks::simple_pr(self, "update-branch", n, &repo, key),
            "merge" => tasks::merge(self, n, &repo, key),
            "approveWorkflows" => tasks::approve_workflows(self, n, &repo, key),
            _ => {}
        }
    }

    fn issue_action(&mut self, action: &str, number: i64, repo: &str, key: SectionKey) {
        match action {
            "close" | "reopen" => tasks::simple_issue(self, action, number, repo, key),
            _ => {}
        }
    }

    fn mark_all_done(&mut self, ids: Vec<String>, key: SectionKey) {
        if ids.is_empty() {
            return;
        }
        let tid = "notification_done_all".to_string();
        let count = ids.len();
        self.start_task(
            &tid,
            &format!("Marking {count} notifications as done"),
            &format!("{count} notifications marked as done"),
        );
        self.pool.spawn_detached(move || {
            let mut last_err = None;
            for id in &ids {
                if let Err(e) = tasks::mark_done(id) {
                    last_err = Some(e.to_string());
                }
            }
            let failed = last_err.is_some();
            Some(Msg::TaskFinished {
                task_id: tid,
                section: Some(key),
                err: last_err,
                payload: if failed {
                    Payload::None
                } else {
                    Payload::ClearAllNotifications
                },
            })
        });
    }

    // ---------------- view switching / sections ----------------

    fn switch_selected_view(&mut self) {
        if self.view == ViewType::Notifications {
            self.subject_pr = None;
            self.subject_issue = None;
            self.subject_id.clear();
        }
        // With FF_REPO_VIEW the cycle passes through the branches view.
        let repo_ff = crate::config::feature_enabled("FF_REPO_VIEW");
        self.view = match self.view {
            ViewType::Notifications => ViewType::Prs,
            ViewType::Prs => ViewType::Issues,
            ViewType::Issues if repo_ff => ViewType::Repo,
            _ => ViewType::Notifications,
        };
        self.curr = 0;
        let view = self.view;
        if view == ViewType::Repo {
            self.start_repo_view();
        }
        self.queue_unfetched(view);
        if self
            .live
            .last_sync
            .get(&view)
            .is_none_or(|t| t.elapsed() >= live::SYNC_ON_SWITCH)
        {
            self.sync_views(&[view]);
            if view == ViewType::Notifications {
                self.queue_notifications_if_due(false);
            }
        }
        self.pump_full();
        self.change_check(view);
        self.on_viewed_row_changed();
    }

    fn add_new_section(&mut self) {
        let gh = self.gh_repo.clone();
        let cfg = self.cfg.clone();
        let view = self.view;
        let id = self.sections().len();
        let s = match view {
            ViewType::Issues => Section::new(
                id,
                Kind::Issue,
                SectionSpec {
                    title: "Scratch",
                    filters: "archived:false",
                    limit: None,
                },
                issue_columns(&cfg, &IssuesSectionConfig::default().layout),
                &cfg,
                gh.as_ref(),
            ),
            ViewType::Notifications => Section::new(
                id,
                Kind::Notification,
                SectionSpec {
                    title: "Scratch",
                    filters: "archived:false",
                    limit: None,
                },
                notification_columns(),
                &cfg,
                gh.as_ref(),
            ),
            _ => Section::new(
                id,
                Kind::Pr,
                SectionSpec {
                    title: "Scratch",
                    filters: "archived:false",
                    limit: None,
                },
                pr_columns(&cfg, &PrsSectionConfig::default().layout),
                &cfg,
                gh.as_ref(),
            ),
        };
        // A scratch section isn't fetched until refreshed, and shows its empty state.
        let mut s = s;
        s.last_fetch_at = Some(Instant::now());
        self.sections_mut().push(s);
        self.curr = id;
        self.on_viewed_row_changed();
    }

    fn remove_section(&mut self) {
        let n = self.sections().len();
        if n <= 1 {
            return;
        }
        let del = self.curr;
        let secs = self.sections_mut();
        secs.remove(del);
        for (i, s) in secs.iter_mut().enumerate() {
            s.id = i;
        }
        let n = self.sections().len();
        self.curr = del.min(n - 1);
        self.on_viewed_row_changed();
    }

    // ---------------- custom keybindings ----------------

    fn is_user_defined_keybinding(&self, key: &str) -> bool {
        let kb = &self.cfg.keybindings;
        let custom = |list: &Vec<crate::config::Keybinding>| {
            list.iter().any(|b| b.builtin.is_empty() && b.key == key)
        };
        if custom(&kb.universal) {
            return true;
        }
        match self.view {
            ViewType::Issues => custom(&kb.issues),
            ViewType::Prs => custom(&kb.prs),
            ViewType::Repo => custom(&kb.branches),
            ViewType::Notifications => {
                if custom(&kb.notifications) {
                    return true;
                }
                match self.current_row() {
                    Some(RowRef::Notification(n)) => match n.subject_type() {
                        "PullRequest" => custom(&kb.prs),
                        "Issue" => custom(&kb.issues),
                        _ => false,
                    },
                    _ => false,
                }
            }
        }
    }

    fn execute_keybinding(&mut self, key: &str) {
        let kb = self.cfg.keybindings.clone();
        if let Some(b) = kb.universal.iter().find(|b| b.key == key) {
            let mut vars = HashMap::new();
            if let Some(p) = &self.repo_path {
                vars.insert("RepoPath".to_string(), p.display().to_string());
            }
            self.run_custom_command(&b.command, vars);
            return;
        }
        let row = self.current_row();
        let find = |list: &Vec<crate::config::Keybinding>| {
            list.iter()
                .find(|b| b.key == key && !b.command.is_empty())
                .cloned()
        };
        let s = |v: &str| v.to_string();
        match (self.view, row) {
            (ViewType::Issues, Some(RowRef::Issue(i))) => {
                if let Some(b) = kb.issues.iter().find(|b| b.key == key) {
                    let vars = HashMap::from([
                        (s("RepoName"), i.repository.name_with_owner.clone()),
                        (s("IssueNumber"), i.number.to_string()),
                        (s("IssueTitle"), i.title.clone()),
                        (s("Author"), i.author.login.clone()),
                    ]);
                    self.run_custom_command(&b.command, vars);
                }
            }
            (ViewType::Prs, Some(RowRef::Pr(p))) => {
                if let Some(b) = find(&kb.prs) {
                    let vars = HashMap::from([
                        (s("RepoName"), p.primary.repository.name_with_owner.clone()),
                        (s("PrNumber"), p.primary.number.to_string()),
                        (s("HeadRefName"), p.primary.head_ref_name.clone()),
                        (s("BaseRefName"), p.primary.base_ref_name.clone()),
                        (s("Author"), p.primary.author.login.clone()),
                    ]);
                    self.run_custom_command(&b.command, vars);
                }
            }
            (ViewType::Repo, Some(RowRef::Branch(b))) => {
                if let Some(binding) = find(&kb.branches) {
                    let mut vars = HashMap::from([(s("HeadRefName"), b.b.name.clone())]);
                    if let Some(p) = &self.repo_path {
                        vars.insert(s("RepoPath"), p.display().to_string());
                    }
                    if let Some(pr) = &b.pr {
                        vars.insert(s("RepoName"), pr.repository.name_with_owner.clone());
                        vars.insert(s("PrNumber"), pr.number.to_string());
                        vars.insert(s("BaseRefName"), pr.base_ref_name.clone());
                        vars.insert(s("Author"), pr.author.login.clone());
                    }
                    self.run_custom_command(&binding.command, vars);
                }
            }
            (ViewType::Notifications, Some(RowRef::Notification(n))) => {
                if let Some(b) = find(&kb.notifications) {
                    let vars = HashMap::from([
                        (s("RepoName"), n.repo().to_string()),
                        (s("Number"), n.number().to_string()),
                    ]);
                    self.run_custom_command(&b.command, vars);
                    return;
                }
                match n.subject_type() {
                    "PullRequest" => {
                        if let Some(b) = find(&kb.prs) {
                            let mut vars = HashMap::from([
                                (s("RepoName"), n.repo().to_string()),
                                (s("PrNumber"), n.number().to_string()),
                            ]);
                            if let Some(pr) = &self.subject_pr {
                                vars.insert(s("HeadRefName"), pr.primary.head_ref_name.clone());
                                vars.insert(s("BaseRefName"), pr.primary.base_ref_name.clone());
                                vars.insert(s("Author"), pr.primary.author.login.clone());
                            }
                            self.run_custom_command(&b.command, vars);
                        }
                    }
                    "Issue" => {
                        if let Some(b) = find(&kb.issues) {
                            let mut vars = HashMap::from([
                                (s("RepoName"), n.repo().to_string()),
                                (s("IssueNumber"), n.number().to_string()),
                            ]);
                            if let Some(i) = &self.subject_issue {
                                vars.insert(s("Author"), i.author.login.clone());
                            }
                            self.run_custom_command(&b.command, vars);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// resolveTemplateInput + runCustomCommand
    fn run_custom_command(&mut self, template: &str, mut vars: HashMap<String, String>) {
        if let Some(repo) = vars.get("RepoName").cloned()
            && let Some(p) = util::repo_local_path(&repo, &self.cfg.repo_paths)
        {
            vars.insert("RepoPath".into(), p);
        }
        if !vars.contains_key("RepoPath")
            && let Some(p) = &self.repo_path
        {
            vars.insert("RepoPath".into(), p.display().to_string());
        }
        let cmd = match crate::template::render(template, &vars) {
            Ok(c) => c,
            Err(_) => {
                self.error = Some(format!("failed to parsetemplate {template}"));
                return;
            }
        };
        let shown = cmd.clone();
        let (view, id) = (self.view, self.curr);
        let key = SectionKey { view, id };
        self.exec = Some(ExecRequest {
            cmd: util::shell_command(&cmd),
            on_done: Box::new(move |res| match res {
                Ok(st) if st.success() => vec![Msg::TaskFinished {
                    task_id: String::new(),
                    section: Some(key),
                    err: None,
                    payload: Payload::None,
                }],
                Ok(st) => vec![Msg::Error(format!(
                    "Whoops, got an error: {st}\nWhile running: {shown}"
                ))],
                Err(e) => vec![Msg::Error(format!(
                    "Whoops, got an error: {e}\nWhile running: {shown}"
                ))],
            }),
        });
    }

    // ---------------- external commands ----------------

    pub fn open_browser(&mut self) {
        let id = task_id("open_browser");
        self.start_task(&id, "Opening in browser", "Opened in browser");
        let url = self.current_row().map(|r| r.url());
        self.pool.spawn_detached(move || {
            let err = match url {
                None => Some("current selection doesn't have a URL".to_string()),
                Some(u) => util::open_browser(&u).err().map(|e| e.to_string()),
            };
            Some(Msg::TaskFinished {
                task_id: id,
                section: None,
                err,
                payload: Payload::None,
            })
        });
    }

    fn diff_pr(&mut self, number: i64, repo: &str) {
        let mut cmd = Command::new("gh");
        cmd.args(["pr", "diff", &number.to_string(), "-R", repo]);
        for (k, v) in self.cfg.diff_pager_env() {
            cmd.env(k, v);
        }
        self.exec = Some(ExecRequest {
            cmd,
            on_done: Box::new(|res| match res {
                Err(e) => vec![Msg::Error(e.to_string())],
                Ok(st) if !st.success() => vec![Msg::Error(format!("gh pr diff: {st}"))],
                _ => vec![],
            }),
        });
    }

    fn checkout_pr(&mut self, number: i64, repo: &str) {
        let Some(path) = util::repo_local_path(repo, &self.cfg.repo_paths) else {
            self.error = Some(
                "local path to repo not specified, set one in your config.yml under repoPaths"
                    .into(),
            );
            return;
        };
        let path = util::expand_home(&path);
        let tid = format!("checkout_{number}");
        self.start_task(
            &tid,
            &format!("Checking out PR #{number}"),
            &format!("PR #{number} has been checked out at {path}"),
        );
        self.pool.spawn_detached(move || {
            let err = tasks::run_gh(&["pr", "checkout", &number.to_string()], Some(&path)).err();
            Some(Msg::TaskFinished {
                task_id: tid,
                section: None,
                err,
                payload: Payload::None,
            })
        });
    }

    fn checkout_issue(&mut self, number: i64, repo: &str) {
        let Some(path) = util::repo_local_path(repo, &self.cfg.repo_paths) else {
            self.error = Some(
                "local path to repo not specified, set one in your config.yml under repoPaths"
                    .into(),
            );
            return;
        };
        let path = util::expand_home(&path);
        let tid = format!("issue_checkout_{number}");
        self.start_task(
            &tid,
            &format!("Checking out branch for issue #{number}"),
            &format!("Branch for issue #{number} has been checked out at {path}"),
        );
        let repo = repo.to_string();
        self.pool.spawn_detached(move || {
            let err = tasks::run_gh(
                &[
                    "issue",
                    "develop",
                    &number.to_string(),
                    "-R",
                    &repo,
                    "--checkout",
                ],
                Some(&path),
            )
            .err();
            Some(Msg::TaskFinished {
                task_id: tid,
                section: None,
                err,
                payload: Payload::None,
            })
        });
    }

    fn watch_checks(&mut self, pr: &PrRow) {
        let n = pr.primary.number;
        let repo = pr.primary.repository.name_with_owner.clone();
        let title = pr.primary.title.clone();
        let rollup = pr
            .primary
            .commits
            .nodes
            .first()
            .map(|c| c.commit.status_check_rollup.state.clone())
            .unwrap_or_default();
        let tid = format!("pr_watch_{n}");
        self.start_task(
            &tid,
            &format!("Watching checks for PR #{n}"),
            &format!("Watching checks for PR #{n}"),
        );
        self.finish_task(&tid, None);
        self.pool.spawn_detached(move || {
            let _ = tasks::run_gh(
                &[
                    "pr",
                    "checks",
                    "--watch",
                    "--fail-fast",
                    &n.to_string(),
                    "-R",
                    &repo,
                ],
                None,
            );
            let status = match rollup.as_str() {
                "SUCCESS" => "✅ Checks have passed",
                "FAILURE" => "❌ Checks have failed",
                _ => &format!("{} Checks are pending", crate::icons::PENDING_CHECKS),
            };
            tasks::desktop_notify(
                &format!("ghtui: {title}"),
                &format!("PR #{n} in {repo}\n{status}"),
            );
            None
        });
    }

    // ---------------- sidebar content ----------------

    pub fn sidebar_content(&self) -> Block {
        let width = self.sidebar_content_width();
        let Some(row) = self.current_row() else {
            return vec![];
        };
        let t = &self.theme;
        let approve_key = self
            .keys
            .pr
            .approve_workflows
            .keys
            .first()
            .cloned()
            .unwrap_or_default();
        let pr_state = views::PrViewState {
            tab: self.pr_tab,
            summary_more: self.summary_more,
            editor: self.pr_editor.as_ref(),
            approve_workflows_key: approve_key,
        };
        match row {
            RowRef::Pr(pr) => views::pr_view(t, &pr, width, &pr_state),
            RowRef::Issue(i) => views::issue_view(t, &i, width, self.issue_editor.as_ref()),
            RowRef::Notification(n) => {
                if self.subject_id == n.id() {
                    if let Some(pr) = &self.subject_pr {
                        return views::pr_view(t, pr, width, &pr_state);
                    }
                    if let Some(i) = &self.subject_issue {
                        return views::issue_view(t, i, width, self.issue_editor.as_ref());
                    }
                }
                if matches!(n.subject_type(), "PullRequest" | "Issue") {
                    views::notification_loading(t, &n, width)
                } else {
                    views::notification_prompt(t, &n)
                }
            }
            RowRef::Branch(b) => {
                let preview_w = self.areas().preview.map_or(0, |p| p.outer().width);
                let rule_w = (preview_w as usize).saturating_sub(5);
                views::branch_view(t, &b, self.branch_status.as_ref(), rule_w)
            }
        }
    }

    pub fn ensure_sidebar_cache(&mut self) {
        let width = self.sidebar_content_width();
        let valid = matches!(&self.sidebar_cache, Some((w, _)) if *w == width);
        if !valid {
            let content = self.sidebar_content();
            self.sidebar_cache = Some((width, content));
        }
        let max = self.sidebar_max_scroll();
        if self.sidebar_scroll > max {
            self.sidebar_scroll = max;
        }
    }

    pub fn on_resize(&mut self, w: usize, h: usize) {
        self.width = w;
        self.height = h;
        let pos = self.cfg.defaults.preview.position.as_str();
        if pos == "auto" || pos.is_empty() {
            self.position_override = None;
        }
        self.invalidate_sidebar();
    }

    /// After an external command: sync the current view, which may have changed.
    pub fn after_exec(&mut self) {
        let view = self.view;
        self.sync_views(&[view]);
        if view == ViewType::Notifications {
            self.queue_notifications_if_due(false);
            self.pump_full();
        }
    }
}

// ---------------- repo view ----------------

impl App {
    fn repo_dir(&self) -> Option<String> {
        self.repo_path.as_ref().map(|p| p.display().to_string())
    }

    /// Starts the branches view once: reads the repo, fetches origin, syncs its PRs,
    /// and schedules refreshes.
    fn start_repo_view(&mut self) {
        if self.live.repo_started || self.repo.is_empty() || self.repo_path.is_none() {
            return;
        }
        self.live.repo_started = true;
        let cfg = self.cfg.clone();
        self.read_repo(false);
        self.git_fetch();
        self.sync_views(&[ViewType::Repo]);
        self.repo_refresh_id += 1;
        let id = self.repo_refresh_id;
        let now = Instant::now();
        self.timers.push((
            now + Duration::from_secs(cfg.repo.branches_refetch_interval_seconds),
            Msg::RepoTick { id, prs: false },
        ));
        self.timers.push((
            now + Duration::from_secs(cfg.repo.prs_refetch_interval_seconds),
            Msg::RepoTick { id, prs: true },
        ));
    }

    fn repo_task(
        &mut self,
        tid: String,
        start: &str,
        done: &str,
        reset: bool,
        work: impl FnOnce(&str) -> anyhow::Result<crate::git::Repo> + Send + 'static,
    ) {
        let Some(dir) = self.repo_dir() else { return };
        self.start_task(&tid, start, done);
        let key = SectionKey {
            view: ViewType::Repo,
            id: 0,
        };
        self.pool.spawn(move || {
            Some(match work(&dir) {
                Ok(repo) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: None,
                    payload: Payload::Repo { repo, reset },
                },
                Err(e) => Msg::TaskFinished {
                    task_id: tid,
                    section: None,
                    err: Some(e.to_string()),
                    payload: Payload::None,
                },
            })
        });
    }

    fn read_repo(&mut self, reset: bool) {
        let tid = task_id("fetching_branches");
        self.repo_task(tid, "Reading local branches", "Branches read", reset, |d| {
            crate::git::get_repo(d)
        });
    }

    fn git_fetch(&mut self) {
        let tid = task_id("git_fetch_repo");
        self.repo_task(
            tid,
            "Fetching branches from origin",
            "Fetched origin branches",
            false,
            crate::git::fetch_repo,
        );
    }

    fn fetch_branch_prs(&mut self) {
        let tid = task_id("fetching_pr_branches");
        self.start_task(&tid, "Fetching PRs", "PRs fetched");
        let query = format!(
            "author:@me repo:{}",
            crate::git::repo_short_name(&self.repo_url)
        );
        let limit = self.cfg.defaults.prs_limit;
        let key = SectionKey {
            view: ViewType::Repo,
            id: 0,
        };
        self.pool.spawn(move || {
            Some(match github::fetch_pull_requests(&query, limit, None) {
                Ok(p) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: None,
                    payload: Payload::BranchPrs(p.items),
                },
                Err(e) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: Some(e.to_string()),
                    payload: Payload::None,
                },
            })
        });
    }

    /// updateBranchesWithPrs: attach each branch's PR, checked-out branch first, then newest.
    fn relink_branch_prs(&mut self) {
        let Some(s) = self.repo.first_mut() else {
            return;
        };
        let prs: Vec<PullRequest> = s.prs.iter().map(|p| p.primary.clone()).collect();
        for b in s.branches.iter_mut() {
            b.pr = prs.iter().find(|p| p.head_ref_name == b.b.name).cloned();
        }
        s.branches.sort_by(|a, b| {
            use std::cmp::Ordering;
            if a.b.is_checked_out != b.b.is_checked_out {
                return if a.b.is_checked_out {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            match (a.b.last_updated_at, b.b.last_updated_at) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => a.b.name.cmp(&b.b.name),
            }
        });
        s.clamp_cursor();
    }

    fn refresh_branch_status(&mut self) {
        let Some(dir) = self.repo_dir() else { return };
        self.pool
            .spawn(move || crate::git::status(&dir).ok().map(Msg::BranchStatus));
    }

    fn current_branch(&self) -> Option<section::BranchRow> {
        match self.current_row() {
            Some(RowRef::Branch(b)) => Some(*b),
            _ => None,
        }
    }

    fn handle_repo_view_key(&mut self, key: &str) -> bool {
        let br = self.keys.branch.clone();
        if self.keys.u.open_github.matches(key) {
            if let Some(b) = self.current_branch() {
                let name = b.b.name.clone();
                let tid = format!("branch_open_{name}");
                self.start_task(
                    &tid,
                    &format!("Opening PR for branch {name}"),
                    &format!("PR for branch {name} has been opened"),
                );
                let repo = self.repo_url.clone();
                self.pool.spawn(move || {
                    let err =
                        tasks::run_gh(&["pr", "view", "--web", &name, "-R", &repo], None).err();
                    Some(Msg::TaskFinished {
                        task_id: tid,
                        section: None,
                        err,
                        payload: Payload::None,
                    })
                });
            }
            return false;
        }
        for (b, action) in [
            (&br.delete, "delete"),
            (&br.new, "new"),
            (&br.create_pr, "create_pr"),
        ] {
            if b.matches(key) {
                if let Some(s) = self.section_mut() {
                    s.prompt = Some((action.to_string(), TextInput::new("")));
                }
                return true;
            }
        }
        if br.view_prs.matches(key) {
            self.switch_selected_view();
            return false;
        }
        false
    }

    /// Branch keys: checkout, push, force-push, fast-forward.
    fn branch_section_key(&mut self, key: &str) {
        let br = self.keys.branch.clone();
        let Some(b) = self.current_branch() else {
            return;
        };
        let name = b.b.name.clone();
        let stamp = Local::now().timestamp();
        if br.checkout.matches(key) {
            let n = name.clone();
            self.repo_task(
                format!("checkout_{name}_{stamp}"),
                &format!("Checking out branch {name}"),
                &format!("Branch {name} has been checked out"),
                true,
                move |d| {
                    crate::git::checkout(d, &n)?;
                    crate::git::get_repo(d)
                },
            );
        } else if br.push.matches(key) || br.force_push.matches(key) {
            let force = br.force_push.matches(key);
            let with = if force { " with force" } else { "" };
            let (remote, upstream) = match b.b.remotes.first() {
                Some(r) => (r.clone(), false),
                None => ("origin".to_string(), true),
            };
            let n = name.clone();
            self.repo_task(
                format!("push_{name}_{stamp}"),
                &format!("Pushing branch {name}{with}"),
                &format!("Branch {name} has been pushed{with}"),
                false,
                move |d| {
                    crate::git::push(d, &remote, &n, force, upstream)?;
                    crate::git::get_repo(d)
                },
            );
        } else if br.fast_forward.matches(key) {
            let n = name.clone();
            let checked_out = b.b.is_checked_out;
            self.repo_task(
                format!("fast-forward_{name}_{stamp}"),
                &format!("Fast-forwarding branch {name}"),
                &format!("Branch {name} has been fast-forwarded"),
                false,
                move |d| {
                    crate::git::fast_forward(d, &n, checked_out)?;
                    crate::git::get_repo(d)
                },
            );
        }
    }

    fn delete_branch(&mut self) {
        let Some(b) = self.current_branch() else {
            return;
        };
        let name = b.b.name.clone();
        let n = name.clone();
        self.repo_task(
            format!("delete_{name}_{}", Local::now().timestamp()),
            &format!("Deleting branch {name}"),
            &format!("Branch {name} has been deleted"),
            false,
            move |d| {
                crate::git::delete_branch(d, &n)?;
                crate::git::get_repo(d)
            },
        );
    }

    fn new_branch(&mut self, name: &str) {
        let n = name.to_string();
        let base = self.repo_head.clone();
        self.repo_task(
            format!("create_branch_{name}_{}", Local::now().timestamp()),
            &format!("Creating branch {name}"),
            &format!("Branch {name} has been created"),
            false,
            move |d| {
                crate::git::new_branch(d, &n, &base)?;
                crate::git::get_repo(d)
            },
        );
    }

    /// Runs `gh pr create` interactively, then fetches the new PR.
    fn create_branch_pr(&mut self, title: &str) {
        let Some(b) = self.current_branch() else {
            return;
        };
        let branch = b.b.name.clone();
        let tid = format!("create_pr_{title}");
        self.start_task(
            &tid,
            &format!("Creating PR \"{title}\""),
            &format!("PR \"{title}\" has been created"),
        );
        let mut cmd = std::process::Command::new("gh");
        cmd.args(["pr", "create", "--title", title, "-R", &self.repo_url]);
        let short = crate::git::repo_short_name(&self.repo_url);
        let key = SectionKey {
            view: ViewType::Repo,
            id: 0,
        };
        self.exec = Some(ExecRequest {
            cmd,
            on_done: Box::new(move |res| {
                let created = matches!(&res, Ok(st) if st.success());
                let mut msgs = vec![Msg::TaskFinished {
                    task_id: tid,
                    section: None,
                    err: None,
                    payload: Payload::None,
                }];
                if created {
                    msgs.push(Msg::FetchBranchPr { branch, short, key });
                }
                msgs
            }),
        });
    }

    /// Fetches the PR for a branch after it was created.
    fn fetch_branch_pr(&mut self, branch: String, short: String, key: SectionKey) {
        let tid = task_id(&format!("fetching_pr_for_branch_{branch}"));
        self.start_task(
            &tid,
            &format!("Fetching PR for branch {branch}"),
            "PR fetched",
        );
        self.pool.spawn(move || {
            let res = github::fetch_pull_requests(
                &format!("author:@me repo:{short} head:{branch}"),
                1,
                None,
            );
            Some(match res {
                Ok(p) if p.items.len() == 1 => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: None,
                    payload: Payload::BranchPr(Box::new(p.items.into_iter().next().unwrap())),
                },
                Ok(p) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: Some(format!("expected 1 PR, got {}", p.items.len())),
                    payload: Payload::None,
                },
                Err(e) => Msg::TaskFinished {
                    task_id: tid,
                    section: Some(key),
                    err: Some(e.to_string()),
                    payload: Payload::None,
                },
            })
        });
    }
}

/// The URLs of a section's PR rows with `number`.
fn pr_urls(s: &Section, number: i64) -> Vec<String> {
    s.prs
        .iter()
        .filter(|p| p.primary.number == number)
        .map(|p| p.primary.url.clone())
        .collect()
}

fn apply_update_pr(s: &mut Section, u: &UpdatePr) {
    if let Some(row) = s.prs.iter_mut().find(|p| p.primary.number == u.number) {
        let pr = &mut row.primary;
        if let Some(c) = u.is_closed {
            pr.state = if c { "CLOSED".into() } else { "OPEN".into() };
        }
        if let (Some(c), Some(e)) = (&u.new_comment, row.enriched.as_mut()) {
            e.comments.nodes.push(c.clone());
        }
        if let Some(a) = &u.added_assignees {
            for login in a {
                if !pr.assignees.nodes.iter().any(|x| &x.login == login) {
                    pr.assignees.nodes.push(Login {
                        login: login.clone(),
                    });
                }
            }
        }
        if let Some(r) = &u.removed_assignees {
            pr.assignees.nodes.retain(|x| !r.contains(&x.login));
        }
        if let Some(l) = &u.labels {
            pr.labels.nodes = l.clone();
        }
        if u.ready_for_review == Some(true) {
            pr.is_draft = false;
        }
        if u.is_merged == Some(true) {
            pr.state = "MERGED".into();
            pr.mergeable.clear();
        }
    }
    s.is_loading = false;
}

pub struct NotificationFilters {
    pub repo_filters: Vec<String>,
    pub reason_filters: Vec<String>,
    pub is_done: bool,
}

pub fn parse_notification_filters(search: &str) -> NotificationFilters {
    let mut f = NotificationFilters {
        repo_filters: vec![],
        reason_filters: vec![],
        is_done: false,
    };
    for tok in search.split_whitespace() {
        if let Some(r) = tok.strip_prefix("repo:") {
            f.repo_filters.push(r.to_string());
        } else if let Some(r) = tok.strip_prefix("reason:") {
            match r {
                "participating" => f.reason_filters.extend(
                    [
                        "author",
                        "comment",
                        "mention",
                        "review_requested",
                        "assign",
                        "state_change",
                    ]
                    .map(String::from),
                ),
                "review-requested" => f.reason_filters.push("review_requested".into()),
                "team-mention" => f.reason_filters.push("team_mention".into()),
                "ci-activity" => f.reason_filters.push("ci_activity".into()),
                "security-alert" => f.reason_filters.push("security_alert".into()),
                "state-change" => f.reason_filters.push("state_change".into()),
                other => f.reason_filters.push(other.to_string()),
            }
        } else if tok == "is:done" {
            f.is_done = true;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Notification, PageInfo};

    fn notif(id: &str, hours_ago: i64) -> NotifRow {
        NotifRow::new(Notification {
            id: id.into(),
            updated_at: chrono::Utc::now() - chrono::Duration::hours(hours_ago),
            ..Default::default()
        })
    }

    fn inbox(ids: &[&str]) -> App {
        let (pool, _rx) = Pool::new(0);
        let mut app = App::new(
            Config::default(),
            Keys::default(),
            pool,
            None,
            None,
            120,
            40,
        );
        app.view = ViewType::Notifications;
        app.curr = 0;
        let s = &mut app.notifs[0];
        s.notifs = ids
            .iter()
            .enumerate()
            .map(|(i, id)| notif(id, i as i64 + 1))
            .collect();
        s.total_count = s.notifs.len();
        app
    }

    fn ids(app: &App) -> Vec<String> {
        app.notifs[0]
            .notifs
            .iter()
            .map(|n| n.id().to_string())
            .collect()
    }

    fn selected(app: &App) -> String {
        app.notifs[0].current_notif().unwrap().id().to_string()
    }

    fn done_finished(app: &mut App, row: NotifRow, err: Option<&str>) {
        app.handle_msg(Msg::TaskFinished {
            task_id: format!("notification_done_{}", row.id()),
            section: Some(SectionKey {
                view: ViewType::Notifications,
                id: 0,
            }),
            err: err.map(String::from),
            payload: Payload::NotificationDone(Box::new(row)),
        });
    }

    #[test]
    fn done_removes_the_row_before_github_answers() {
        let mut app = inbox(&["a", "b", "c"]);
        app.notifs[0].cursor = 1;
        app.handle_key("d");
        assert_eq!(ids(&app), ["a", "c"]);
        assert_eq!(selected(&app), "c");
    }

    #[test]
    fn done_on_the_last_row_selects_the_new_last_row() {
        let mut app = inbox(&["a", "b", "c"]);
        app.notifs[0].cursor = 2;
        app.handle_key("d");
        assert_eq!(ids(&app), ["a", "b"]);
        assert_eq!(selected(&app), "b");
    }

    #[test]
    fn a_failed_done_puts_the_row_back() {
        let mut app = inbox(&["a", "b", "c"]);
        app.notifs[0].cursor = 1;
        let row = app.notifs[0].current_notif().cloned().unwrap();
        app.handle_key("d");
        done_finished(&mut app, row, Some("boom"));
        assert_eq!(ids(&app), ["a", "b", "c"]);
        assert!(!app.notifs[0].session_done.contains("b"));
    }

    #[test]
    fn a_fetch_started_before_done_does_not_bring_the_row_back() {
        let mut app = inbox(&["a", "b", "c"]);
        app.notifs[0].cursor = 1;
        app.notifs[0].last_fetch_task = "fetch".into();
        app.handle_key("d");
        app.handle_msg(Msg::TaskFinished {
            task_id: "fetch".into(),
            section: Some(SectionKey {
                view: ViewType::Notifications,
                id: 0,
            }),
            err: None,
            payload: Payload::Notifications {
                task_id: "fetch".into(),
                rows: vec![notif("a", 1), notif("b", 2), notif("c", 3)],
                page_info: PageInfo::default(),
            },
        });
        assert_eq!(ids(&app), ["a", "c"]);
    }

    #[test]
    fn notification_filters_parse_like_go() {
        let f = parse_notification_filters("repo:a/b reason:participating");
        assert_eq!(f.repo_filters, vec!["a/b"]);
        assert_eq!(f.reason_filters.len(), 6);
        assert!(!f.is_done);

        let f = parse_notification_filters("reason:review-requested");
        assert_eq!(f.reason_filters, vec!["review_requested"]);
        assert!(parse_notification_filters("is:done").is_done);
    }

    fn esc_inbox(quit_on_esc: bool, confirm_quit: bool) -> App {
        let mut app = inbox(&["a"]);
        app.cfg.quit_on_esc = quit_on_esc;
        app.cfg.confirm_quit = confirm_quit;
        app
    }

    #[test]
    fn esc_does_not_quit_by_default() {
        let mut app = esc_inbox(false, false);
        app.handle_key("esc");
        assert!(!app.quit);
    }

    #[test]
    fn esc_quits_when_enabled() {
        let mut app = esc_inbox(true, false);
        app.view = ViewType::Prs;
        app.handle_key("esc");
        assert!(app.quit);
    }

    #[test]
    fn esc_asks_to_confirm_when_confirm_quit_is_on() {
        let mut app = esc_inbox(true, true);
        app.handle_key("esc");
        assert!(!app.quit);
        assert!(app.confirm_quit_shown);
    }

    #[test]
    fn esc_quits_at_once_while_a_notification_subject_is_shown() {
        let mut app = esc_inbox(true, false);
        app.subject_issue = Some(Issue::default());
        app.handle_key("esc");
        assert!(app.quit);
    }

    #[test]
    fn esc_closes_help_before_quitting() {
        let mut app = esc_inbox(true, false);
        app.handle_key("?");
        assert!(app.help.is_some());
        app.handle_key("esc");
        assert!(app.help.is_none());
        assert!(!app.quit);
    }
}
