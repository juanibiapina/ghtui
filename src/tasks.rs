//! Background actions: gh CLI mutations and the notification fetch loop.

use std::collections::{HashMap, HashSet};
use std::process::Command;

use anyhow::Result;
use chrono::Utc;

use crate::app::{App, ExecRequest, NotificationFilters};
use crate::bookmarks;
use crate::github;
use crate::model::*;
use crate::msg::*;
use crate::section::NotifRow;

/// Runs `gh` and returns the trimmed stderr as the error message on failure.
pub fn run_gh(args: &[&str], dir: Option<&str>) -> std::result::Result<String, String> {
    let mut c = Command::new("gh");
    c.args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            out.status.to_string()
        } else {
            err.lines().last().unwrap_or_default().to_string()
        })
    }
}

fn fire(
    app: &mut App,
    id: String,
    args: Vec<String>,
    start: String,
    done: String,
    key: SectionKey,
    payload: Payload,
) {
    app.start_task(&id, &start, &done);
    app.pool.spawn(move || {
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let err = run_gh(&refs, None).err();
        Some(Msg::TaskFinished {
            task_id: id,
            section: Some(key),
            err,
            payload,
        })
    });
}

fn s(v: &str) -> String {
    v.to_string()
}

/// close / reopen / ready / update-branch
pub fn simple_pr(app: &mut App, verb: &str, n: i64, repo: &str, key: SectionKey) {
    let (prefix, start, done, update) = match verb {
        "close" => (
            "pr_close",
            format!("Closing PR #{n}"),
            format!("PR #{n} has been closed"),
            UpdatePr {
                is_closed: Some(true),
                ..Default::default()
            },
        ),
        "reopen" => (
            "pr_reopen",
            format!("Reopening PR #{n}"),
            format!("PR #{n} has been reopened"),
            UpdatePr {
                is_closed: Some(false),
                ..Default::default()
            },
        ),
        "ready" => (
            "pr_ready",
            format!("Marking PR #{n} as ready for review"),
            format!("PR #{n} has been marked as ready for review"),
            UpdatePr {
                ready_for_review: Some(true),
                ..Default::default()
            },
        ),
        _ => (
            "pr_update",
            format!("Updating PR #{n}"),
            format!("PR #{n} has been updated"),
            UpdatePr::default(),
        ),
    };
    let args = vec![s("pr"), s(verb), n.to_string(), s("-R"), s(repo)];
    fire(
        app,
        format!("{prefix}_{n}"),
        args,
        start,
        done,
        key,
        Payload::UpdatePr(UpdatePr {
            number: n,
            ..update
        }),
    );
}

pub fn simple_issue(app: &mut App, verb: &str, n: i64, repo: &str, key: SectionKey) {
    let closed = verb == "close";
    let (start, done) = if closed {
        (
            format!("Closing issue #{n}"),
            format!("Issue #{n} has been closed"),
        )
    } else {
        (
            format!("Reopening issue #{n}"),
            format!("Issue #{n} has been reopened"),
        )
    };
    let args = vec![s("issue"), s(verb), n.to_string(), s("-R"), s(repo)];
    let payload = Payload::UpdateIssue(UpdateIssue {
        number: n,
        is_closed: Some(closed),
        ..Default::default()
    });
    fire(
        app,
        format!("issue_{verb}_{n}"),
        args,
        start,
        done,
        key,
        payload,
    );
}

/// Merging runs interactively, so the TUI is suspended while it runs.
pub fn merge(app: &mut App, n: i64, repo: &str, key: SectionKey) {
    let id = format!("merge_{n}");
    app.start_task(
        &id,
        &format!("Merging PR #{n}"),
        &format!("PR #{n} has been merged"),
    );
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "merge", &n.to_string(), "-R", repo]);
    app.exec = Some(ExecRequest {
        cmd,
        on_done: Box::new(move |res| {
            let merged = matches!(&res, Ok(st) if st.success());
            let err = match res {
                Err(e) => Some(e.to_string()),
                Ok(st) if !st.success() => Some(st.to_string()),
                _ => None,
            };
            vec![Msg::TaskFinished {
                task_id: id,
                section: Some(key),
                err,
                payload: Payload::UpdatePr(UpdatePr {
                    number: n,
                    is_merged: Some(merged),
                    ..Default::default()
                }),
            }]
        }),
    });
}

pub fn approve_workflows(app: &mut App, n: i64, repo: &str, key: SectionKey) {
    let id = format!("pr_approve_workflows_{n}");
    app.start_task(
        &id,
        &format!("Approving workflows for PR #{n}"),
        &format!("Workflows for PR #{n} have been approved"),
    );
    let repo = repo.to_string();
    app.pool.spawn(move || {
        let fail = |e: String| Msg::TaskFinished {
            task_id: id.clone(),
            section: Some(key),
            err: Some(e),
            payload: Payload::UpdatePr(UpdatePr {
                number: n,
                ..Default::default()
            }),
        };
        let sha = match run_gh(
            &[
                "pr",
                "view",
                &n.to_string(),
                "-R",
                &repo,
                "--json",
                "headRefOid",
                "--jq",
                ".headRefOid",
            ],
            None,
        ) {
            Ok(s) => s.trim().to_string(),
            Err(e) => return Some(fail(format!("failed to get head SHA: {e}"))),
        };
        let runs = match run_gh(
            &[
                "api",
                &format!("repos/{repo}/actions/runs?status=action_required&head_sha={sha}"),
                "--jq",
                ".workflow_runs[].id",
            ],
            None,
        ) {
            Ok(s) => s,
            Err(e) => return Some(fail(format!("failed to get workflow runs: {e}"))),
        };
        let ids: Vec<&str> = runs.split_whitespace().collect();
        if ids.is_empty() {
            return Some(fail("no workflows awaiting approval".into()));
        }
        let mut last_err = None;
        for run in ids {
            if let Err(e) = run_gh(
                &[
                    "api",
                    "-X",
                    "POST",
                    &format!("repos/{repo}/actions/runs/{run}/approve"),
                ],
                None,
            ) {
                last_err = Some(if e.contains("not from a fork pull request") {
                    "workflow not approvable via API (only fork PR workflows can be approved)"
                        .to_string()
                } else {
                    format!("failed to approve run {run}: {e}")
                });
            }
        }
        Some(Msg::TaskFinished {
            task_id: id,
            section: Some(key),
            err: last_err,
            payload: Payload::UpdatePr(UpdatePr {
                number: n,
                ..Default::default()
            }),
        })
    });
}

pub fn comment(app: &mut App, is_pr: bool, n: i64, repo: &str, body: &str, key: SectionKey) {
    let noun = if is_pr { "pr" } else { "issue" };
    let new_comment = Comment {
        author: Login {
            login: app.user.clone(),
        },
        body: body.to_string(),
        updated_at: Utc::now(),
    };
    let (start, done) = if is_pr {
        (
            format!("Commenting on PR #{n}"),
            format!("Commented on PR #{n}"),
        )
    } else {
        (
            format!("Commenting on issue #{n}"),
            format!("Commented on issue #{n}"),
        )
    };
    let payload = if is_pr {
        Payload::UpdatePr(UpdatePr {
            number: n,
            new_comment: Some(new_comment),
            ..Default::default()
        })
    } else {
        Payload::UpdateIssue(UpdateIssue {
            number: n,
            new_comment: Some(new_comment),
            ..Default::default()
        })
    };
    let args = vec![
        s(noun),
        s("comment"),
        n.to_string(),
        s("-R"),
        s(repo),
        s("-b"),
        s(body),
    ];
    fire(
        app,
        format!("{noun}_comment_{n}"),
        args,
        start,
        done,
        key,
        payload,
    );
}

pub fn approve(app: &mut App, n: i64, repo: &str, comment: &str, key: SectionKey) {
    let mut args = vec![
        s("pr"),
        s("review"),
        s("-R"),
        s(repo),
        n.to_string(),
        s("--approve"),
    ];
    if !comment.is_empty() {
        args.push(s("--body"));
        args.push(s(comment));
    }
    fire(
        app,
        format!("pr_approve_{n}"),
        args,
        format!("Approving pr #{n}"),
        format!("pr #{n} has been approved"),
        key,
        Payload::UpdatePr(UpdatePr {
            number: n,
            ..Default::default()
        }),
    );
}

pub fn assign(
    app: &mut App,
    noun: &str,
    n: i64,
    repo: &str,
    users: Vec<String>,
    add: bool,
    key: SectionKey,
) {
    let mut args = vec![s(noun), s("edit"), n.to_string(), s("-R"), s(repo)];
    let flag = if add {
        "--add-assignee"
    } else {
        "--remove-assignee"
    };
    for u in &users {
        args.push(s(flag));
        args.push(u.clone());
    }
    let list = format!("[{}]", users.join(" "));
    let (id, start, done) = if add {
        (
            format!("{noun}_assign_{n}"),
            format!("Assigning {noun} #{n} to {list}"),
            format!("{noun} #{n} has been assigned to {list}"),
        )
    } else {
        (
            format!("{noun}_unassign_{n}"),
            format!("Unassigning {list} from {noun} #{n}"),
            format!("{list} unassigned from {noun} #{n}"),
        )
    };
    let payload = if noun == "pr" {
        Payload::UpdatePr(UpdatePr {
            number: n,
            added_assignees: add.then(|| users.clone()),
            removed_assignees: (!add).then(|| users.clone()),
            ..Default::default()
        })
    } else {
        Payload::UpdateIssue(UpdateIssue {
            number: n,
            added_assignees: add.then(|| users.clone()),
            removed_assignees: (!add).then(|| users.clone()),
            ..Default::default()
        })
    };
    fire(app, id, args, start, done, key, payload);
}

pub fn label(
    app: &mut App,
    noun: &str,
    n: i64,
    repo: &str,
    labels: Vec<String>,
    existing: &[Label],
    key: SectionKey,
) {
    let mut args = vec![s(noun), s("edit"), n.to_string(), s("-R"), s(repo)];
    for l in existing {
        if !labels.contains(&l.name) {
            args.push(s("--remove-label"));
            args.push(l.name.clone());
        }
    }
    for l in &labels {
        args.push(s("--add-label"));
        args.push(l.clone());
    }
    let colors: HashMap<&str, &str> = existing
        .iter()
        .map(|l| (l.name.as_str(), l.color.as_str()))
        .collect();
    let new_labels: Vec<Label> = labels
        .iter()
        .map(|l| Label {
            name: l.clone(),
            color: colors
                .get(l.as_str())
                .map(|c| c.to_string())
                .unwrap_or_default(),
            description: String::new(),
        })
        .collect();
    let list = format!("[{}]", labels.join(" "));
    let payload = if noun == "pr" {
        Payload::UpdatePr(UpdatePr {
            number: n,
            labels: Some(new_labels),
            ..Default::default()
        })
    } else {
        Payload::UpdateIssue(UpdateIssue {
            number: n,
            labels: Some(new_labels),
            ..Default::default()
        })
    };
    fire(
        app,
        format!("{noun}_label_{n}"),
        args,
        format!("Labeling {noun} #{n} to {list}"),
        format!("{noun} #{n} has been labeled with {list}"),
        key,
        payload,
    );
}

pub fn desktop_notify(title: &str, body: &str) {
    if cfg!(target_os = "macos") {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            esc(body),
            esc(title)
        );
        let _ = Command::new("osascript").args(["-e", &script]).output();
    } else {
        let _ = Command::new("notify-send").args([title, body]).output();
    }
}

/// Marks a notification done. It marks the thread read first so that GitHub's
/// `last_read_at`, which the new comment count starts from, moves to now.
pub fn mark_done(id: &str) -> Result<()> {
    let read = github::mark_notification_read(id);
    github::mark_notification_done(id)?;
    bookmarks::store().remove(id);
    read
}

/// The notification fetch loop: pages until `limit` rows survive local filtering.
pub fn fetch_notifications(
    f: &NotificationFilters,
    limit: usize,
    page: Option<PageInfo>,
    session_done: &HashSet<String>,
) -> Result<(Vec<NotifRow>, PageInfo)> {
    let mut out: Vec<NotifRow> = vec![];
    let mut current = page;
    let reasons: HashSet<&str> = f.reason_filters.iter().map(|s| s.as_str()).collect();
    loop {
        let res = github::fetch_notifications(limit, &f.repo_filters, current.as_ref())?;
        let last_page = res.page_info.clone();
        for n in res.notifications {
            if session_done.contains(&n.id) {
                continue;
            }
            if !reasons.is_empty() && !reasons.contains(n.reason.as_str()) {
                continue;
            }
            out.push(NotifRow::new(n));
        }
        if out.len() >= limit || !last_page.has_next_page {
            return Ok((out, last_page));
        }
        current = Some(last_page);
    }
}
