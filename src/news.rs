//! What happened on a PR or an issue since a notification was last read.

use chrono::{DateTime, Duration, Utc};

use crate::model::{
    Assignee, Event, Issue, Label, Login, PullRequestDetails, ReviewState, TimelineEvent,
};

/// A notification subject's details.
#[derive(Clone, Copy)]
pub enum Subject<'a> {
    Pr(&'a PullRequestDetails),
    Issue(&'a Issue),
}

#[derive(Debug)]
pub struct News {
    pub since: Option<DateTime<Utc>>,
    /// Newest first.
    pub items: Vec<NewItem>,
}

#[derive(Debug, PartialEq)]
pub struct NewItem {
    pub at: DateTime<Utc>,
    pub actor: String,
    pub kind: NewKind,
}

#[derive(Debug, PartialEq)]
pub enum NewKind {
    Opened,
    Comment {
        body: String,
    },
    Review {
        state: ReviewState,
        body: String,
    },
    ReviewComment {
        path: String,
        line: Option<i64>,
        body: String,
    },
    /// Oldest first.
    Commits {
        headlines: Vec<String>,
    },
    ForcePushed,
    Closed,
    Reopened,
    Merged,
    ReviewRequested {
        reviewer: String,
    },
    Assigned {
        assignee: String,
    },
    Unassigned {
        assignee: String,
    },
    Labeled {
        label: Label,
    },
    Unlabeled {
        label: Label,
    },
    Renamed {
        from: String,
        to: String,
    },
    ReadyForReview,
    ConvertedToDraft,
}

impl NewKind {
    fn is_comment(&self) -> bool {
        matches!(
            self,
            NewKind::Comment { .. } | NewKind::Review { .. } | NewKind::ReviewComment { .. }
        )
    }
}

impl News {
    /// Whether something created at `at` counts as new.
    pub fn is_new(&self, at: DateTime<Utc>) -> bool {
        self.since.is_none_or(|s| at > s)
    }

    /// New comments, reviews and review comments.
    pub fn comment_count(&self) -> usize {
        self.items.iter().filter(|i| i.kind.is_comment()).count()
    }

    /// The newest commenter, else whoever acted last.
    pub fn latest_actor(&self) -> Option<&str> {
        let named = |i: &&NewItem| !i.actor.is_empty();
        self.items
            .iter()
            .filter(named)
            .find(|i| i.kind.is_comment())
            .or_else(|| self.items.iter().find(named))
            .map(|i| i.actor.as_str())
    }
}

/// Everything created after `last_read`, or everything when it was never read.
pub fn since(subject: Subject, last_read: Option<DateTime<Utc>>) -> News {
    let mut news = News {
        since: last_read,
        items: vec![],
    };
    let mut all: Vec<NewItem> = vec![];
    let mut push = |at: DateTime<Utc>, actor: &str, kind: NewKind| {
        all.push(NewItem {
            at,
            actor: actor.to_string(),
            kind,
        })
    };
    let (created_at, author, comments, events) = match subject {
        Subject::Pr(pr) => (
            pr.created_at,
            &pr.author,
            &pr.comments.nodes,
            &pr.timeline_items.nodes,
        ),
        Subject::Issue(i) => (
            i.created_at,
            &i.author,
            &i.comments.nodes,
            &i.timeline_items.nodes,
        ),
    };
    push(created_at, Login::of(author), NewKind::Opened);
    for c in comments {
        push(
            c.created_at,
            Login::of(&c.author),
            NewKind::Comment {
                body: c.body.clone(),
            },
        );
    }
    if let Subject::Pr(pr) = subject {
        for r in &pr.reviews.nodes {
            let Some(at) = r.submitted_at else { continue };
            let wraps_comments = r.state == ReviewState::Commented && r.body.trim().is_empty();
            if r.state == ReviewState::Pending || wraps_comments {
                continue;
            }
            push(
                at,
                Login::of(&r.author),
                NewKind::Review {
                    state: r.state,
                    body: r.body.clone(),
                },
            );
        }
        for th in &pr.review_threads.nodes {
            for c in &th.comments.nodes {
                push(
                    c.created_at,
                    Login::of(&c.author),
                    NewKind::ReviewComment {
                        path: th.path.clone(),
                        line: th.line,
                        body: c.body.clone(),
                    },
                );
            }
        }
        for c in &pr.all_commits.nodes {
            let c = &c.commit;
            let actor = c
                .author
                .as_ref()
                .and_then(|a| a.user.as_ref().map(|u| u.login.clone()).or(a.name.clone()))
                .unwrap_or_default();
            push(
                c.committed_date,
                &actor,
                NewKind::Commits {
                    headlines: vec![c.message_headline.clone()],
                },
            );
        }
    }
    let merges: Vec<DateTime<Utc>> = events
        .iter()
        .filter_map(|e| match e {
            TimelineEvent::Merged(e) => Some(e.created_at),
            _ => None,
        })
        .collect();
    for e in events {
        let mut event = |e: &Event, kind: NewKind| push(e.created_at, Login::of(&e.actor), kind);
        let assignee =
            |a: &Option<Assignee>| a.as_ref().and_then(|a| a.login.clone()).unwrap_or_default();
        match e {
            TimelineEvent::Closed(e)
                if merges
                    .iter()
                    .any(|m| (*m - e.created_at).abs() < Duration::seconds(5)) => {}
            TimelineEvent::Closed(e) => event(e, NewKind::Closed),
            TimelineEvent::Reopened(e) => event(e, NewKind::Reopened),
            TimelineEvent::Merged(e) => event(e, NewKind::Merged),
            TimelineEvent::ReadyForReview(e) => event(e, NewKind::ReadyForReview),
            TimelineEvent::ConvertedToDraft(e) => event(e, NewKind::ConvertedToDraft),
            TimelineEvent::ForcePushed(e) => event(e, NewKind::ForcePushed),
            TimelineEvent::ReviewRequested(e) => {
                let Some(reviewer) = e.requested_reviewer.as_ref().and_then(|r| r.name()) else {
                    continue;
                };
                event(
                    &e.event,
                    NewKind::ReviewRequested {
                        reviewer: reviewer.to_string(),
                    },
                )
            }
            TimelineEvent::Assigned(e) => event(
                &e.event,
                NewKind::Assigned {
                    assignee: assignee(&e.assignee),
                },
            ),
            TimelineEvent::Unassigned(e) => event(
                &e.event,
                NewKind::Unassigned {
                    assignee: assignee(&e.assignee),
                },
            ),
            TimelineEvent::Labeled(e) => event(
                &e.event,
                NewKind::Labeled {
                    label: e.label.clone(),
                },
            ),
            TimelineEvent::Unlabeled(e) => event(
                &e.event,
                NewKind::Unlabeled {
                    label: e.label.clone(),
                },
            ),
            TimelineEvent::Renamed(e) => event(
                &e.event,
                NewKind::Renamed {
                    from: e.previous_title.clone(),
                    to: e.current_title.clone(),
                },
            ),
            TimelineEvent::Unknown => {}
        }
    }
    all.retain(|i| news.is_new(i.at));
    all.sort_by_key(|i| i.at);
    for item in all {
        if let (
            Some(NewItem {
                at,
                actor,
                kind: NewKind::Commits { headlines },
            }),
            NewKind::Commits { headlines: more },
        ) = (news.items.last_mut(), &item.kind)
            && *actor == item.actor
        {
            headlines.extend(more.iter().cloned());
            *at = item.at;
            continue;
        }
        news.items.push(item);
    }
    news.items.reverse();
    news
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Comment, CommitAuthor, CommitNode, CountedNodes, HistoryCommit, Nodes, RequestedReviewer,
        Review, ReviewRequestedEvent, ReviewThread,
    };

    fn read_at() -> DateTime<Utc> {
        "2026-01-01T12:00:00Z".parse().unwrap()
    }

    fn at(minutes: i64) -> DateTime<Utc> {
        read_at() + Duration::minutes(minutes)
    }

    fn login(name: &str) -> Option<Login> {
        Some(Login { login: name.into() })
    }

    fn comment(who: &str, minutes: i64) -> Comment {
        Comment {
            author: login(who),
            body: format!("{who} says"),
            created_at: at(minutes),
            updated_at: at(minutes),
        }
    }

    fn event(who: &str, minutes: i64) -> Event {
        Event {
            actor: login(who),
            created_at: at(minutes),
        }
    }

    fn commit(who: &str, minutes: i64, headline: &str) -> CommitNode<HistoryCommit> {
        CommitNode {
            commit: HistoryCommit {
                abbreviated_oid: String::new(),
                committed_date: at(minutes),
                message_headline: headline.into(),
                author: Some(CommitAuthor {
                    name: None,
                    user: login(who),
                }),
                status_check_rollup: None,
            },
        }
    }

    fn review(who: &str, minutes: i64, state: ReviewState, body: &str) -> Review {
        Review {
            author: login(who),
            body: body.into(),
            state,
            updated_at: at(minutes),
            submitted_at: Some(at(minutes)),
        }
    }

    fn counted<T>(nodes: Vec<T>) -> CountedNodes<T> {
        CountedNodes {
            total_count: nodes.len() as i64,
            nodes,
        }
    }

    fn pr() -> PullRequestDetails {
        PullRequestDetails {
            author: login("author"),
            created_at: at(-600),
            ..Default::default()
        }
    }

    fn kinds(news: &News) -> Vec<(&str, &NewKind)> {
        news.items
            .iter()
            .map(|i| (i.actor.as_str(), &i.kind))
            .collect()
    }

    #[test]
    fn only_activity_after_the_last_read_is_new_and_newest_comes_first() {
        let mut pr = pr();
        pr.comments = counted(vec![comment("old", -5), comment("bob", 10)]);
        pr.reviews = counted(vec![review("carol", 20, ReviewState::Approved, "")]);
        pr.review_threads = counted(vec![ReviewThread {
            id: String::new(),
            is_outdated: false,
            line: Some(12),
            path: "src/a.rs".into(),
            comments: counted(vec![comment("dan", 30)]),
        }]);
        pr.timeline_items = Nodes {
            nodes: vec![
                TimelineEvent::Labeled(crate::model::LabelEvent {
                    event: event("old", -1),
                    label: Label::default(),
                }),
                TimelineEvent::Merged(event("erin", 40)),
            ],
        };
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(
            kinds(&news),
            vec![
                ("erin", &NewKind::Merged),
                (
                    "dan",
                    &NewKind::ReviewComment {
                        path: "src/a.rs".into(),
                        line: Some(12),
                        body: "dan says".into(),
                    }
                ),
                (
                    "carol",
                    &NewKind::Review {
                        state: ReviewState::Approved,
                        body: String::new(),
                    }
                ),
                (
                    "bob",
                    &NewKind::Comment {
                        body: "bob says".into()
                    }
                ),
            ]
        );
        assert_eq!(news.comment_count(), 3);
        assert_eq!(news.latest_actor(), Some("dan"));
    }

    #[test]
    fn an_old_comment_edited_after_the_last_read_is_not_new() {
        let mut edited = comment("bob", -60);
        edited.updated_at = at(60);
        let issue = Issue {
            comments: counted(vec![edited]),
            created_at: at(-600),
            ..Default::default()
        };
        assert!(
            since(Subject::Issue(&issue), Some(read_at()))
                .items
                .is_empty()
        );
    }

    #[test]
    fn a_thread_never_read_is_new_from_its_opening() {
        let mut pr = pr();
        pr.comments = counted(vec![comment("bob", -5)]);
        let news = since(Subject::Pr(&pr), None);
        assert_eq!(news.items.len(), 2);
        assert_eq!(news.items[1].kind, NewKind::Opened);
        assert_eq!(news.items[1].actor, "author");
    }

    #[test]
    fn consecutive_commits_by_one_author_merge() {
        let mut pr = pr();
        pr.all_commits = Nodes {
            nodes: vec![
                commit("bob", 1, "one"),
                commit("bob", 2, "two"),
                commit("eve", 3, "three"),
            ],
        };
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(
            kinds(&news),
            vec![
                (
                    "eve",
                    &NewKind::Commits {
                        headlines: vec!["three".into()]
                    }
                ),
                (
                    "bob",
                    &NewKind::Commits {
                        headlines: vec!["one".into(), "two".into()]
                    }
                ),
            ]
        );
        assert_eq!(news.items[1].at, at(2));
    }

    #[test]
    fn pending_reviews_and_reviews_that_only_wrap_comments_are_left_out() {
        let mut pr = pr();
        let mut pending = review("bob", 5, ReviewState::Pending, "draft");
        pending.submitted_at = None;
        pr.reviews = counted(vec![
            pending,
            review("carol", 6, ReviewState::Commented, " "),
            review("dan", 7, ReviewState::Commented, "looks fine"),
        ]);
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(news.items.len(), 1);
        assert_eq!(news.items[0].actor, "dan");
    }

    #[test]
    fn a_merge_hides_the_close_it_causes() {
        let mut pr = pr();
        pr.timeline_items = Nodes {
            nodes: vec![
                TimelineEvent::Merged(event("erin", 40)),
                TimelineEvent::Closed(event("erin", 40)),
            ],
        };
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(kinds(&news), vec![("erin", &NewKind::Merged)]);
    }

    #[test]
    fn review_requests_name_the_user_or_team() {
        let mut pr = pr();
        pr.timeline_items = Nodes {
            nodes: vec![
                TimelineEvent::ReviewRequested(ReviewRequestedEvent {
                    event: event("bob", 1),
                    requested_reviewer: Some(RequestedReviewer::Team {
                        slug: "core".into(),
                        name: "Core".into(),
                    }),
                }),
                TimelineEvent::ReviewRequested(ReviewRequestedEvent {
                    event: event("bob", 2),
                    requested_reviewer: None,
                }),
            ],
        };
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(
            kinds(&news),
            vec![(
                "bob",
                &NewKind::ReviewRequested {
                    reviewer: "core".into()
                }
            )]
        );
    }

    #[test]
    fn without_comments_the_latest_actor_is_whoever_acted_last() {
        let mut pr = pr();
        pr.timeline_items = Nodes {
            nodes: vec![TimelineEvent::Closed(event("erin", 3))],
        };
        let news = since(Subject::Pr(&pr), Some(read_at()));
        assert_eq!(news.latest_actor(), Some("erin"));
        assert_eq!(news.comment_count(), 0);
    }
}
