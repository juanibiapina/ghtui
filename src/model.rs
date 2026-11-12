//! GitHub data shapes. Field names follow the GraphQL/REST responses. Nullable
//! fields are `Option`s, and enums decode values GitHub adds later as `Unknown`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

/// A GraphQL list that may be null or hold nulls: the items that are present.
fn present<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<Option<T>>>::deserialize(d)?
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect())
}

/// The state of a PR or an issue.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(test, derive(Default))]
pub enum State {
    #[cfg_attr(test, default)]
    Open,
    Closed,
    Merged,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(test, derive(Default))]
pub enum MergeableState {
    Mergeable,
    Conflicting,
    #[cfg_attr(test, default)]
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(test, derive(Default))]
pub enum AuthorAssociation {
    Member,
    Owner,
    Mannequin,
    Collaborator,
    Contributor,
    FirstTimeContributor,
    FirstTimer,
    None,
    #[cfg_attr(test, default)]
    #[serde(other)]
    Unknown,
}

impl AuthorAssociation {
    /// The role as GitHub names it, in lowercase.
    pub fn label(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Owner => "owner",
            Self::Mannequin => "mannequin",
            Self::Collaborator => "collaborator",
            Self::Contributor => "contributor",
            Self::FirstTimeContributor => "first_time_contributor",
            Self::FirstTimer => "first_timer",
            Self::None => "none",
            Self::Unknown => "unknown role",
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn flag(self) -> &'static str {
        match self {
            MergeMethod::Merge => "--merge",
            MergeMethod::Squash => "--squash",
            MergeMethod::Rebase => "--rebase",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(test, derive(Default))]
pub enum MergeStateStatus {
    Dirty,
    Blocked,
    Behind,
    Draft,
    Unstable,
    HasHooks,
    Clean,
    #[cfg_attr(test, default)]
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(test, derive(Default))]
pub enum ReviewState {
    Pending,
    #[cfg_attr(test, default)]
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PatchStatus {
    Added,
    Deleted,
    Renamed,
    Copied,
    Modified,
    Changed,
    #[serde(other)]
    Unknown,
}

/// The states, statuses and conclusions of checks, status contexts and their
/// rollup. GitHub spreads these over several enums that share one vocabulary.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckState {
    ActionRequired,
    Cancelled,
    Completed,
    Error,
    Expected,
    Failure,
    InProgress,
    Neutral,
    Pending,
    Queued,
    Requested,
    Skipped,
    Stale,
    StartupFailure,
    Success,
    TimedOut,
    Waiting,
    #[serde(other)]
    Unknown,
}

impl CheckState {
    pub fn is_waiting(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Queued | Self::InProgress | Self::Waiting
        )
    }
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failure | Self::TimedOut | Self::StartupFailure)
    }
    pub fn is_skip(self) -> bool {
        self == Self::Skipped
    }
    pub fn is_neutral(self) -> bool {
        self == Self::Neutral
    }
    pub fn is_success(self) -> bool {
        self == Self::Success
    }
}

/// The kind of thing a notification is about.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(Default))]
pub enum SubjectType {
    #[cfg_attr(test, default)]
    PullRequest,
    Issue,
    CheckSuite,
    Commit,
    Discussion,
    Release,
    RepositoryVulnerabilityAlert,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(Default))]
pub struct Login {
    pub login: String,
}

impl Login {
    /// The login of an author GitHub may have deleted, shown as GitHub does.
    pub fn of(author: &Option<Login>) -> &str {
        author.as_ref().map_or("ghost", |a| a.login.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct NameOnly {
    pub name: String,
}

/// A connection's nodes. A null connection or node counts as absent.
#[derive(Serialize, Clone, Debug)]
pub struct Nodes<T> {
    pub nodes: Vec<T>,
}

/// A connection's nodes and its total count. A null connection is empty.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CountedNodes<T> {
    pub nodes: Vec<T>,
    pub total_count: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "T: Deserialize<'de>"))]
struct RawConnection<T> {
    #[serde(deserialize_with = "present")]
    nodes: Vec<T>,
    total_count: Option<i64>,
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nodes<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Option::<RawConnection<T>>::deserialize(d)?;
        Ok(Nodes {
            nodes: raw.map(|r| r.nodes).unwrap_or_default(),
        })
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CountedNodes<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Some(raw) = Option::<RawConnection<T>>::deserialize(d)? else {
            return Ok(CountedNodes {
                nodes: vec![],
                total_count: 0,
            });
        };
        let total_count = raw
            .total_count
            .ok_or_else(|| serde::de::Error::missing_field("totalCount"))?;
        Ok(CountedNodes {
            nodes: raw.nodes,
            total_count,
        })
    }
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Nodes { nodes: vec![] }
    }
}

#[cfg(test)]
impl<T> Default for CountedNodes<T> {
    fn default() -> Self {
        CountedNodes {
            nodes: vec![],
            total_count: 0,
        }
    }
}

/// A connection's total count. A null connection counts zero.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Count {
    pub total_count: i64,
}

impl<'de> Deserialize<'de> for Count {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            total_count: i64,
        }
        Ok(Count {
            total_count: Option::<Raw>::deserialize(d)?.map_or(0, |r| r.total_count),
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct BranchProtectionRule {
    pub required_approving_review_count: Option<i64>,
    pub requires_approving_reviews: bool,
    pub requires_code_owner_reviews: bool,
    pub requires_status_checks: bool,
    #[serde(deserialize_with = "present")]
    pub required_status_check_contexts: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Repository {
    pub name: String,
    pub owner: Login,
    pub name_with_owner: String,
    pub is_archived: bool,
    pub branch_protection_rules: Nodes<BranchProtectionRule>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(Default))]
pub struct Label {
    pub color: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RollupState {
    pub state: CheckState,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitWithRollup {
    pub status_check_rollup: Option<RollupState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitNode<T> {
    pub commit: T,
}

/// A row in a PR section.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct PullRequest {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub author: Option<Login>,
    pub author_association: AuthorAssociation,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub state: State,
    pub mergeable: MergeableState,
    pub review_decision: Option<ReviewDecision>,
    pub additions: i64,
    pub deletions: i64,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_repository: Option<NameOnly>,
    pub head_ref: Option<NameOnly>,
    pub repository: Repository,
    pub assignees: Nodes<Login>,
    pub comments: Count,
    pub review_threads: Count,
    pub reviews: Count,
    pub review_requests: Count,
    pub is_draft: bool,
    pub is_in_merge_queue: bool,
    pub commits: Nodes<CommitNode<CommitWithRollup>>,
    pub labels: Nodes<Label>,
    pub merge_state_status: MergeStateStatus,
}

impl PullRequest {
    /// The rollup state of the head commit's checks.
    pub fn rollup_state(&self) -> Option<CheckState> {
        self.commits
            .nodes
            .first()
            .and_then(|c| c.commit.status_check_rollup.as_ref())
            .map(|r| r.state)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CountByState {
    pub count: i64,
    pub state: CheckState,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkflowName {
    pub workflow: NameOnly,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckSuiteRef {
    pub creator: Option<Login>,
    pub workflow_run: Option<WorkflowName>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckRun {
    pub name: String,
    pub status: CheckState,
    pub conclusion: Option<CheckState>,
    pub check_suite: CheckSuiteRef,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StatusContext {
    pub context: String,
    pub state: CheckState,
    pub creator: Option<Login>,
}

/// A node of statusCheckRollup.contexts.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "__typename")]
pub enum CheckContext {
    CheckRun(CheckRun),
    StatusContext(StatusContext),
    #[serde(other)]
    Unknown,
}

/// Check run and status context counts by state.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContextCounts {
    pub total_count: i64,
    pub check_run_count: i64,
    #[serde(deserialize_with = "present")]
    pub check_run_counts_by_state: Vec<CountByState>,
    pub status_context_count: i64,
    #[serde(deserialize_with = "present")]
    pub status_context_counts_by_state: Vec<CountByState>,
}

impl ContextCounts {
    /// Every count by state, merged and sorted.
    pub fn by_state(&self) -> Vec<CountByState> {
        let mut v: Vec<CountByState> = self
            .check_run_counts_by_state
            .iter()
            .chain(&self.status_context_counts_by_state)
            .cloned()
            .collect();
        v.sort();
        v
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RollupContexts {
    #[serde(flatten)]
    pub counts: ContextCounts,
    #[serde(deserialize_with = "present")]
    pub nodes: Vec<CheckContext>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StatusCheckRollup {
    pub state: CheckState,
    pub contexts: RollupContexts,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StatusCheckCounts {
    pub state: CheckState,
    pub contexts: ContextCounts,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckSuiteNode {
    pub status: CheckState,
    pub conclusion: Option<CheckState>,
    pub app: Option<NameOnly>,
    pub workflow_run: Option<WorkflowName>,
}

impl CheckSuiteNode {
    pub fn is_awaiting_approval(&self) -> bool {
        self.conclusion == Some(CheckState::ActionRequired)
    }
    pub fn is_pending(&self) -> bool {
        self.workflow_run.is_some()
            && matches!(
                self.status,
                CheckState::Queued | CheckState::Pending | CheckState::Waiting
            )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LastCommit {
    pub oid: String,
    pub commit_url: String,
    pub status_check_rollup: Option<StatusCheckRollup>,
    pub check_suites: CountedNodes<CheckSuiteNode>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitAuthor {
    pub name: Option<String>,
    pub user: Option<Login>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCommit {
    pub abbreviated_oid: String,
    pub committed_date: DateTime<Utc>,
    pub message_headline: String,
    pub author: Option<CommitAuthor>,
    pub status_check_rollup: Option<StatusCheckCounts>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Comment {
    pub author: Option<Login>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReviewThread {
    pub id: String,
    pub is_outdated: bool,
    pub line: Option<i64>,
    pub path: String,
    pub comments: CountedNodes<Comment>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Review {
    pub author: Option<Login>,
    pub body: String,
    pub state: ReviewState,
    pub updated_at: DateTime<Utc>,
    /// Absent while the review is pending.
    pub submitted_at: Option<DateTime<Utc>>,
}

/// A timeline event's actor and time.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub actor: Option<Login>,
    pub created_at: DateTime<Utc>,
}

/// The user, bot, mannequin or organization an assignment names.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Assignee {
    pub login: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequestedEvent {
    #[serde(flatten)]
    pub event: Event,
    pub requested_reviewer: Option<RequestedReviewer>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssigneeEvent {
    #[serde(flatten)]
    pub event: Event,
    pub assignee: Option<Assignee>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LabelEvent {
    #[serde(flatten)]
    pub event: Event,
    pub label: Label,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RenamedTitleEvent {
    #[serde(flatten)]
    pub event: Event,
    pub previous_title: String,
    pub current_title: String,
}

/// A node of a PR's or issue's timelineItems.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "__typename")]
pub enum TimelineEvent {
    #[serde(rename = "ClosedEvent")]
    Closed(Event),
    #[serde(rename = "ReopenedEvent")]
    Reopened(Event),
    #[serde(rename = "MergedEvent")]
    Merged(Event),
    #[serde(rename = "ReadyForReviewEvent")]
    ReadyForReview(Event),
    #[serde(rename = "ConvertToDraftEvent")]
    ConvertedToDraft(Event),
    #[serde(rename = "HeadRefForcePushedEvent")]
    ForcePushed(Event),
    #[serde(rename = "ReviewRequestedEvent")]
    ReviewRequested(ReviewRequestedEvent),
    #[serde(rename = "AssignedEvent")]
    Assigned(AssigneeEvent),
    #[serde(rename = "UnassignedEvent")]
    Unassigned(AssigneeEvent),
    #[serde(rename = "LabeledEvent")]
    Labeled(LabelEvent),
    #[serde(rename = "UnlabeledEvent")]
    Unlabeled(LabelEvent),
    #[serde(rename = "RenamedTitleEvent")]
    Renamed(RenamedTitleEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "__typename")]
pub enum RequestedReviewer {
    User {
        login: String,
    },
    Bot {
        login: String,
    },
    Mannequin {
        login: String,
    },
    Team {
        slug: String,
        name: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    pub as_code_owner: bool,
    pub requested_reviewer: Option<RequestedReviewer>,
}

impl RequestedReviewer {
    /// The user's login or the team's slug.
    pub fn name(&self) -> Option<&str> {
        match self {
            RequestedReviewer::User { login }
            | RequestedReviewer::Bot { login }
            | RequestedReviewer::Mannequin { login } => Some(login),
            RequestedReviewer::Team { slug, .. } => Some(slug),
            RequestedReviewer::Unknown => None,
        }
    }
}

impl ReviewRequest {
    /// The requested user's login or team's slug.
    pub fn display_name(&self) -> Option<&str> {
        self.requested_reviewer.as_ref()?.name()
    }
    pub fn is_team(&self) -> bool {
        matches!(
            self.requested_reviewer,
            Some(RequestedReviewer::Team { .. })
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedReviewer {
    pub is_author: bool,
    pub is_commenter: bool,
    pub reviewer: Login,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub additions: i64,
    pub deletions: i64,
    pub path: String,
    pub change_type: PatchStatus,
}

/// The full PR fetched for the preview pane.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct PullRequestDetails {
    pub id: String,
    pub url: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub state: State,
    pub is_draft: bool,
    pub author: Option<Login>,
    pub author_association: AuthorAssociation,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub mergeable: MergeableState,
    pub review_decision: Option<ReviewDecision>,
    pub additions: i64,
    pub deletions: i64,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_repository: Option<NameOnly>,
    pub head_ref: Option<NameOnly>,
    pub labels: Nodes<Label>,
    pub assignees: Nodes<Login>,
    pub repository: Repository,
    pub commits: CountedNodes<CommitNode<LastCommit>>,
    pub all_commits: Nodes<CommitNode<HistoryCommit>>,
    pub comments: CountedNodes<Comment>,
    pub review_threads: CountedNodes<ReviewThread>,
    pub review_requests: CountedNodes<ReviewRequest>,
    pub reviews: CountedNodes<Review>,
    #[serde(deserialize_with = "present")]
    pub suggested_reviewers: Vec<SuggestedReviewer>,
    pub files: CountedNodes<ChangedFile>,
    pub merge_state_status: MergeStateStatus,
    pub is_in_merge_queue: bool,
    pub timeline_items: Nodes<TimelineEvent>,
}

impl PullRequestDetails {
    /// The head commit, the last of `commits`.
    pub fn head(&self) -> Option<&LastCommit> {
        self.commits.nodes.last().map(|c| &c.commit)
    }
}

impl From<&PullRequestDetails> for PullRequest {
    fn from(d: &PullRequestDetails) -> Self {
        let rollup = d
            .head()
            .and_then(|c| c.status_check_rollup.as_ref())
            .map(|r| RollupState { state: r.state });
        PullRequest {
            id: d.id.clone(),
            number: d.number,
            title: d.title.clone(),
            author: d.author.clone(),
            author_association: d.author_association,
            updated_at: d.updated_at,
            created_at: d.created_at,
            url: d.url.clone(),
            state: d.state,
            mergeable: d.mergeable,
            review_decision: d.review_decision,
            additions: d.additions,
            deletions: d.deletions,
            head_ref_name: d.head_ref_name.clone(),
            base_ref_name: d.base_ref_name.clone(),
            head_repository: d.head_repository.clone(),
            head_ref: d.head_ref.clone(),
            repository: d.repository.clone(),
            assignees: d.assignees.clone(),
            comments: Count {
                total_count: d.comments.total_count,
            },
            review_threads: Count {
                total_count: d.review_threads.total_count,
            },
            reviews: Count {
                total_count: d.reviews.total_count,
            },
            review_requests: Count {
                total_count: d.review_requests.total_count,
            },
            is_draft: d.is_draft,
            is_in_merge_queue: d.is_in_merge_queue,
            commits: Nodes {
                nodes: vec![CommitNode {
                    commit: CommitWithRollup {
                        status_check_rollup: rollup,
                    },
                }],
            },
            labels: d.labels.clone(),
            merge_state_status: d.merge_state_status,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Issue {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub state: State,
    pub author: Option<Login>,
    pub author_association: AuthorAssociation,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub repository: Repository,
    pub assignees: Nodes<Login>,
    pub comments: CountedNodes<Comment>,
    pub reactions: Count,
    pub labels: Nodes<Label>,
    /// Only fetched with an issue's details; list searches leave it empty.
    #[serde(default)]
    pub timeline_items: Nodes<TimelineEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct PageInfo {
    pub has_next_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct NotificationSubject {
    pub title: String,
    pub url: Option<String>,
    pub latest_comment_url: Option<String>,
    #[serde(rename = "type")]
    pub kind: SubjectType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct NotificationRepository {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub owner: Login,
    pub html_url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Notification {
    pub id: String,
    pub reason: String,
    pub updated_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub subject: NotificationSubject,
    pub repository: NotificationRepository,
    pub url: String,
    pub subscription_url: String,
}

/// The status or conclusion of a workflow run, job or step, as REST names it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Default))]
pub enum RunState {
    ActionRequired,
    Cancelled,
    #[cfg_attr(test, default)]
    Completed,
    Failure,
    InProgress,
    Neutral,
    Pending,
    Queued,
    Requested,
    Skipped,
    Stale,
    StartupFailure,
    Success,
    TimedOut,
    Waiting,
    #[serde(other)]
    Unknown,
}

impl RunState {
    pub fn is_waiting(self) -> bool {
        matches!(
            self,
            Self::InProgress | Self::Pending | Self::Queued | Self::Requested | Self::Waiting
        )
    }
    /// Failed, cancelled, timed out or waiting for approval.
    pub fn is_unsuccessful(self) -> bool {
        matches!(
            self,
            Self::Failure
                | Self::Cancelled
                | Self::TimedOut
                | Self::StartupFailure
                | Self::ActionRequired
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct WorkflowRun {
    pub id: i64,
    /// The workflow's name.
    pub name: Option<String>,
    pub display_title: String,
    pub html_url: String,
    pub status: Option<RunState>,
    pub conclusion: Option<RunState>,
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub event: String,
    pub run_number: i64,
    pub run_attempt: Option<i64>,
    pub actor: Option<Login>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkflowRunsResponse {
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Step {
    pub name: String,
    pub status: RunState,
    pub conclusion: Option<RunState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Job {
    pub name: String,
    pub status: RunState,
    pub conclusion: Option<RunState>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JobsResponse {
    pub jobs: Vec<Job>,
}

/// A workflow run and the jobs of the attempt a notification is about.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct RunDetails {
    pub run: WorkflowRun,
    pub attempt: i64,
    pub jobs: Vec<Job>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Asset {
    pub name: String,
    pub size: i64,
    pub download_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct Release {
    pub name: Option<String>,
    pub tag_name: String,
    pub body: Option<String>,
    pub author: Option<Login>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

/// How a commit changed a file, as REST names it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
    Renamed,
    Copied,
    Changed,
    Unchanged,
    #[serde(other)]
    Unknown,
}

impl From<FileStatus> for PatchStatus {
    fn from(s: FileStatus) -> Self {
        match s {
            FileStatus::Added => PatchStatus::Added,
            FileStatus::Removed => PatchStatus::Deleted,
            FileStatus::Modified | FileStatus::Unchanged => PatchStatus::Modified,
            FileStatus::Renamed => PatchStatus::Renamed,
            FileStatus::Copied => PatchStatus::Copied,
            FileStatus::Changed => PatchStatus::Changed,
            FileStatus::Unknown => PatchStatus::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitFile {
    pub filename: String,
    pub status: FileStatus,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct CommitStats {
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct GitAuthor {
    pub name: Option<String>,
    pub date: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct GitCommit {
    pub message: String,
    pub author: Option<GitAuthor>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct RestCommit {
    pub sha: String,
    pub html_url: String,
    pub commit: GitCommit,
    /// The GitHub account, when the author's email maps to one.
    pub author: Option<Login>,
    pub stats: Option<CommitStats>,
    #[serde(default)]
    pub files: Vec<CommitFile>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitComment {
    pub user: Option<Login>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Default))]
pub struct CommitDetails {
    pub commit: RestCommit,
    pub comments: Vec<CommitComment>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(Default))]
pub struct Discussion {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub body: String,
    pub author: Option<Login>,
    pub created_at: DateTime<Utc>,
    pub category: NameOnly,
    pub is_answered: Option<bool>,
    pub comments: CountedNodes<Comment>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETAILS: &str = r#"{
        "id":"PR_1","url":"https://github.com/o/r/pull/1","number":1,"title":"T","body":"",
        "state":"OPEN","isDraft":false,"author":null,"authorAssociation":"SOMETHING_NEW",
        "updatedAt":"2026-11-07T10:00:00Z","createdAt":"2026-11-01T10:00:00Z",
        "mergeable":"MERGEABLE","reviewDecision":null,"additions":1,"deletions":0,
        "headRefName":"b","baseRefName":"main","headRepository":null,"headRef":null,
        "labels":null,"assignees":{"nodes":[null]},
        "repository":{"name":"r","owner":{"login":"o"},"nameWithOwner":"o/r","isArchived":false,
            "branchProtectionRules":{"nodes":[]}},
        "commits":{"totalCount":1,"nodes":[{"commit":{"oid":"abc","commitUrl":"u",
            "statusCheckRollup":{"state":"FAILURE","contexts":{"totalCount":2,"checkRunCount":1,
                "checkRunCountsByState":[{"count":1,"state":"FAILURE"}],"statusContextCount":1,
                "statusContextCountsByState":[{"count":1,"state":"SUCCESS"}],
                "nodes":[
                    {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE",
                        "checkSuite":{"creator":null,"workflowRun":null}},
                    {"__typename":"StatusContext","context":"ci/deploy","state":"SUCCESS","creator":{"login":"bot"}}
                ]}},
            "checkSuites":{"totalCount":0,"nodes":[]}}}]},
        "allCommits":{"nodes":[]},
        "comments":{"totalCount":4,"nodes":[]},
        "reviewThreads":{"totalCount":2,"nodes":[]},
        "reviewRequests":{"totalCount":1,"nodes":[{"asCodeOwner":true,
            "requestedReviewer":{"__typename":"Team","slug":"core","name":"Core"}}]},
        "reviews":{"totalCount":3,"nodes":[]},
        "suggestedReviewers":[],
        "files":{"totalCount":0,"nodes":[]},
        "mergeStateStatus":"BLOCKED","isInMergeQueue":false,
        "timelineItems":{"nodes":[
            {"__typename":"MergedEvent","actor":{"login":"m"},"createdAt":"2026-11-07T10:00:00Z"},
            {"__typename":"ReviewRequestedEvent","actor":null,"createdAt":"2026-11-07T09:00:00Z",
                "requestedReviewer":null},
            {"__typename":"SomethingNewEvent"}
        ]}
    }"#;

    fn details() -> PullRequestDetails {
        serde_json::from_str(DETAILS).unwrap()
    }

    #[test]
    fn a_deleted_author_shows_as_ghost() {
        assert_eq!(Login::of(&details().author), "ghost");
    }

    #[test]
    fn values_github_adds_later_decode_as_unknown() {
        assert_eq!(details().author_association, AuthorAssociation::Unknown);
    }

    #[test]
    fn null_connections_and_nodes_are_empty() {
        let d = details();
        assert!(d.labels.nodes.is_empty());
        assert!(d.assignees.nodes.is_empty());
    }

    #[test]
    fn check_runs_and_status_contexts_decode_by_type() {
        let d = details();
        let nodes = &d
            .head()
            .unwrap()
            .status_check_rollup
            .as_ref()
            .unwrap()
            .contexts
            .nodes;
        assert!(
            matches!(&nodes[0], CheckContext::CheckRun(r) if r.conclusion == Some(CheckState::Failure))
        );
        assert!(matches!(&nodes[1], CheckContext::StatusContext(s) if s.context == "ci/deploy"));
    }

    #[test]
    fn only_queued_workflow_runs_are_pending() {
        let suites: CountedNodes<CheckSuiteNode> = serde_json::from_str(
            r#"{"totalCount":2,"nodes":[
                {"status":"QUEUED","conclusion":null,"app":{"name":"Vercel"},"workflowRun":null},
                {"status":"QUEUED","conclusion":null,"app":{"name":"GitHub Actions"},
                    "workflowRun":{"workflow":{"name":"CI"}}}
            ]}"#,
        )
        .unwrap();
        let pending: Vec<bool> = suites
            .nodes
            .iter()
            .map(CheckSuiteNode::is_pending)
            .collect();
        assert_eq!(pending, [false, true]);
    }

    #[test]
    fn timeline_events_decode_by_type() {
        let nodes = details().timeline_items.nodes;
        assert!(matches!(&nodes[0], TimelineEvent::Merged(e) if Login::of(&e.actor) == "m"));
        assert!(
            matches!(&nodes[1], TimelineEvent::ReviewRequested(e) if e.requested_reviewer.is_none())
        );
        assert!(matches!(nodes[2], TimelineEvent::Unknown));
    }

    #[test]
    fn a_team_review_request_shows_its_slug() {
        let d = details();
        let request = &d.review_requests.nodes[0];
        assert!(request.is_team());
        assert_eq!(request.display_name(), Some("core"));
    }

    #[test]
    fn a_missing_required_field_is_an_error() {
        let without_title = DETAILS.replace(r#""title":"T","#, "");
        assert!(serde_json::from_str::<PullRequestDetails>(&without_title).is_err());
    }

    #[test]
    fn a_row_built_from_details_keeps_counts_and_checks() {
        let row = PullRequest::from(&details());
        assert_eq!(row.comments.total_count, 4);
        assert_eq!(row.review_threads.total_count, 2);
        assert_eq!(row.reviews.total_count, 3);
        assert_eq!(row.review_requests.total_count, 1);
        assert_eq!(row.rollup_state(), Some(CheckState::Failure));
    }
}
