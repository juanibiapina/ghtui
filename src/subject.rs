//! Notification subjects other than PRs and issues: workflow runs, releases,
//! commits and discussions. `key` names a notification's subject for the
//! detail store, and `fetch` finds and loads what a key names.

use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use regex::Regex;

use crate::github;
use crate::model::*;
use crate::section::NotifRow;

#[derive(Clone, Debug)]
pub enum SubjectDetails {
    Run(RunDetails),
    Release(Release),
    Commit(CommitDetails),
    Discussion(Discussion),
}

impl SubjectDetails {
    /// The page `o` and `Y` open.
    pub fn html_url(&self) -> &str {
        match self {
            Self::Run(r) => &r.run.html_url,
            Self::Release(r) => &r.html_url,
            Self::Commit(c) => &c.commit.html_url,
            Self::Discussion(d) => &d.url,
        }
    }
}

fn last_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

/// The store key for a notification's subject. None for PRs and issues, which
/// have stores of their own, and for kinds without a preview.
pub fn key(n: &NotifRow) -> Option<String> {
    let repo = n.repo();
    let title = &n.n.subject.title;
    let url = n.n.subject.url.as_deref();
    match n.subject_type() {
        SubjectType::CheckSuite => Some(format!(
            "run|{repo}|{}|{title}",
            n.n.updated_at.to_rfc3339()
        )),
        SubjectType::Release => url.map(|u| format!("release|{u}")),
        SubjectType::Commit => url.map(|u| format!("commit|{repo}|{}", last_segment(u))),
        SubjectType::Discussion => {
            let number: i64 = url.and_then(|u| last_segment(u).parse().ok()).unwrap_or(0);
            Some(format!("discussion|{repo}|{number}|{title}"))
        }
        _ => None,
    }
}

/// True when the preview can show the notification's subject.
pub fn previewable(n: &NotifRow) -> bool {
    matches!(
        n.subject_type(),
        SubjectType::PullRequest | SubjectType::Issue
    ) || key(n).is_some()
}

/// Finds and loads the subject `key` names. Errors from `github` come back
/// unchanged, so the store can tell rate limits apart.
pub fn fetch(key: &str) -> Result<SubjectDetails> {
    let bad = || anyhow!("unknown subject key: {key}");
    let (kind, rest) = key.split_once('|').ok_or_else(bad)?;
    match kind {
        "run" => {
            let mut parts = rest.splitn(3, '|');
            let (Some(repo), Some(at), Some(title)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(bad());
            };
            let at = DateTime::parse_from_rfc3339(at)?.with_timezone(&Utc);
            fetch_run(repo, at, title).map(SubjectDetails::Run)
        }
        "release" => github::fetch_release(rest).map(SubjectDetails::Release),
        "commit" => {
            let (repo, sha) = rest.split_once('|').ok_or_else(bad)?;
            github::fetch_commit(repo, sha).map(SubjectDetails::Commit)
        }
        "discussion" => {
            let mut parts = rest.splitn(3, '|');
            let (Some(repo), Some(number), Some(title)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return Err(bad());
            };
            let d = match number.parse::<i64>().unwrap_or(0) {
                0 => github::find_discussion(repo, title)?,
                n => github::fetch_discussion(repo, n)?,
            };
            Ok(SubjectDetails::Discussion(d))
        }
        _ => Err(bad()),
    }
}

fn fetch_run(repo: &str, at: DateTime<Utc>, title: &str) -> Result<RunDetails> {
    let parsed = parse_title(title);
    let runs = github::fetch_runs(repo, parsed.as_ref().map(|t| t.branch.as_str()), at)?;
    let run = pick_run(title, at, &runs)
        .cloned()
        .ok_or_else(|| anyhow!("no workflow run found for \"{title}\""))?;
    let attempt = match &parsed {
        Some(t) => t.attempt,
        None => run.run_attempt.unwrap_or(1),
    };
    let jobs = github::fetch_run_jobs(repo, run.id, attempt)?;
    Ok(RunDetails { run, attempt, jobs })
}

/// What a check suite notification's title says about its run.
#[derive(Debug, PartialEq)]
struct RunTitle {
    workflow: String,
    attempt: i64,
    branch: String,
}

/// Reads "<workflow> workflow run[, Attempt #N] <outcome> for <branch> branch".
fn parse_title(title: &str) -> Option<RunTitle> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^(?P<workflow>.+?) workflow run(?:, Attempt #(?P<attempt>\d+))? \S+ for (?P<branch>.+) branch$",
        )
        .unwrap()
    });
    let c = re.captures(title)?;
    Some(RunTitle {
        workflow: c["workflow"].to_string(),
        attempt: c
            .name("attempt")
            .and_then(|a| a.as_str().parse().ok())
            .unwrap_or(1),
        branch: c["branch"].to_string(),
    })
}

/// The run a check suite notification is about: the run of the workflow its
/// title names that finished closest to `at`, otherwise the closest run.
pub fn pick_run<'a>(
    title: &str,
    at: DateTime<Utc>,
    runs: &'a [WorkflowRun],
) -> Option<&'a WorkflowRun> {
    parse_title(title)
        .and_then(|t| {
            runs.iter()
                .filter(|r| r.name.as_deref() == Some(t.workflow.as_str()))
                .min_by_key(|r| (at - r.updated_at).abs())
        })
        .or_else(|| github::best_workflow_run(runs, at))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn run(id: i64, name: &str, minutes_before: i64, at: DateTime<Utc>) -> WorkflowRun {
        WorkflowRun {
            id,
            name: Some(name.into()),
            updated_at: at - Duration::minutes(minutes_before),
            ..Default::default()
        }
    }

    #[test]
    fn the_run_of_the_named_workflow_wins_over_a_closer_run_of_another() {
        let at = Utc::now();
        let runs = [
            run(1, "Lint", 0, at),
            run(2, "Test", 5, at),
            run(3, "Test", 90, at),
        ];
        let title = "Test workflow run failed for main branch";
        assert_eq!(pick_run(title, at, &runs).map(|r| r.id), Some(2));
    }

    #[test]
    fn titles_name_the_workflow_attempt_and_branch() {
        assert_eq!(
            parse_title("Test workflow run, Attempt #2 failed for gpui-control branch"),
            Some(RunTitle {
                workflow: "Test".into(),
                attempt: 2,
                branch: "gpui-control".into(),
            })
        );
        assert_eq!(
            parse_title("Build mini system workflow run failed for main branch"),
            Some(RunTitle {
                workflow: "Build mini system".into(),
                attempt: 1,
                branch: "main".into(),
            })
        );
    }

    #[test]
    fn an_unreadable_title_falls_back_to_the_closest_run() {
        let at = Utc::now();
        let runs = [run(1, "Lint", 30, at), run(2, "Test", 5, at)];
        assert_eq!(
            pick_run("Something happened", at, &runs).map(|r| r.id),
            Some(2)
        );
    }

    #[test]
    fn a_workflow_without_runs_falls_back_to_the_closest_run() {
        let at = Utc::now();
        let runs = [run(1, "Lint", 30, at), run(2, "Deploy", 5, at)];
        let title = "Test workflow run failed for main branch";
        assert_eq!(pick_run(title, at, &runs).map(|r| r.id), Some(2));
    }

    fn notification(kind: SubjectType, url: Option<&str>) -> NotifRow {
        NotifRow::new(Notification {
            subject: NotificationSubject {
                title: "T".into(),
                url: url.map(String::from),
                kind,
                ..Default::default()
            },
            repository: NotificationRepository {
                full_name: "o/r".into(),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    #[test]
    fn only_runs_releases_commits_and_discussions_have_keys() {
        let api = "https://api.github.com/repos/o/r";
        let with_key = [
            notification(SubjectType::CheckSuite, None),
            notification(SubjectType::Release, Some(&format!("{api}/releases/1"))),
            notification(SubjectType::Commit, Some(&format!("{api}/commits/abc"))),
            notification(SubjectType::Discussion, None),
        ];
        assert!(with_key.iter().all(|n| key(n).is_some()));
        let without = [
            notification(SubjectType::PullRequest, Some(&format!("{api}/pulls/1"))),
            notification(SubjectType::Issue, Some(&format!("{api}/issues/1"))),
            notification(SubjectType::RepositoryVulnerabilityAlert, None),
            notification(SubjectType::Unknown, None),
        ];
        assert!(without.iter().all(|n| key(n).is_none()));
        assert!(without[..2].iter().all(previewable));
        assert!(!previewable(&without[2]));
    }
}
