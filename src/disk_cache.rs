//! The last session's lists and details on disk, so a launch paints at once.
//! One directory per account (`<cache dir>/ghtui/<account key>/`): a small
//! `lists.json` loaded before the first sync, and a large `details.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::ViewType;
use crate::github::Conditional;
use crate::model::{EnrichedPullRequest, Issue, Notification, PullRequest};
use crate::store::Entry;

/// Bump to discard files written by an incompatible version.
pub const VERSION: u32 = 1;
pub const MAX_DETAILS: usize = 300;
pub const MAX_CONDITIONAL: usize = 50;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stamped<T> {
    pub value: T,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Lists {
    pub version: u32,
    pub login: Option<Stamped<String>>,
    pub latest_version: Option<Stamped<String>>,
    pub last_check: Vec<(ViewType, DateTime<Utc>)>,
    pub sections: Vec<SectionSnapshot>,
    /// REST responses for conditional requests, so an unchanged list costs nothing.
    pub conditional: Vec<Conditional>,
    pub notifications_polled: Option<(DateTime<Utc>, u64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionSnapshot {
    pub view: ViewType,
    pub filters: String,
    pub fetched_at: DateTime<Utc>,
    #[serde(default)]
    pub prs: Vec<PullRequest>,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub notifications: Vec<Notification>,
    #[serde(default)]
    pub total_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    pub version: u32,
    pub prs: Vec<(String, Entry<EnrichedPullRequest>)>,
    pub issues: Vec<(String, Entry<Issue>)>,
}

#[derive(Clone, Debug)]
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    pub fn new(dir: PathBuf) -> DiskCache {
        DiskCache { dir }
    }

    /// The cache of the signed-in account in the platform cache directory.
    pub fn for_account(key: &str) -> Option<DiskCache> {
        Some(DiskCache::new(dirs::cache_dir()?.join("ghtui").join(key)))
    }

    pub fn load_lists(&self) -> Lists {
        load(&self.dir.join("lists.json"), |l: &Lists| l.version)
    }

    pub fn load_details(&self) -> Details {
        load(&self.dir.join("details.json"), |d: &Details| d.version)
    }

    /// Writes both files, merged with what another instance may have saved.
    pub fn save(&self, lists: Lists, details: Option<Details>) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let merged = merge_lists(self.load_lists(), lists);
        write_atomic(&self.dir.join("lists.json"), &serde_json::to_vec(&merged)?)?;
        if let Some(details) = details {
            let merged = merge_details(self.load_details(), details);
            write_atomic(
                &self.dir.join("details.json"),
                &serde_json::to_vec(&merged)?,
            )?;
        }
        Ok(())
    }
}

fn load<T: DeserializeOwned + Default>(path: &Path, version: impl Fn(&T) -> u32) -> T {
    let Ok(bytes) = std::fs::read(path) else {
        return T::default();
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(v) if version(&v) == VERSION => v,
        Ok(_) => {
            crate::warn!(
                "discarding cache from another version path={}",
                path.display()
            );
            T::default()
        }
        Err(e) => {
            crate::warn!("unreadable cache path={} err={e}", path.display());
            T::default()
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn newer<T>(a: Option<Stamped<T>>, b: Option<Stamped<T>>) -> Option<Stamped<T>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.at >= a.at { b } else { a }),
        (a, b) => b.or(a),
    }
}

/// Keeps the newer of each section, check time, login and release.
pub fn merge_lists(disk: Lists, new: Lists) -> Lists {
    let mut sections: HashMap<(ViewType, String), SectionSnapshot> = HashMap::new();
    for s in disk.sections.into_iter().chain(new.sections) {
        let key = (s.view, s.filters.clone());
        match sections.get(&key) {
            Some(cur) if cur.fetched_at > s.fetched_at => {}
            _ => {
                sections.insert(key, s);
            }
        }
    }
    let mut sections: Vec<SectionSnapshot> = sections.into_values().collect();
    sections.sort_by_key(|s| std::cmp::Reverse(s.fetched_at));
    let mut last_check: HashMap<ViewType, DateTime<Utc>> = HashMap::new();
    for (v, t) in disk.last_check.into_iter().chain(new.last_check) {
        let e = last_check.entry(v).or_insert(t);
        *e = (*e).max(t);
    }
    let mut conditional: HashMap<String, Conditional> = HashMap::new();
    for c in disk.conditional.into_iter().chain(new.conditional) {
        match conditional.get(&c.url) {
            Some(cur) if cur.at > c.at => {}
            _ => {
                conditional.insert(c.url.clone(), c);
            }
        }
    }
    let mut conditional: Vec<Conditional> = conditional.into_values().collect();
    conditional.sort_by_key(|c| std::cmp::Reverse(c.at));
    conditional.truncate(MAX_CONDITIONAL);
    let notifications_polled = match (disk.notifications_polled, new.notifications_polled) {
        (Some(a), Some(b)) => Some(if b.0 >= a.0 { b } else { a }),
        (a, b) => b.or(a),
    };
    Lists {
        version: VERSION,
        login: newer(disk.login, new.login),
        latest_version: newer(disk.latest_version, new.latest_version),
        last_check: last_check.into_iter().collect(),
        sections,
        conditional,
        notifications_polled,
    }
}

fn merge_entries<T>(
    disk: Vec<(String, Entry<T>)>,
    new: Vec<(String, Entry<T>)>,
    cap: usize,
) -> Vec<(String, Entry<T>)> {
    let mut by_url: HashMap<String, Entry<T>> = HashMap::new();
    for (url, e) in disk.into_iter().chain(new) {
        match by_url.get(&url) {
            Some(cur) if cur.fetched_at > e.fetched_at => {}
            _ => {
                by_url.insert(url, e);
            }
        }
    }
    let mut v: Vec<(String, Entry<T>)> = by_url.into_iter().collect();
    v.sort_by_key(|(_, e)| std::cmp::Reverse(e.fetched_at));
    v.truncate(cap);
    v
}

/// Keeps the newer of each detail, capped to the newest.
pub fn merge_details(disk: Details, new: Details) -> Details {
    Details {
        version: VERSION,
        prs: merge_entries(disk.prs, new.prs, MAX_DETAILS),
        issues: merge_entries(disk.issues, new.issues, MAX_DETAILS),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn temp_cache(name: &str) -> DiskCache {
        let dir = std::env::temp_dir().join(format!("ghtui-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        DiskCache::new(dir)
    }

    /// A PR search node as GitHub returns it.
    fn pr_fixture() -> PullRequest {
        serde_json::from_str(
            r#"{"id":"PR_1","number":7,"title":"Add sync","author":{"login":"juan"},
            "authorAssociation":"OWNER","updatedAt":"2026-11-07T10:00:00Z","createdAt":"2026-11-01T10:00:00Z",
            "url":"https://github.com/juanibiapina/ghtui/pull/7","state":"OPEN","mergeable":null,
            "reviewDecision":null,"additions":3,"deletions":1,"headRefName":"sync","baseRefName":"main",
            "headRepository":{"name":"ghtui"},"headRef":null,
            "repository":{"name":"ghtui","owner":{"login":"juanibiapina"},"nameWithOwner":"juanibiapina/ghtui","isArchived":false,"branchProtectionRules":{"nodes":[]}},
            "assignees":{"nodes":[{"login":"juan"}]},"comments":{"totalCount":2},"reviewThreads":{"totalCount":0},
            "reviews":{"totalCount":1},"reviewRequests":{"totalCount":0},"isDraft":false,"isInMergeQueue":false,
            "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]},
            "labels":{"nodes":[{"color":"a2eeef","name":"enhancement","description":null}]},"mergeStateStatus":"CLEAN"}"#,
        )
        .unwrap()
    }

    fn notification_fixture() -> Notification {
        serde_json::from_str(
            r#"{"id":"1","unread":true,"reason":"subscribed","updated_at":"2014-11-07T22:01:45Z",
            "last_read_at":null,"subject":{"title":"Greetings","url":"https://api.github.com/repos/octokit/octokit.rb/issues/123",
            "latest_comment_url":"https://api.github.com/repos/octokit/octokit.rb/issues/comments/123","type":"Issue"},
            "repository":{"id":1296269,"name":"Hello-World","full_name":"octocat/Hello-World","private":false,
            "owner":{"login":"octocat"},"html_url":"https://github.com/octocat/Hello-World"},
            "url":"https://api.github.com/notifications/threads/1",
            "subscription_url":"https://api.github.com/notifications/threads/1/subscription"}"#,
        )
        .unwrap()
    }

    fn section(filters: &str, at: DateTime<Utc>) -> SectionSnapshot {
        SectionSnapshot {
            view: ViewType::Prs,
            filters: filters.into(),
            fetched_at: at,
            prs: vec![pr_fixture()],
            issues: vec![],
            notifications: vec![notification_fixture()],
            total_count: 1,
        }
    }

    fn entry(at: DateTime<Utc>) -> Entry<EnrichedPullRequest> {
        Entry {
            value: EnrichedPullRequest::default(),
            fetched_at: at,
            stale: false,
        }
    }

    #[test]
    fn lists_and_details_survive_a_round_trip() {
        let cache = temp_cache("roundtrip");
        let now = Utc::now();
        let lists = Lists {
            login: Some(Stamped {
                value: "juan".into(),
                at: now,
            }),
            sections: vec![section("is:open", now)],
            ..Default::default()
        };
        let details = Details {
            prs: vec![("u".into(), entry(now))],
            ..Default::default()
        };
        cache.save(lists, Some(details)).unwrap();
        let lists = cache.load_lists();
        let s = &lists.sections[0];
        assert_eq!(s.prs[0].id, "PR_1");
        assert_eq!(s.prs[0].repository.name_with_owner, "juanibiapina/ghtui");
        assert_eq!(
            s.prs[0].commits.nodes[0].commit.status_check_rollup.state,
            "SUCCESS"
        );
        assert_eq!(s.notifications[0].subject.kind, "Issue");
        assert!(s.notifications[0].subscription.ends_with("/subscription"));
        assert_eq!(lists.login.unwrap().value, "juan");
        assert_eq!(cache.load_details().prs.len(), 1);
    }

    #[test]
    fn a_file_from_another_version_is_ignored() {
        let cache = temp_cache("version");
        std::fs::create_dir_all(&cache.dir).unwrap();
        std::fs::write(
            cache.dir.join("lists.json"),
            r#"{"version":999,"sections":[]}"#,
        )
        .unwrap();
        assert_eq!(cache.load_lists().version, 0);
        std::fs::write(cache.dir.join("lists.json"), "not json").unwrap();
        assert_eq!(cache.load_lists().version, 0);
    }

    #[test]
    fn saves_from_two_instances_keep_the_newer_entries() {
        let cache = temp_cache("merge");
        let now = Utc::now();
        let a = Lists {
            sections: vec![section("repo:a", now), section("repo:shared", now)],
            ..Default::default()
        };
        cache.save(a, None).unwrap();
        let b = Lists {
            sections: vec![
                section("repo:b", now),
                section("repo:shared", now - Duration::hours(1)),
            ],
            ..Default::default()
        };
        cache.save(b, None).unwrap();
        let lists = cache.load_lists();
        let filters: Vec<&str> = lists.sections.iter().map(|s| s.filters.as_str()).collect();
        assert_eq!(lists.sections.len(), 3);
        assert!(filters.contains(&"repo:a") && filters.contains(&"repo:b"));
        let shared = lists
            .sections
            .iter()
            .find(|s| s.filters == "repo:shared")
            .unwrap();
        assert_eq!(shared.fetched_at, now);
    }

    #[test]
    fn details_keep_the_newest_entries_up_to_the_cap() {
        let now = Utc::now();
        let new: Vec<(String, Entry<EnrichedPullRequest>)> = (0..MAX_DETAILS + 5)
            .map(|i| (format!("u{i}"), entry(now - Duration::minutes(i as i64))))
            .collect();
        let merged = merge_details(
            Details::default(),
            Details {
                prs: new,
                ..Default::default()
            },
        );
        assert_eq!(merged.prs.len(), MAX_DETAILS);
        assert_eq!(merged.prs[0].0, "u0");
        assert!(
            !merged
                .prs
                .iter()
                .any(|(u, _)| u == &format!("u{}", MAX_DETAILS + 4))
        );
    }
}
