//! Pure decisions behind keeping lists and details fresh: merging a list
//! sync into a section, the order to preload details in, and the notification
//! columns computed from a subject's details.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use chrono::{DateTime, Utc};

use crate::github::Membership;
use crate::model::{EnrichedPullRequest, Issue};
use crate::section::{NotifRow, PrRow};

/// A section row the list sync can match by node ID.
pub trait SyncRow: Clone {
    fn node_id(&self) -> &str;
    fn updated_at(&self) -> DateTime<Utc>;
}

impl SyncRow for PrRow {
    fn node_id(&self) -> &str {
        &self.primary.id
    }
    fn updated_at(&self) -> DateTime<Utc> {
        self.primary.updated_at
    }
}

impl SyncRow for Issue {
    fn node_id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// The ID and update time of each row, as the list sync compares them.
pub fn known<R: SyncRow>(rows: &[R]) -> Vec<(String, DateTime<Utc>)> {
    rows.iter()
        .map(|r| (r.node_id().to_string(), r.updated_at()))
        .collect()
}

/// IDs in `m` that are new to `known` or changed since.
pub fn changed_ids(known: &[(String, DateTime<Utc>)], m: &Membership) -> Vec<String> {
    let known: HashMap<&str, DateTime<Utc>> =
        known.iter().map(|(id, at)| (id.as_str(), *at)).collect();
    m.rows
        .iter()
        .filter(|(id, at)| known.get(id.as_str()) != Some(at))
        .map(|(id, _)| id.clone())
        .collect()
}

/// The section's rows in membership order: `fetched` rows where present,
/// otherwise the cached row. Rows that left the list are dropped.
pub fn apply_membership<R: SyncRow>(cached: &[R], m: &Membership, fetched: &[R]) -> Vec<R> {
    let cached: HashMap<&str, &R> = cached.iter().map(|r| (r.node_id(), r)).collect();
    let fetched: HashMap<&str, &R> = fetched.iter().map(|r| (r.node_id(), r)).collect();
    m.rows
        .iter()
        .filter_map(|(id, _)| fetched.get(id.as_str()).or(cached.get(id.as_str())))
        .map(|r| (*r).clone())
        .collect()
}

/// A section's candidates for preloading, aligned with its rows.
pub struct PrefetchRows<T> {
    pub items: Vec<Option<T>>,
    pub cursor: usize,
    pub top: usize,
}

/// What to preload, nearest first: the current section outward from the
/// cursor, then the visible rows of the view's other sections, then the first
/// screen of every other view. `screen` is the number of rows a table shows.
pub fn prefetch_order<T: Clone + Eq + Hash>(
    current: &PrefetchRows<T>,
    same_view: &[PrefetchRows<T>],
    other_views: &[PrefetchRows<T>],
    screen: usize,
) -> Vec<T> {
    let screen = screen.max(1);
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut push = |item: &Option<T>| {
        if let Some(t) = item
            && seen.insert(t.clone())
        {
            out.push(t.clone());
        }
    };
    let n = current.items.len();
    if n > 0 {
        let c = current.cursor.min(n - 1);
        push(&current.items[c]);
        for d in 1..=screen {
            if c + d < n {
                push(&current.items[c + d]);
            }
            if d <= c {
                push(&current.items[c - d]);
            }
        }
    }
    for s in same_view {
        for item in s.items.iter().skip(s.top).take(screen) {
            push(item);
        }
    }
    for s in other_views {
        for item in s.items.iter().take(screen) {
            push(item);
        }
    }
    out
}

/// A notification subject's details.
pub enum Subject<'a> {
    Pr(&'a EnrichedPullRequest),
    Issue(&'a Issue),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Extras {
    pub new_comments: i64,
    pub state: String,
    pub is_draft: bool,
    pub actor: String,
}

/// The new-comment count, state and latest actor of a notification.
pub fn notification_extras(row: &NotifRow, subject: Subject) -> Extras {
    let read_at = row.n.last_read_at;
    let after = |t: &DateTime<Utc>| read_at.map(|r| *t > r).unwrap_or(true);
    let mut activity: Vec<(DateTime<Utc>, &str)> = vec![];
    let (state, is_draft, author) = match subject {
        Subject::Pr(pr) => {
            for c in &pr.comments.nodes {
                activity.push((c.updated_at, &c.author.login));
            }
            for th in &pr.review_threads.nodes {
                for c in &th.comments.nodes {
                    activity.push((c.updated_at, &c.author.login));
                }
            }
            for r in &pr.reviews.nodes {
                activity.push((r.updated_at, &r.author.login));
            }
            (pr.state.clone(), pr.is_draft, pr.author.login.as_str())
        }
        Subject::Issue(i) => {
            for c in &i.comments.nodes {
                activity.push((c.updated_at, &c.author.login));
            }
            (i.state.clone(), false, i.author.login.as_str())
        }
    };
    let new: Vec<&(DateTime<Utc>, &str)> = activity.iter().filter(|(t, _)| after(t)).collect();
    let actor = new
        .iter()
        .max_by_key(|(t, _)| *t)
        .map(|(_, a)| a.to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| author.to_string());
    Extras {
        new_comments: new.len() as i64,
        state,
        is_draft,
        actor,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::model::{Comment, Login, Nodes, Notification, PageInfo, PullRequest, Review};

    fn row(id: &str, at: DateTime<Utc>, title: &str) -> PrRow {
        PrRow {
            primary: PullRequest {
                id: id.into(),
                updated_at: at,
                title: title.into(),
                ..Default::default()
            },
            enriched: None,
        }
    }

    fn membership(rows: &[(&str, DateTime<Utc>)]) -> Membership {
        Membership {
            total_count: rows.len(),
            page_info: PageInfo::default(),
            rows: rows.iter().map(|(i, t)| (i.to_string(), *t)).collect(),
        }
    }

    #[test]
    fn a_sync_reuses_unchanged_rows_and_takes_fetched_ones() {
        let t0 = Utc::now() - Duration::hours(1);
        let t1 = Utc::now();
        let cached = vec![
            row("a", t0, "old a"),
            row("b", t0, "b"),
            row("gone", t0, "gone"),
        ];
        let m = membership(&[("new", t1), ("a", t1), ("b", t0)]);
        assert_eq!(changed_ids(&known(&cached), &m), vec!["new", "a"]);
        let fetched = vec![row("a", t1, "new a"), row("new", t1, "new")];
        let rows = apply_membership(&cached, &m, &fetched);
        let titles: Vec<&str> = rows.iter().map(|r| r.primary.title.as_str()).collect();
        assert_eq!(titles, vec!["new", "new a", "b"]);
    }

    #[test]
    fn a_changed_row_that_failed_to_load_keeps_its_cached_version() {
        let t0 = Utc::now() - Duration::hours(1);
        let cached = vec![row("a", t0, "a")];
        let m = membership(&[("a", Utc::now()), ("unknown", Utc::now())]);
        let rows = apply_membership(&cached, &m, &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].primary.title, "a");
    }

    fn items(n: usize) -> Vec<Option<usize>> {
        (0..n).map(Some).collect()
    }

    #[test]
    fn preloading_starts_at_the_cursor_and_spreads_outward() {
        let current = PrefetchRows {
            items: items(10),
            cursor: 4,
            top: 0,
        };
        let order = prefetch_order(&current, &[], &[], 2);
        assert_eq!(order, vec![4, 5, 3, 6, 2]);
    }

    #[test]
    fn preloading_covers_visible_rows_then_other_views() {
        let current = PrefetchRows {
            items: vec![Some(1), None, Some(2)],
            cursor: 0,
            top: 0,
        };
        let other_section = PrefetchRows {
            items: vec![Some(10), Some(11), Some(12), Some(13)],
            cursor: 0,
            top: 1,
        };
        let other_view = PrefetchRows {
            items: vec![Some(20), Some(1), Some(21), Some(22)],
            cursor: 3,
            top: 2,
        };
        let order = prefetch_order(&current, &[other_section], &[other_view], 2);
        assert_eq!(order, vec![1, 2, 11, 12, 20]);
    }

    fn comment(login: &str, at: DateTime<Utc>) -> Comment {
        Comment {
            author: Login {
                login: login.into(),
            },
            body: String::new(),
            updated_at: at,
        }
    }

    #[test]
    fn extras_count_activity_since_the_last_read() {
        let read = Utc::now() - Duration::hours(2);
        let row = NotifRow::new(Notification {
            last_read_at: Some(read),
            ..Default::default()
        });
        let pr = EnrichedPullRequest {
            state: "OPEN".into(),
            is_draft: true,
            author: Login {
                login: "author".into(),
            },
            comments: Nodes {
                nodes: vec![
                    comment("old", read - Duration::hours(1)),
                    comment("bob", read + Duration::minutes(5)),
                ],
                total_count: 2,
            },
            reviews: Nodes {
                nodes: vec![Review {
                    author: Login {
                        login: "carol".into(),
                    },
                    updated_at: read + Duration::minutes(30),
                    ..Default::default()
                }],
                total_count: 1,
            },
            ..Default::default()
        };
        let e = notification_extras(&row, Subject::Pr(&pr));
        assert_eq!(
            e,
            Extras {
                new_comments: 2,
                state: "OPEN".into(),
                is_draft: true,
                actor: "carol".into(),
            }
        );
    }

    #[test]
    fn without_new_activity_the_actor_is_the_author() {
        let row = NotifRow::new(Notification {
            last_read_at: Some(Utc::now()),
            ..Default::default()
        });
        let issue = Issue {
            state: "CLOSED".into(),
            author: Login {
                login: "author".into(),
            },
            comments: Nodes {
                nodes: vec![comment("old", Utc::now() - Duration::days(1))],
                total_count: 1,
            },
            ..Default::default()
        };
        let e = notification_extras(&row, Subject::Issue(&issue));
        assert_eq!(e.new_comments, 0);
        assert_eq!(e.actor, "author");
        assert_eq!(e.state, "CLOSED");
    }
}
