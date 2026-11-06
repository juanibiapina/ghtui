//! Local git state for the branches view (internal/git/git.go), read with the
//! git CLI the same way git-module does.

use std::process::Command;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, TimeZone, Utc};

#[derive(Clone, Debug, Default)]
pub struct NameStatus {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Branch {
    pub name: String,
    pub last_updated_at: Option<DateTime<Utc>>,
    pub last_commit_msg: Option<String>,
    pub commits_ahead: i64,
    pub commits_behind: i64,
    pub is_checked_out: bool,
    pub remotes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Repo {
    pub branches: Vec<Branch>,
    pub head_branch_name: String,
    pub status: NameStatus,
}

fn git(dir: &str, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("{}", if err.is_empty() { format!("git {} failed", args.join(" ")) } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// git.GetOriginUrl
pub fn origin_url(dir: &str) -> Result<String> {
    let remotes = git(dir, &["remote"])?;
    if !remotes.lines().any(|r| r.trim() == "origin") {
        bail!("no origin remote found");
    }
    let urls = git(dir, &["remote", "get-url", "origin"])?;
    urls.lines().next().map(|s| s.to_string()).ok_or_else(|| anyhow!("no origin remote found"))
}

/// git.GetRepoShortName
pub fn repo_short_name(url: &str) -> String {
    let r = url.strip_prefix("https://github.com/").unwrap_or(url);
    r.strip_suffix(".git").unwrap_or(r).to_string()
}

/// git.GetStatus: `git diff HEAD --name-status`.
pub fn status(dir: &str) -> Result<NameStatus> {
    let out = git(dir, &["diff", "HEAD", "--name-status"])?;
    let mut s = NameStatus::default();
    for line in out.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 2 {
            continue;
        }
        match f[0].chars().next() {
            Some('A') => s.added.push(f[1].to_string()),
            Some('D') => s.removed.push(f[1].to_string()),
            Some('M') => s.modified.push(f[1].to_string()),
            _ => {}
        }
    }
    Ok(s)
}

fn rev_count(dir: &str, range: &str) -> i64 {
    git(dir, &["rev-list", "--count", range]).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// git.GetRepo
pub fn get_repo(dir: &str) -> Result<Repo> {
    let names: Vec<String> = git(dir, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])?
        .lines()
        .map(|s| s.to_string())
        .collect();
    let head_ref = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let status = status(dir)?;
    let mut branches: Vec<Branch> = names
        .iter()
        .map(|b| {
            let log = git(dir, &["log", "-1", "--format=%ct%x00%s", b]).ok();
            let (updated, msg) = match log.as_deref().and_then(|l| l.trim_end().split_once('\0')) {
                Some((ts, subject)) => (
                    ts.parse::<i64>().ok().and_then(|t| Utc.timestamp_opt(t, 0).single()),
                    Some(subject.to_string()),
                ),
                None => (None, None),
            };
            // git-module's RemoteGetURL is called with the branch name as the remote.
            let remotes = git(dir, &["remote", "get-url", b])
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default();
            Branch {
                name: b.clone(),
                last_updated_at: updated,
                last_commit_msg: msg,
                commits_ahead: rev_count(dir, &format!("origin/{b}..{b}")),
                commits_behind: rev_count(dir, &format!("{b}..origin/{b}")),
                is_checked_out: *b == head_ref,
                remotes,
            }
        })
        .collect();
    branches.sort_by(|a, b| match (a.last_updated_at, b.last_updated_at) {
        (Some(x), Some(y)) => y.cmp(&x),
        _ => std::cmp::Ordering::Equal,
    });
    let head = git(dir, &["symbolic-ref", "HEAD"])?.trim().to_string();
    let head_branch_name = head.strip_prefix("refs/heads/").unwrap_or(&head).to_string();
    // git.GetRepo also reads the remotes and the origin URL and fails without them.
    git(dir, &["remote", "show"])?;
    git(dir, &["remote", "get-url", "--all", "origin"])?;
    Ok(Repo { branches, head_branch_name, status })
}

/// git.FetchRepo: `git fetch --all`, then read.
pub fn fetch_repo(dir: &str) -> Result<Repo> {
    git(dir, &["fetch", "--all"])?;
    get_repo(dir)
}

pub fn checkout(dir: &str, branch: &str) -> Result<()> {
    git(dir, &["checkout", branch]).map(|_| ())
}

pub fn new_branch(dir: &str, name: &str, base: &str) -> Result<()> {
    git(dir, &["checkout", "-b", name, base]).map(|_| ())
}

pub fn delete_branch(dir: &str, name: &str) -> Result<()> {
    git(dir, &["branch", "-D", name]).map(|_| ())
}

pub fn push(dir: &str, remote: &str, branch: &str, force: bool, set_upstream: bool) -> Result<()> {
    let mut args = vec!["push"];
    if force {
        args.push("--force");
    }
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push(remote);
    args.push(branch);
    git(dir, &args).map(|_| ())
}

pub fn fast_forward(dir: &str, branch: &str, checked_out: bool) -> Result<()> {
    if checked_out {
        git(dir, &["pull", "--ff-only", "--no-edit", "origin", branch]).map(|_| ())
    } else {
        git(dir, &["fetch", "--no-write-fetch-head", "origin", &format!("{branch}:{branch}")]).map(|_| ())
    }
}
