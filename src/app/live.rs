//! Keeping data fresh at low cost: the focus gate, launch from the disk cache,
//! the list sync, the change check, preloading details, and saving.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};

use super::App;
use crate::config::ViewType;
use crate::disk_cache::{self, Details, DiskCache, Lists, SectionSnapshot, Stamped};
use crate::github::{self, Membership, PrVersion, SyncQuery};
use crate::model::*;
use crate::msg::*;
use crate::refresh::{self, PrefetchRows, Subject};
use crate::section::{Kind, NotifRow, PrRow, RowRef, Section};
use crate::store::{DetailStore, LanePolicy, Retry};

/// Full searches running at once in the background.
const MAX_FULL_SEARCHES: usize = 3;
/// A view syncs on switch when its last sync is older than this.
pub(super) const SYNC_ON_SWITCH: Duration = Duration::from_secs(300);
/// Focus and launch skip the sync when it ran this recently.
const SYNC_DEBOUNCE: Duration = Duration::from_secs(30);
/// The change check skips a view checked this recently.
const CHECK_DEBOUNCE: Duration = Duration::from_secs(60);
/// The change check runs this often while focused.
const CHECK_INTERVAL: Duration = Duration::from_secs(300);
const SAVE_INTERVAL: Duration = Duration::from_secs(30);
const DAY: chrono::Duration = chrono::Duration::hours(24);

pub struct Live {
    pub focused: bool,
    pub started: bool,
    pub details_loaded: bool,
    pub pr_store: DetailStore<PullRequestDetails>,
    pub issue_store: DetailStore<Issue>,
    pub last_sync: HashMap<ViewType, Instant>,
    pub last_check: HashMap<ViewType, Instant>,
    pub pr_checked: HashMap<String, Instant>,
    pub full_queue: VecDeque<SectionKey>,
    pub full_running: HashSet<String>,
    pub refresh_due: bool,
    pub subject_wanted: Option<String>,
    pub cache: Option<DiskCache>,
    pub dirty: bool,
    pub last_save: Instant,
    pub login_at: Option<DateTime<Utc>>,
    pub latest_version_at: Option<DateTime<Utc>>,
    pub rate_notice_at: Option<Instant>,
    pub repo_started: bool,
}

fn lane_policy() -> LanePolicy {
    LanePolicy {
        batch_size: 5,
        wait: std::sync::Arc::new(github::background_wait),
        retry: std::sync::Arc::new(|e| {
            if github::is_rate_limited(e) {
                Retry::Later
            } else if github::is_timeout(e) {
                Retry::OneByOne
            } else {
                Retry::Fail
            }
        }),
    }
}

impl Live {
    pub fn new(pool: &Pool) -> Live {
        let tx = std::sync::Mutex::new(pool.msgs.clone());
        let pr_store = DetailStore::new(
            |urls: &[String], bg| {
                if bg {
                    github::background(|| github::fetch_pull_requests_by_url(urls))
                } else {
                    github::fetch_pull_requests_by_url(urls)
                }
            },
            move |url, result| {
                let _ = tx.lock().unwrap().send(Msg::PrDetail {
                    url,
                    result: result.map(Box::new),
                });
            },
            lane_policy(),
        );
        let tx = std::sync::Mutex::new(pool.msgs.clone());
        let issue_store = DetailStore::new(
            |urls: &[String], bg| {
                if bg {
                    github::background(|| github::fetch_issues_by_url(urls))
                } else {
                    github::fetch_issues_by_url(urls)
                }
            },
            move |url, result| {
                let _ = tx.lock().unwrap().send(Msg::IssueDetail {
                    url,
                    result: result.map(Box::new),
                });
            },
            lane_policy(),
        );
        Live {
            focused: true,
            started: false,
            details_loaded: false,
            pr_store,
            issue_store,
            last_sync: HashMap::new(),
            last_check: HashMap::new(),
            pr_checked: HashMap::new(),
            full_queue: VecDeque::new(),
            full_running: HashSet::new(),
            refresh_due: false,
            subject_wanted: None,
            cache: None,
            dirty: false,
            last_save: Instant::now(),
            login_at: None,
            latest_version_at: None,
            rate_notice_at: None,
            repo_started: false,
        }
    }
}

/// An `Instant` `age` ago, or as far back as the clock allows.
fn instant_ago(age: Duration) -> Instant {
    let now = Instant::now();
    let mut age = age;
    loop {
        if let Some(t) = now.checked_sub(age) {
            return t;
        }
        if age.is_zero() {
            return now;
        }
        age /= 2;
    }
}

fn age_of(at: DateTime<Utc>) -> Duration {
    (Utc::now() - at).to_std().unwrap_or_default()
}

fn wall_of(t: Instant) -> DateTime<Utc> {
    Utc::now() - chrono::Duration::from_std(t.elapsed()).unwrap_or_default()
}

/// Fills each row's details from the store, marking details older than the row stale.
pub(super) fn fill_details(rows: &mut [PrRow], store: &DetailStore<PullRequestDetails>) {
    for row in rows {
        if let Some(e) = store.get(&row.primary.url) {
            if e.value.updated_at < row.primary.updated_at {
                store.mark_stale(&row.primary.url);
            }
            row.details = Some(e.value);
        }
    }
}

fn set_rollup(pr: &mut PullRequest, state: &str) {
    match pr.commits.nodes.last_mut() {
        Some(c) => c.commit.status_check_rollup.state = state.to_string(),
        None => pr.commits.nodes.push(CommitNode {
            commit: CommitWithRollup {
                status_check_rollup: RollupState {
                    state: state.to_string(),
                },
            },
        }),
    }
}

/// One section's part of a list sync.
struct SyncTarget {
    key: SectionKey,
    kind: Kind,
    query: SyncQuery,
    known: Vec<(String, DateTime<Utc>)>,
}

impl App {
    fn sections_of(&self, view: ViewType) -> &Vec<Section> {
        match view {
            ViewType::Prs => &self.prs,
            ViewType::Issues => &self.issues,
            ViewType::Notifications => &self.notifs,
            ViewType::Repo => &self.repo,
        }
    }

    /// The disk cache key of the repo view's branch PRs.
    fn branch_pr_filters(&self) -> String {
        format!(
            "author:@me repo:{}",
            crate::git::repo_short_name(&self.repo_url)
        )
    }

    fn snapshot_filters(&self, view: ViewType, s: &Section) -> String {
        if view == ViewType::Repo {
            self.branch_pr_filters()
        } else {
            s.search_value_for_fetch(self.gh_repo.as_ref(), true)
        }
    }

    /// Shows the last session's lists, then starts.
    pub fn on_cache_loaded(&mut self, lists: Lists, cache: Option<DiskCache>) {
        self.live.cache = cache;
        github::seed_conditional(lists.conditional.clone(), lists.notifications_polled);
        self.seed_lists(lists);
        crate::info!("lists seeded");
        self.start();
    }

    fn seed_lists(&mut self, lists: Lists) {
        if let Some(l) = lists.login
            && Utc::now() - l.at < DAY
        {
            self.user = l.value;
            self.live.login_at = Some(l.at);
        }
        if let Some(v) = lists.latest_version
            && Utc::now() - v.at < DAY
        {
            self.latest_version = v.value;
            self.live.latest_version_at = Some(v.at);
        }
        for (view, at) in lists.last_check {
            self.live.last_check.insert(view, instant_ago(age_of(at)));
        }
        for view in [
            ViewType::Prs,
            ViewType::Issues,
            ViewType::Notifications,
            ViewType::Repo,
        ] {
            let n = self.sections_for(view).len();
            for id in 0..n {
                let filters = self.snapshot_filters(view, &self.sections_of(view)[id]);
                let Some(snap) = lists
                    .sections
                    .iter()
                    .find(|s| s.view == view && s.filters == filters)
                else {
                    continue;
                };
                let s = &mut self.sections_for(view)[id];
                match s.kind {
                    Kind::Pr | Kind::Branch => {
                        s.prs = snap
                            .prs
                            .iter()
                            .map(|p| PrRow {
                                primary: p.clone(),
                                details: None,
                            })
                            .collect();
                    }
                    Kind::Issue => s.issues = snap.issues.clone(),
                    Kind::Notification => {
                        s.notifs = snap
                            .notifications
                            .iter()
                            .cloned()
                            .map(NotifRow::new)
                            .collect();
                        s.sort_notifications();
                    }
                }
                if s.kind != Kind::Branch {
                    s.total_count = snap.total_count;
                    s.last_fetch_at = Some(instant_ago(age_of(snap.fetched_at)));
                    s.last_updated = snap.fetched_at.with_timezone(&Local);
                }
                s.clamp_cursor();
            }
        }
        if let Some(s) = self.repo.first_mut()
            && !s.prs.is_empty()
        {
            self.relink_branch_prs();
        }
        self.invalidate_preview();
    }

    /// Fills the detail stores with the last session's details.
    pub(super) fn on_details_loaded(&mut self, d: Details) {
        self.live.pr_store.seed(d.prs);
        self.live.issue_store.seed(d.issues);
        self.live.details_loaded = true;
        crate::info!("details seeded");
        let store = &self.live.pr_store;
        for s in self.prs.iter_mut() {
            fill_details(&mut s.prs, store);
        }
        self.notification_extras(None);
        self.invalidate_preview();
        if self.live.started {
            self.change_check(self.view);
            self.show_current_row_details();
            self.schedule_notification_preview();
            self.refresh_prefetch();
        }
    }

    pub fn start(&mut self) {
        if self.live.started {
            return;
        }
        self.live.started = true;
        if self.live.login_at.is_none() {
            self.pool.spawn(|| {
                Some(match github::current_login() {
                    Ok(u) => Msg::User(u),
                    Err(e) => Msg::Error(e.to_string()),
                })
            });
        }
        if self.live.latest_version_at.is_none() {
            self.pool
                .spawn(|| github::latest_version().ok().map(Msg::LatestVersion));
        }
        if crate::config::feature_enabled("FF_REPO_VIEW") {
            self.start_repo_view();
        }
        self.catch_up();
        self.on_viewed_row_changed();
    }

    fn snapshot_lists(&self) -> Lists {
        let mut sections = vec![];
        for view in [
            ViewType::Prs,
            ViewType::Issues,
            ViewType::Notifications,
            ViewType::Repo,
        ] {
            let list = match view {
                ViewType::Prs => &self.prs,
                ViewType::Issues => &self.issues,
                ViewType::Notifications => &self.notifs,
                ViewType::Repo => &self.repo,
            };
            for s in list {
                let fetched_at = match (s.kind, s.last_fetch_at) {
                    (Kind::Branch, _) if !s.prs.is_empty() => self
                        .live
                        .last_sync
                        .get(&ViewType::Repo)
                        .map(|t| wall_of(*t)),
                    (Kind::Branch, _) => None,
                    (_, t) => t.map(wall_of),
                };
                let Some(fetched_at) = fetched_at else {
                    continue;
                };
                sections.push(SectionSnapshot {
                    view,
                    filters: self.snapshot_filters(view, s),
                    fetched_at,
                    prs: s.prs.iter().map(|r| r.primary.clone()).collect(),
                    issues: s.issues.clone(),
                    notifications: s.notifs.iter().map(|n| n.n.clone()).collect(),
                    total_count: s.total_count,
                });
            }
        }
        Lists {
            version: disk_cache::VERSION,
            login: self.live.login_at.map(|at| Stamped {
                value: self.user.clone(),
                at,
            }),
            latest_version: self.live.latest_version_at.map(|at| Stamped {
                value: self.latest_version.clone(),
                at,
            }),
            last_check: self
                .live
                .last_check
                .iter()
                .map(|(v, t)| (*v, wall_of(*t)))
                .collect(),
            sections,
            conditional: github::conditional_snapshot(),
            notifications_polled: github::notifications_polled(),
        }
    }

    /// Writes the cache; in the background unless `blocking`.
    pub fn save_cache(&mut self, blocking: bool) {
        let Some(cache) = self.live.cache.clone() else {
            return;
        };
        let started = Instant::now();
        let lists = self.snapshot_lists();
        let details = self.live.details_loaded.then(|| Details {
            version: disk_cache::VERSION,
            prs: self.live.pr_store.snapshot(disk_cache::MAX_DETAILS),
            issues: self.live.issue_store.snapshot(disk_cache::MAX_DETAILS),
        });
        self.live.dirty = false;
        self.live.last_save = Instant::now();
        let save = move || {
            if let Err(e) = cache.save(lists, details) {
                crate::warn!("saving the cache failed err={e:#}");
            }
            crate::info!("cache saved ms={}", started.elapsed().as_millis());
        };
        if blocking {
            save();
        } else {
            std::thread::spawn(save);
        }
    }

    pub fn on_focus(&mut self, focused: bool) {
        crate::info!("focus focused={focused}");
        self.live.focused = focused;
        self.live.pr_store.set_active(focused);
        self.live.issue_store.set_active(focused);
        if focused && self.live.started {
            self.catch_up();
        }
    }

    /// Brings the app up to date after launch or a return of focus.
    pub(super) fn catch_up(&mut self) {
        self.live.pr_store.set_active(true);
        self.live.issue_store.set_active(true);
        let views = self.views_current_first();
        for v in &views {
            self.queue_unfetched(*v);
        }
        let full = std::mem::take(&mut self.live.refresh_due);
        if full {
            self.queue_full_refresh(&views);
        } else {
            let due: Vec<ViewType> = views
                .iter()
                .copied()
                .filter(|v| {
                    self.live
                        .last_sync
                        .get(v)
                        .is_none_or(|t| t.elapsed() >= SYNC_DEBOUNCE)
                })
                .collect();
            self.sync_views(&due);
            self.queue_notifications_if_due(false);
        }
        self.pump_full();
        self.change_check(self.view);
        self.refresh_prefetch();
    }

    fn views_current_first(&self) -> Vec<ViewType> {
        let mut views = vec![self.view];
        for v in [
            ViewType::Notifications,
            ViewType::Prs,
            ViewType::Issues,
            ViewType::Repo,
        ] {
            if v != self.view && (v != ViewType::Repo || !self.repo.is_empty()) {
                views.push(v);
            }
        }
        views
    }

    /// Housekeeping on each tick: the refetch interval, the periodic change check and saving.
    pub(super) fn live_tick(&mut self) -> bool {
        if !self.live.started {
            return false;
        }
        let mut changed = false;
        let interval = self.cfg.defaults.refetch_interval_minutes;
        if interval > 0 && self.last_refresh.elapsed() >= Duration::from_secs(interval * 60) {
            self.last_refresh = Instant::now();
            if self.live.focused {
                let views = self.views_current_first();
                self.queue_full_refresh(&views);
                self.pump_full();
                changed = true;
            } else {
                self.live.refresh_due = true;
            }
        }
        if self.live.focused
            && self
                .live
                .last_check
                .get(&self.view)
                .is_none_or(|t| t.elapsed() >= CHECK_INTERVAL)
        {
            self.change_check(self.view);
        }
        if self.live.dirty && self.live.last_save.elapsed() >= SAVE_INTERVAL {
            self.save_cache(false);
        }
        changed
    }

    /// Sections that never loaded get a full search.
    pub(super) fn queue_unfetched(&mut self, view: ViewType) {
        let n = self.sections_for(view).len();
        for id in 0..n {
            let s = &self.sections_for(view)[id];
            if matches!(s.kind, Kind::Pr | Kind::Issue | Kind::Notification)
                && s.last_fetch_at.is_none()
                && !s.is_loading
            {
                self.queue_full(SectionKey { view, id });
            }
        }
    }

    fn queue_full_refresh(&mut self, views: &[ViewType]) {
        for v in views {
            let n = self.sections_for(*v).len();
            for id in 0..n {
                if matches!(self.sections_for(*v)[id].kind, Kind::Pr | Kind::Issue) {
                    self.queue_full(SectionKey { view: *v, id });
                }
            }
        }
        self.queue_notifications_if_due(true);
    }

    /// Notifications reload when GitHub's poll interval allows it.
    pub(super) fn queue_notifications_if_due(&mut self, force: bool) {
        if !force && !github::notifications_poll_due() {
            return;
        }
        for id in 0..self.notifs.len() {
            if self.notifs[id].last_fetch_at.is_some() {
                self.queue_full(SectionKey {
                    view: ViewType::Notifications,
                    id,
                });
            }
        }
    }

    fn queue_full(&mut self, key: SectionKey) {
        if !self.live.full_queue.contains(&key) {
            self.live.full_queue.push_back(key);
        }
    }

    /// Starts queued full searches, up to `MAX_FULL_SEARCHES` at a time.
    pub(super) fn pump_full(&mut self) {
        while self.live.full_running.len() < MAX_FULL_SEARCHES {
            let Some(key) = self.live.full_queue.pop_front() else {
                return;
            };
            let Some(s) = self.sections_for(key.view).get_mut(key.id) else {
                continue;
            };
            s.page_info = None;
            if let Some(tid) = self.fetch_page(key.view, key.id, true) {
                self.live.full_running.insert(tid);
            }
        }
    }

    fn sync_targets(&self, view: ViewType) -> Vec<SyncTarget> {
        let mut out = vec![];
        match view {
            ViewType::Prs | ViewType::Issues => {
                let list = if view == ViewType::Prs {
                    &self.prs
                } else {
                    &self.issues
                };
                let default_limit = if view == ViewType::Prs {
                    self.cfg.defaults.prs_limit
                } else {
                    self.cfg.defaults.issues_limit
                };
                for (id, s) in list.iter().enumerate() {
                    if s.last_fetch_at.is_none() || s.is_loading {
                        continue;
                    }
                    let filters = s.search_value_for_fetch(self.gh_repo.as_ref(), true);
                    let limit = s.limit.unwrap_or(default_limit);
                    let (query, known) = if s.kind == Kind::Pr {
                        (SyncQuery::prs(&filters, limit), refresh::known(&s.prs))
                    } else {
                        (
                            SyncQuery::issues(&filters, limit),
                            refresh::known(&s.issues),
                        )
                    };
                    out.push(SyncTarget {
                        key: SectionKey { view, id },
                        kind: s.kind,
                        query,
                        known,
                    });
                }
            }
            ViewType::Repo => {
                if self.live.repo_started
                    && !self.repo_url.is_empty()
                    && let Some(s) = self.repo.first()
                {
                    out.push(SyncTarget {
                        key: SectionKey {
                            view: ViewType::Repo,
                            id: 0,
                        },
                        kind: Kind::Pr,
                        query: SyncQuery::prs(
                            &self.branch_pr_filters(),
                            self.cfg.defaults.prs_limit,
                        ),
                        known: refresh::known(&s.prs),
                    });
                }
            }
            ViewType::Notifications => {}
        }
        out
    }

    /// Syncs `views`: the current view in its own request, then the rest.
    pub(super) fn sync_views(&mut self, views: &[ViewType]) {
        let (current, rest): (Vec<ViewType>, Vec<ViewType>) =
            views.iter().partition(|v| **v == self.view);
        for group in [current, rest] {
            let targets: Vec<SyncTarget> =
                group.iter().flat_map(|v| self.sync_targets(*v)).collect();
            for v in &group {
                self.live.last_sync.insert(*v, Instant::now());
            }
            if targets.is_empty() {
                continue;
            }
            self.pool
                .spawn(move || Some(github::background(|| run_sync(targets))));
        }
    }

    pub(super) fn on_synced(&mut self, results: Vec<SyncedSection>) {
        for sec in results {
            match sec.result {
                Ok(rows) => self.apply_sync(sec.key, rows),
                Err(e) => self.note_error(&format!("sync failed: {e}")),
            }
        }
        self.live.dirty = true;
        self.refresh_prefetch();
    }

    fn apply_sync(&mut self, key: SectionKey, rows: SyncedRows) {
        let is_current = key.view == self.view && key.id == self.curr;
        let before = is_current
            .then(|| self.current_row().map(|r| r.url()))
            .flatten();
        let store = &self.live.pr_store;
        let list = match key.view {
            ViewType::Prs => &mut self.prs,
            ViewType::Issues => &mut self.issues,
            ViewType::Repo => &mut self.repo,
            ViewType::Notifications => return,
        };
        let Some(s) = list.get_mut(key.id) else {
            return;
        };
        if s.is_loading && key.view != ViewType::Repo {
            return;
        }
        let m: &Membership = &rows.membership;
        if matches!(s.kind, Kind::Pr | Kind::Branch) {
            let mut fetched: Vec<PrRow> = rows
                .prs
                .into_iter()
                .map(|p| PrRow {
                    primary: p,
                    details: None,
                })
                .collect();
            fill_details(&mut fetched, store);
            s.prs = refresh::apply_membership(&s.prs, m, &fetched);
        } else {
            s.issues = refresh::apply_membership(&s.issues, m, &rows.issues);
        }
        if s.kind != Kind::Branch {
            s.total_count = m.total_count;
            s.page_info = Some(m.page_info.clone());
            s.last_fetch_at = Some(Instant::now());
            s.last_updated = Local::now();
        }
        s.clamp_cursor();
        if key.view == ViewType::Repo {
            self.relink_branch_prs();
        }
        let after = is_current
            .then(|| self.current_row().map(|r| r.url()))
            .flatten();
        if is_current && before != after {
            self.on_viewed_row_changed();
        } else {
            self.invalidate_preview();
        }
    }

    fn check_ids(&self, view: ViewType) -> Vec<String> {
        let mut ids: Vec<String> = match view {
            ViewType::Prs => self
                .prs
                .iter()
                .flat_map(|s| s.prs.iter().map(|r| r.primary.id.clone()))
                .collect(),
            ViewType::Repo => self
                .repo
                .iter()
                .flat_map(|s| s.prs.iter().map(|r| r.primary.id.clone()))
                .collect(),
            ViewType::Notifications => self
                .notifs
                .iter()
                .flat_map(|s| s.notifs.iter())
                .filter(|n| n.subject_type() == "PullRequest")
                .filter_map(|n| self.live.pr_store.peek(&n.url(), |e| e.value.id.clone()))
                .collect(),
            ViewType::Issues => vec![],
        };
        let mut seen = HashSet::new();
        ids.retain(|id| !id.is_empty() && seen.insert(id.clone()));
        ids.truncate(100);
        ids
    }

    /// Asks GitHub which loaded PRs changed, unless the view was checked recently.
    pub(super) fn change_check(&mut self, view: ViewType) {
        if !self.live.focused || !self.live.started {
            return;
        }
        if self
            .live
            .last_check
            .get(&view)
            .is_some_and(|t| t.elapsed() < CHECK_DEBOUNCE)
        {
            return;
        }
        let ids = self.check_ids(view);
        if ids.is_empty() {
            // Notification subjects' IDs arrive with the details.
            if self.live.details_loaded {
                self.live.last_check.insert(view, Instant::now());
            }
            return;
        }
        self.live.last_check.insert(view, Instant::now());
        self.live.dirty = true;
        self.spawn_check(ids);
    }

    fn spawn_check(&mut self, ids: Vec<String>) {
        let now = Instant::now();
        for id in &ids {
            self.live.pr_checked.insert(id.clone(), now);
        }
        self.pool.spawn(move || {
            Some(Msg::Checked(
                github::background(|| github::check_pull_requests(&ids)).map_err(|e| e.to_string()),
            ))
        });
    }

    pub(super) fn on_checked(&mut self, versions: Vec<PrVersion>) {
        for v in &versions {
            for s in self.prs.iter_mut().chain(self.repo.iter_mut()) {
                for row in s.prs.iter_mut().filter(|r| r.primary.id == v.id) {
                    set_rollup(&mut row.primary, &v.rollup_state);
                    if let Some(e) = self.live.pr_store.get(&row.primary.url)
                        && github::is_outdated(&e.value, v)
                    {
                        self.live.pr_store.mark_stale(&row.primary.url);
                    }
                }
            }
            if let Some(e) = self.live.pr_store.get(&v.url)
                && github::is_outdated(&e.value, v)
            {
                self.live.pr_store.mark_stale(&v.url);
            }
        }
        self.invalidate_preview();
        self.refresh_prefetch();
    }

    /// The selection rested on a PR: check it alone if its last check is old.
    pub(super) fn check_selected(&mut self, url: &str) {
        if !self.live.focused {
            return;
        }
        let id = match self.current_row() {
            Some(RowRef::Pr(pr)) if pr.primary.url == url => pr.primary.id.clone(),
            Some(RowRef::Notification(n)) if n.url() == url => match self.live.pr_store.get(url) {
                Some(e) => e.value.id,
                None => return,
            },
            _ => return,
        };
        if id.is_empty()
            || self
                .live
                .pr_checked
                .get(&id)
                .is_some_and(|t| t.elapsed() < CHECK_DEBOUNCE)
        {
            return;
        }
        self.spawn_check(vec![id]);
    }

    fn schedule_check_selected(&mut self, url: String) {
        self.timers.push((
            Instant::now() + Duration::from_millis(500),
            Msg::CheckSelected { url },
        ));
    }

    /// Shows the selected PR's details from the store, fetching them if missing.
    pub(super) fn show_current_row_details(&mut self) {
        if !self.live.details_loaded {
            return;
        }
        let Some(RowRef::Pr(pr)) = self.current_row() else {
            return;
        };
        let url = pr.primary.url.clone();
        if self.view == ViewType::Repo {
            return;
        }
        match self.live.pr_store.get(&url) {
            Some(e) => {
                if e.value.updated_at < pr.primary.updated_at {
                    self.live.pr_store.mark_stale(&url);
                }
                if pr.details.is_none() {
                    self.set_details(&url, &e.value);
                }
            }
            None => self.live.pr_store.fetch_now(&url),
        }
        self.schedule_check_selected(url);
    }

    fn set_details(&mut self, url: &str, pr: &PullRequestDetails) {
        for s in self.prs.iter_mut() {
            for row in s.prs.iter_mut().filter(|r| r.primary.url == url) {
                row.details = Some(pr.clone());
            }
        }
    }

    /// Shows a notification's subject from the store. False when it isn't cached.
    pub(super) fn show_cached_subject(&mut self, n: &NotifRow) -> bool {
        let url = n.url();
        match n.subject_type() {
            "PullRequest" => {
                let Some(e) = self.live.pr_store.get(&url) else {
                    return false;
                };
                self.show_subject_pr(n.id(), e.value);
                self.schedule_check_selected(url);
                true
            }
            "Issue" => {
                let Some(e) = self.live.issue_store.get(&url) else {
                    return false;
                };
                self.show_subject_issue(n.id(), e.value);
                true
            }
            _ => false,
        }
    }

    fn show_subject_pr(&mut self, notif_id: &str, pr: PullRequestDetails) {
        if self.subject_id != notif_id {
            self.pr_tab = 0;
        }
        self.subject_pr = Some(PrRow {
            primary: pr.to_pull_request(),
            details: Some(pr),
        });
        self.subject_issue = None;
        self.subject_id = notif_id.to_string();
        self.live.subject_wanted = None;
    }

    fn show_subject_issue(&mut self, notif_id: &str, issue: Issue) {
        self.subject_issue = Some(issue);
        self.subject_pr = None;
        self.subject_id = notif_id.to_string();
        self.live.subject_wanted = None;
    }

    /// Fetches the selected notification's subject now.
    pub(super) fn want_subject(&mut self, n: &NotifRow) {
        self.live.subject_wanted = Some(n.id().to_string());
        match n.subject_type() {
            "PullRequest" => self.live.pr_store.fetch_now(&n.url()),
            "Issue" => self.live.issue_store.fetch_now(&n.url()),
            _ => {}
        }
    }

    pub(super) fn on_pr_detail(
        &mut self,
        url: String,
        result: Result<Box<PullRequestDetails>, String>,
    ) {
        let selected = match self.current_row() {
            Some(RowRef::Notification(n)) if n.url() == url => Some(n.id().to_string()),
            _ => None,
        };
        match result {
            Ok(pr) => {
                self.set_details(&url, &pr);
                if let Some(id) = selected
                    && (self.live.subject_wanted.as_deref() == Some(id.as_str())
                        || self.subject_id == id)
                {
                    self.show_subject_pr(&id, (*pr).clone());
                }
                self.notification_extras(Some(&url));
                self.live.dirty = true;
            }
            Err(e) => {
                if selected.is_some() && self.live.subject_wanted == selected {
                    self.live.subject_wanted = None;
                    self.error = Some(format!("failed fetching notification PR: {e}"));
                } else if matches!(self.current_row(), Some(RowRef::Pr(p)) if p.primary.url == url)
                {
                    self.error = Some(format!("failed enriching pr: {e}"));
                }
                self.note_error(&e);
            }
        }
    }

    pub(super) fn on_issue_detail(&mut self, url: String, result: Result<Box<Issue>, String>) {
        let selected = match self.current_row() {
            Some(RowRef::Notification(n)) if n.url() == url => Some(n.id().to_string()),
            _ => None,
        };
        match result {
            Ok(issue) => {
                if let Some(id) = selected
                    && (self.live.subject_wanted.as_deref() == Some(id.as_str())
                        || self.subject_id == id)
                {
                    self.show_subject_issue(&id, (*issue).clone());
                }
                self.notification_extras(Some(&url));
                self.live.dirty = true;
            }
            Err(e) => {
                if selected.is_some() && self.live.subject_wanted == selected {
                    self.live.subject_wanted = None;
                    self.error = Some(format!("failed fetching notification Issue: {e}"));
                }
                self.note_error(&e);
            }
        }
    }

    /// Updates the comment count, state and actor of notifications whose subject is
    /// cached; only those with subject `url` when given. Marks subjects older than
    /// their notification stale.
    pub(super) fn notification_extras(&mut self, url: Option<&str>) {
        let prs = &self.live.pr_store;
        let issues = &self.live.issue_store;
        for s in self.notifs.iter_mut() {
            for n in s.notifs.iter_mut() {
                let u = n.url();
                if url.is_some_and(|x| x != u) {
                    continue;
                }
                let (extras, fetched_at) = match n.subject_type() {
                    "PullRequest" => match prs.peek(&u, |e| {
                        (
                            refresh::notification_extras(n, Subject::Pr(&e.value)),
                            e.fetched_at,
                        )
                    }) {
                        Some(x) => x,
                        None => continue,
                    },
                    "Issue" => match issues.peek(&u, |e| {
                        (
                            refresh::notification_extras(n, Subject::Issue(&e.value)),
                            e.fetched_at,
                        )
                    }) {
                        Some(x) => x,
                        None => continue,
                    },
                    _ => continue,
                };
                if url.is_none() && n.n.updated_at > fetched_at {
                    if n.subject_type() == "PullRequest" {
                        prs.mark_stale(&u);
                    } else {
                        issues.mark_stale(&u);
                    }
                }
                n.new_comments = extras.new_comments;
                n.subject_state = extras.state;
                n.is_draft = extras.is_draft;
                n.activity = crate::section::activity_description(
                    &n.n.reason,
                    &n.n.subject.kind,
                    &extras.actor,
                );
                n.actor = extras.actor;
            }
        }
    }

    /// After a mutation, the subject's cached details reload.
    pub(super) fn refresh_after_mutation(&mut self, pr_urls: Vec<String>, issue_urls: Vec<String>) {
        let selected = self.current_row().map(|r| r.url());
        for url in pr_urls {
            self.live.pr_store.mark_stale(&url);
            if selected.as_deref() == Some(url.as_str()) {
                self.live.pr_store.fetch_now(&url);
            }
        }
        for url in issue_urls {
            self.live.issue_store.mark_stale(&url);
            if selected.as_deref() == Some(url.as_str()) {
                self.live.issue_store.fetch_now(&url);
            }
        }
        self.refresh_prefetch();
    }

    pub(super) fn mark_all_details_stale(&mut self) {
        self.live.pr_store.mark_all_stale();
        self.live.issue_store.mark_all_stale();
    }

    /// Hands the stores what to preload, nearest to the cursor first.
    pub(super) fn refresh_prefetch(&mut self) {
        if !self.live.details_loaded || !self.live.started {
            return;
        }
        let rows_of = |s: &Section| -> PrefetchRows<(bool, String)> {
            let items = match s.kind {
                Kind::Pr => s
                    .prs
                    .iter()
                    .map(|r| Some((true, r.primary.url.clone())))
                    .collect(),
                Kind::Notification => s
                    .notifs
                    .iter()
                    .map(|n| match n.subject_type() {
                        "PullRequest" => Some((true, n.url())),
                        "Issue" => Some((false, n.url())),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };
            PrefetchRows {
                items,
                cursor: s.cursor,
                top: s.top,
            }
        };
        let body_h = self.body_height();
        let screen = self
            .section()
            .map(|s| s.per_page(&self.cfg, body_h))
            .unwrap_or(10)
            .max(1);
        let current = self.section().map(rows_of).unwrap_or(PrefetchRows {
            items: vec![],
            cursor: 0,
            top: 0,
        });
        let same: Vec<PrefetchRows<(bool, String)>> = self
            .sections()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.curr)
            .map(|(_, s)| rows_of(s))
            .collect();
        let mut others = vec![];
        for v in self.views_current_first().into_iter().skip(1) {
            let list = match v {
                ViewType::Prs => &self.prs,
                ViewType::Issues => &self.issues,
                ViewType::Notifications => &self.notifs,
                ViewType::Repo => &self.repo,
            };
            others.extend(list.iter().map(rows_of));
        }
        let order = refresh::prefetch_order(&current, &same, &others, screen);
        let (prs, issues): (Vec<_>, Vec<_>) = order.into_iter().partition(|(is_pr, _)| *is_pr);
        self.live
            .pr_store
            .prefetch(prs.into_iter().map(|(_, u)| u).collect());
        self.live
            .issue_store
            .prefetch(issues.into_iter().map(|(_, u)| u).collect());
    }

    pub fn store_busy(&self) -> bool {
        self.live.pr_store.busy() || self.live.issue_store.busy()
    }

    /// Logs an error; a rate limit shows once a minute in the footer.
    pub(super) fn note_error(&mut self, e: &str) {
        crate::error!("{e}");
        if e.contains("rate limit")
            && self
                .live
                .rate_notice_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(60))
        {
            self.live.rate_notice_at = Some(Instant::now());
            self.notify_err(e);
        }
    }
}

/// Runs in the background: the sync request, then the changed rows by ID.
fn run_sync(targets: Vec<SyncTarget>) -> Msg {
    let started = Instant::now();
    let queries: Vec<SyncQuery> = targets.iter().map(|t| t.query.clone()).collect();
    let results = match github::sync_sections(&queries) {
        Ok(r) => r,
        Err(e) => {
            let e = e.to_string();
            return Msg::Synced(
                targets
                    .iter()
                    .map(|t| SyncedSection {
                        key: t.key,
                        result: Err(e.clone()),
                    })
                    .collect(),
            );
        }
    };
    let mut pr_ids: Vec<String> = vec![];
    let mut issue_ids: Vec<String> = vec![];
    for (t, r) in targets.iter().zip(&results) {
        if let Ok(m) = r {
            let ids = refresh::changed_ids(&t.known, m);
            if t.kind == Kind::Issue {
                issue_ids.extend(ids);
            } else {
                pr_ids.extend(ids);
            }
        }
    }
    let mut seen = HashSet::new();
    pr_ids.retain(|id| seen.insert(id.clone()));
    issue_ids.retain(|id| seen.insert(id.clone()));
    let prs = if pr_ids.is_empty() {
        vec![]
    } else {
        github::fetch_prs_by_id(&pr_ids).unwrap_or_else(|e| {
            crate::error!("fetching changed PR rows failed err={e}");
            vec![]
        })
    };
    let issues = if issue_ids.is_empty() {
        vec![]
    } else {
        github::fetch_issues_by_id(&issue_ids).unwrap_or_else(|e| {
            crate::error!("fetching changed issue rows failed err={e}");
            vec![]
        })
    };
    crate::info!(
        "sync sections={} changed_prs={} changed_issues={} ms={}",
        targets.len(),
        pr_ids.len(),
        issue_ids.len(),
        started.elapsed().as_millis()
    );
    Msg::Synced(
        targets
            .into_iter()
            .zip(results)
            .map(|(t, r)| SyncedSection {
                key: t.key,
                result: r.map(|m| {
                    let ids: HashSet<&String> = m.rows.iter().map(|(id, _)| id).collect();
                    SyncedRows {
                        prs: prs
                            .iter()
                            .filter(|p| ids.contains(&p.id))
                            .cloned()
                            .collect(),
                        issues: issues
                            .iter()
                            .filter(|i| ids.contains(&i.id))
                            .cloned()
                            .collect(),
                        membership: m,
                    }
                }),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::keys::Keys;

    /// An app whose pool has no workers: background jobs are queued and dropped,
    /// so nothing reaches the network.
    fn app() -> App {
        let (pool, _rx) = Pool::new(0);
        App::new(
            Config::default(),
            Keys::default(),
            pool,
            None,
            None,
            120,
            40,
        )
    }

    fn snapshot_of(app: &App, view: ViewType, id: usize, age: chrono::Duration) -> SectionSnapshot {
        let s = &app.sections_of(view)[id];
        SectionSnapshot {
            view,
            filters: s.search_value_for_fetch(None, true),
            fetched_at: Utc::now() - age,
            prs: vec![PullRequest {
                id: format!("PR_{id}"),
                url: format!("https://github.com/o/r/pull/{id}"),
                ..Default::default()
            }],
            issues: vec![],
            notifications: vec![],
            total_count: 1,
        }
    }

    fn loading(app: &App, view: ViewType) -> Vec<bool> {
        app.sections_of(view).iter().map(|s| s.is_loading).collect()
    }

    fn cached_lists(app: &App, views: &[ViewType], age: chrono::Duration) -> Lists {
        let mut lists = Lists::default();
        for view in views {
            for id in 0..app.sections_of(*view).len() {
                lists.sections.push(snapshot_of(app, *view, id, age));
            }
        }
        lists
    }

    #[test]
    fn a_launch_with_every_list_cached_syncs_instead_of_searching() {
        let mut app = app();
        app.view = ViewType::Prs;
        let views = [ViewType::Prs, ViewType::Issues, ViewType::Notifications];
        let lists = cached_lists(&app, &views, chrono::Duration::minutes(10));
        app.on_cache_loaded(lists, None);
        assert!(loading(&app, ViewType::Prs).iter().all(|l| !l));
        assert!(loading(&app, ViewType::Issues).iter().all(|l| !l));
        assert_eq!(app.prs[0].prs[0].primary.id, "PR_0");
        assert!(app.live.last_sync.contains_key(&ViewType::Prs));
        assert!(app.live.last_sync.contains_key(&ViewType::Issues));
    }

    #[test]
    fn a_section_missing_from_the_cache_gets_a_full_search() {
        let mut app = app();
        app.view = ViewType::Prs;
        assert!(app.prs.len() >= 2);
        let lists = Lists {
            sections: vec![snapshot_of(
                &app,
                ViewType::Prs,
                0,
                chrono::Duration::minutes(1),
            )],
            ..Default::default()
        };
        app.on_cache_loaded(lists, None);
        let prs = loading(&app, ViewType::Prs);
        assert!(!prs[0]);
        assert!(prs[1]);
    }

    #[test]
    fn nothing_refreshes_while_unfocused_and_focus_catches_up_once() {
        let mut app = app();
        app.view = ViewType::Prs;
        app.cfg.defaults.refetch_interval_minutes = 1;
        let views = [ViewType::Prs, ViewType::Issues, ViewType::Notifications];
        let lists = cached_lists(&app, &views, chrono::Duration::seconds(5));
        app.on_cache_loaded(lists, None);
        app.on_focus(false);
        app.last_refresh = instant_ago(Duration::from_secs(120));
        app.tick();
        assert!(loading(&app, ViewType::Prs).iter().all(|l| !l));
        assert!(app.live.refresh_due);
        app.on_focus(true);
        assert!(!app.live.refresh_due);
        assert_eq!(app.live.full_running.len(), MAX_FULL_SEARCHES);
        app.on_focus(true);
        assert_eq!(app.live.full_running.len(), MAX_FULL_SEARCHES);
    }
}
