//! Messages delivered to the app loop from background work, and the worker
//! pool that runs that work (the equivalent of bubbletea commands).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::config::ViewType;
use crate::github::SearchPage;
use crate::model::*;
use crate::section::NotifRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    NotificationRemoved(String),
    NotificationRead(String),
    ClearAllNotifications,
    MarkAllRead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpTarget {
    Search(SectionKey),
    PrEditor,
    IssueEditor,
}

pub enum Msg {
    Suggestions {
        target: CmpTarget,
        users: Option<Vec<(String, String)>>,
        labels: Option<Vec<Label>>,
        err: Option<String>,
    },
    CmpClearStatus(CmpTarget),
    TaskFinished {
        task_id: String,
        section: Option<SectionKey>,
        err: Option<String>,
        payload: Payload,
    },
    Enriched {
        url: String,
        result: Result<EnrichedPullRequest, String>,
    },
    NotifComments {
        id: String,
        count: i64,
        state: String,
        is_draft: bool,
        actor: String,
    },
    NotifUrl {
        id: String,
        url: String,
    },
    NotifPr {
        notif_id: String,
        result: Result<EnrichedPullRequest, String>,
        preview: bool,
    },
    NotifIssue {
        notif_id: String,
        result: Result<Issue, String>,
        preview: bool,
    },
    NotifReadState {
        id: String,
        unread: bool,
    },
    NotifPreview {
        notif_id: String,
    },
    User(String),
    Error(String),
}

pub type Job = Box<dyn FnOnce() -> Option<Msg> + Send>;

#[derive(Clone)]
pub struct Pool {
    jobs: Sender<Job>,
    pub msgs: Sender<Msg>,
}

impl Pool {
    pub fn new(workers: usize) -> (Pool, Receiver<Msg>) {
        let (jtx, jrx) = channel::<Job>();
        let (mtx, mrx) = channel::<Msg>();
        let jrx = Arc::new(Mutex::new(jrx));
        for _ in 0..workers {
            let jrx = jrx.clone();
            let mtx = mtx.clone();
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let rx = jrx.lock().unwrap();
                        rx.recv()
                    };
                    match job {
                        Ok(job) => {
                            if let Some(m) = job()
                                && mtx.send(m).is_err()
                            {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
            });
        }
        (
            Pool {
                jobs: jtx,
                msgs: mtx,
            },
            mrx,
        )
    }

    pub fn spawn(&self, job: impl FnOnce() -> Option<Msg> + Send + 'static) {
        let _ = self.jobs.send(Box::new(job));
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
