//! Messages delivered to the app loop from background work, and the worker
//! pool that runs that work.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::config::ViewType;
use crate::github::SearchPage;
use crate::model::*;
use crate::section::NotifRow;

/// One section's result in a list sync.
pub struct SyncedSection {
    pub key: SectionKey,
    pub result: Result<SyncedRows, String>,
}

pub struct SyncedRows {
    pub membership: crate::github::Membership,
    pub prs: Vec<PullRequest>,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SectionKey {
    pub view: ViewType,
    pub id: usize,
}

#[derive(Debug, Default, Clone)]
pub struct UpdatePr {
    pub number: i64,
    pub is_closed: Option<bool>,
    pub new_comment: Option<Comment>,
    pub ready_for_review: Option<bool>,
    pub is_merged: Option<bool>,
    pub added_assignees: Option<Vec<String>>,
    pub removed_assignees: Option<Vec<String>>,
    pub labels: Option<Vec<Label>>,
}

#[derive(Debug, Default, Clone)]
pub struct UpdateIssue {
    pub number: i64,
    pub labels: Option<Vec<Label>>,
    pub new_comment: Option<Comment>,
    pub is_closed: Option<bool>,
    pub added_assignees: Option<Vec<String>>,
    pub removed_assignees: Option<Vec<String>>,
}

pub enum Payload {
    None,
    Prs {
        task_id: String,
        page: SearchPage<PullRequest>,
    },
    Issues {
        task_id: String,
        page: SearchPage<Issue>,
    },
    Notifications {
        task_id: String,
        rows: Vec<NotifRow>,
        page_info: PageInfo,
    },
    UpdatePr(UpdatePr),
    UpdateIssue(UpdateIssue),
    NotificationDone(Box<crate::section::NotifRow>),
    ClearAllNotifications,
    Repo {
        repo: crate::git::Repo,
        reset: bool,
    },
    BranchPrs(Vec<PullRequest>),
    BranchPr(Box<PullRequest>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionTarget {
    Search(SectionKey),
    PrEditor,
    IssueEditor,
}

pub enum Msg {
    Suggestions {
        target: CompletionTarget,
        users: Option<Vec<(String, String)>>,
        labels: Option<Vec<Label>>,
        err: Option<String>,
    },
    CompletionClearStatus(CompletionTarget),
    TaskFinished {
        task_id: String,
        section: Option<SectionKey>,
        err: Option<String>,
        payload: Payload,
    },
    /// A PR's details from the detail store, whoever asked for them.
    PrDetail {
        url: String,
        result: Result<Box<PullRequestDetails>, String>,
    },
    IssueDetail {
        url: String,
        result: Result<Box<Issue>, String>,
    },
    NotifUrl {
        id: String,
        url: String,
    },
    /// The last session's lists, read from disk.
    CacheLoaded {
        lists: Box<crate::disk_cache::Lists>,
        cache: Option<crate::disk_cache::DiskCache>,
    },
    DetailsLoaded(Box<crate::disk_cache::Details>),
    /// A list sync: each section's membership and the rows that changed.
    Synced(Vec<SyncedSection>),
    /// The change check's view of PRs.
    Checked(Result<Vec<crate::github::PrVersion>, String>),
    /// The selection rested on a PR: check it if its last check is old.
    CheckSelected {
        url: String,
    },
    NotifPreview {
        notif_id: String,
    },
    User(String),
    RepoTick {
        id: u64,
        prs: bool,
    },
    BranchStatus(crate::git::NameStatus),
    FetchBranchPr {
        branch: String,
        short: String,
        key: SectionKey,
    },
    LatestVersion(String),
    Error(String),
}

pub type Job = Box<dyn FnOnce() -> Option<Msg> + Send>;

#[derive(Clone)]
pub struct Pool {
    jobs: Sender<Job>,
    pub msgs: Sender<Msg>,
    pending: Arc<AtomicUsize>,
}

impl Pool {
    pub fn new(workers: usize) -> (Pool, Receiver<Msg>) {
        let (jtx, jrx) = channel::<Job>();
        let (mtx, mrx) = channel::<Msg>();
        let jrx = Arc::new(Mutex::new(jrx));
        let pending = Arc::new(AtomicUsize::new(0));
        for _ in 0..workers {
            let jrx = jrx.clone();
            let mtx = mtx.clone();
            let pending = pending.clone();
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let rx = jrx.lock().unwrap();
                        rx.recv()
                    };
                    let Ok(job) = job else { return };
                    let sent = job().is_none_or(|m| mtx.send(m).is_ok());
                    pending.fetch_sub(1, Ordering::SeqCst);
                    if !sent {
                        return;
                    }
                }
            });
        }
        (
            Pool {
                jobs: jtx,
                msgs: mtx,
                pending,
            },
            mrx,
        )
    }

    pub fn spawn(&self, job: impl FnOnce() -> Option<Msg> + Send + 'static) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.jobs.send(Box::new(job)).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// True while queued or running jobs may still deliver a message.
    pub fn busy(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }

    /// Runs on its own thread, for long-lived work like `gh pr checks --watch`.
    pub fn spawn_detached(&self, job: impl FnOnce() -> Option<Msg> + Send + 'static) {
        let tx = self.msgs.clone();
        std::thread::spawn(move || {
            if let Some(m) = job() {
                let _ = tx.send(m);
            }
        });
    }
}
