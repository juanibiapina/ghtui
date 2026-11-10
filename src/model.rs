//! GitHub data shapes. Field names follow the GraphQL/REST responses.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Login {
    #[serde(deserialize_with = "null_as_default")]
    pub login: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct NameOnly {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(
    default,
    rename_all = "camelCase",
    bound(deserialize = "T: Deserialize<'de>")
)]
pub struct Nodes<T> {
    #[serde(deserialize_with = "null_as_default")]
    pub nodes: Vec<T>,
    #[serde(deserialize_with = "null_as_default")]
    pub total_count: i64,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Nodes {
            nodes: vec![],
            total_count: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Count {
    #[serde(deserialize_with = "null_as_default")]
    pub total_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct BranchProtectionRule {
    pub required_approving_review_count: i64,
    pub requires_approving_reviews: bool,
    pub requires_code_owner_reviews: bool,
    pub requires_status_checks: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub required_status_check_contexts: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Repository {
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub owner: Login,
    pub name_with_owner: String,
    pub is_archived: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub branch_protection_rules: Nodes<BranchProtectionRule>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Label {
    #[serde(deserialize_with = "null_as_default")]
    pub color: String,
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct RollupState {
    #[serde(deserialize_with = "null_as_default")]
    pub state: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct CommitWithRollup {
    #[serde(deserialize_with = "null_as_default")]
    pub status_check_rollup: RollupState,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, bound(deserialize = "T: Deserialize<'de> + Default"))]
pub struct CommitNode<T> {
    #[serde(deserialize_with = "null_as_default")]
    pub commit: T,
}

/// A row in a PR section.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PullRequest {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    pub number: i64,
    pub title: String,
    #[serde(deserialize_with = "null_as_default")]
    pub author: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub author_association: String,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub state: String,
    #[serde(deserialize_with = "null_as_default")]
    pub mergeable: String,
    #[serde(deserialize_with = "null_as_default")]
    pub review_decision: String,
    pub additions: i64,
    pub deletions: i64,
    pub head_ref_name: String,
    pub base_ref_name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub head_repository: NameOnly,
    #[serde(deserialize_with = "null_as_default")]
    pub head_ref: NameOnly,
    #[serde(deserialize_with = "null_as_default")]
    pub repository: Repository,
    #[serde(deserialize_with = "null_as_default")]
    pub assignees: Nodes<Login>,
    #[serde(deserialize_with = "null_as_default")]
    pub comments: Count,
    #[serde(deserialize_with = "null_as_default")]
    pub review_threads: Count,
    #[serde(deserialize_with = "null_as_default")]
    pub reviews: Count,
    #[serde(deserialize_with = "null_as_default")]
    pub review_requests: Count,
    pub is_draft: bool,
    pub is_in_merge_queue: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub commits: Nodes<CommitNode<CommitWithRollup>>,
    #[serde(deserialize_with = "null_as_default")]
    pub labels: Nodes<Label>,
    #[serde(deserialize_with = "null_as_default")]
    pub merge_state_status: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct CountByState {
    pub count: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub state: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct WorkflowName {
    #[serde(deserialize_with = "null_as_default")]
    pub workflow: NameOnly,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckSuiteRef {
    #[serde(deserialize_with = "null_as_default")]
    pub creator: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub workflow_run: WorkflowName,
}

/// A node of statusCheckRollup.contexts: either a CheckRun or a StatusContext.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckContext {
    #[serde(rename = "__typename")]
    pub typename: String,
    // CheckRun
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(deserialize_with = "null_as_default")]
    pub conclusion: String,
    #[serde(deserialize_with = "null_as_default")]
    pub check_suite: CheckSuiteRef,
    // StatusContext
    #[serde(deserialize_with = "null_as_default")]
    pub context: String,
    #[serde(deserialize_with = "null_as_default")]
    pub state: String,
    #[serde(deserialize_with = "null_as_default")]
    pub creator: Login,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct RollupContexts {
    pub total_count: i64,
    pub check_run_count: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub check_run_counts_by_state: Vec<CountByState>,
    pub status_context_count: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub status_context_counts_by_state: Vec<CountByState>,
    #[serde(deserialize_with = "null_as_default")]
    pub nodes: Vec<CheckContext>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct StatusCheckRollup {
    #[serde(deserialize_with = "null_as_default")]
    pub state: String,
    #[serde(deserialize_with = "null_as_default")]
    pub contexts: RollupContexts,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckSuiteNode {
    #[serde(deserialize_with = "null_as_default")]
    pub status: String,
    #[serde(deserialize_with = "null_as_default")]
    pub conclusion: String,
    #[serde(deserialize_with = "null_as_default")]
    pub app: NameOnly,
    #[serde(deserialize_with = "null_as_default")]
    pub workflow_run: WorkflowName,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct LastCommit {
    #[serde(deserialize_with = "null_as_default")]
    pub oid: String,
    #[serde(deserialize_with = "null_as_default")]
    pub commit_url: String,
    #[serde(deserialize_with = "null_as_default")]
    pub status_check_rollup: StatusCheckRollup,
    #[serde(deserialize_with = "null_as_default")]
    pub check_suites: Nodes<CheckSuiteNode>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct CommitAuthor {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub user: Login,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct HistoryCommit {
    pub abbreviated_oid: String,
    pub committed_date: DateTime<Utc>,
    pub message_headline: String,
    #[serde(deserialize_with = "null_as_default")]
    pub author: CommitAuthor,
    #[serde(deserialize_with = "null_as_default")]
    pub status_check_rollup: StatusCheckRollup,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Comment {
    #[serde(deserialize_with = "null_as_default")]
    pub author: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub body: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ReviewThread {
    pub id: String,
    pub is_outdated: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub line: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub path: String,
    #[serde(deserialize_with = "null_as_default")]
    pub comments: Nodes<Comment>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Review {
    #[serde(deserialize_with = "null_as_default")]
    pub author: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub body: String,
    pub state: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct RequestedReviewer {
    #[serde(deserialize_with = "null_as_default")]
    pub login: String,
    #[serde(deserialize_with = "null_as_default")]
    pub slug: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ReviewRequest {
    pub as_code_owner: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub requested_reviewer: RequestedReviewer,
}

impl ReviewRequest {
    pub fn display_name(&self) -> &str {
        let r = &self.requested_reviewer;
        if !r.login.is_empty() {
            &r.login
        } else {
            &r.slug
        }
    }
    pub fn is_team(&self) -> bool {
        self.requested_reviewer.login.is_empty() && !self.requested_reviewer.slug.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SuggestedReviewer {
    pub is_author: bool,
    pub is_commenter: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub reviewer: Login,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ChangedFile {
    pub additions: i64,
    pub deletions: i64,
    pub path: String,
    pub change_type: String,
}

/// The full PR fetched for the preview pane.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PullRequestDetails {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    pub url: String,
    pub number: i64,
    pub title: String,
    #[serde(deserialize_with = "null_as_default")]
    pub body: String,
    pub state: String,
    pub is_draft: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub author: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub author_association: String,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    #[serde(deserialize_with = "null_as_default")]
    pub mergeable: String,
    #[serde(deserialize_with = "null_as_default")]
    pub review_decision: String,
    pub additions: i64,
    pub deletions: i64,
    pub head_ref_name: String,
    pub base_ref_name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub head_repository: NameOnly,
    #[serde(deserialize_with = "null_as_default")]
    pub head_ref: NameOnly,
    #[serde(deserialize_with = "null_as_default")]
    pub labels: Nodes<Label>,
    #[serde(deserialize_with = "null_as_default")]
    pub assignees: Nodes<Login>,
    #[serde(deserialize_with = "null_as_default")]
    pub repository: Repository,
    #[serde(deserialize_with = "null_as_default")]
    pub commits: Nodes<CommitNode<LastCommit>>,
    #[serde(deserialize_with = "null_as_default")]
    pub all_commits: Nodes<CommitNode<HistoryCommit>>,
    #[serde(deserialize_with = "null_as_default")]
    pub comments: Nodes<Comment>,
    #[serde(deserialize_with = "null_as_default")]
    pub review_threads: Nodes<ReviewThread>,
    #[serde(deserialize_with = "null_as_default")]
    pub review_requests: Nodes<ReviewRequest>,
    #[serde(deserialize_with = "null_as_default")]
    pub reviews: Nodes<Review>,
    #[serde(deserialize_with = "null_as_default")]
    pub suggested_reviewers: Vec<SuggestedReviewer>,
    #[serde(deserialize_with = "null_as_default")]
    pub files: Nodes<ChangedFile>,
    #[serde(deserialize_with = "null_as_default")]
    pub merge_state_status: String,
    pub is_in_merge_queue: bool,
}

impl PullRequestDetails {
    pub fn to_pull_request(&self) -> PullRequest {
        PullRequest {
            id: self.id.clone(),
            number: self.number,
            title: self.title.clone(),
            author: self.author.clone(),
            author_association: self.author_association.clone(),
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url.clone(),
            state: self.state.clone(),
            mergeable: self.mergeable.clone(),
            review_decision: self.review_decision.clone(),
            additions: self.additions,
            deletions: self.deletions,
            head_ref_name: self.head_ref_name.clone(),
            base_ref_name: self.base_ref_name.clone(),
            head_repository: self.head_repository.clone(),
            head_ref: self.head_ref.clone(),
            repository: self.repository.clone(),
            assignees: self.assignees.clone(),
            is_draft: self.is_draft,
            labels: self.labels.clone(),
            merge_state_status: self.merge_state_status.clone(),
            is_in_merge_queue: self.is_in_merge_queue,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Issue {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    pub number: i64,
    pub title: String,
    #[serde(deserialize_with = "null_as_default")]
    pub body: String,
    pub state: String,
    #[serde(deserialize_with = "null_as_default")]
    pub author: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub author_association: String,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    #[serde(deserialize_with = "null_as_default")]
    pub repository: Repository,
    #[serde(deserialize_with = "null_as_default")]
    pub assignees: Nodes<Login>,
    #[serde(deserialize_with = "null_as_default")]
    pub comments: Nodes<Comment>,
    #[serde(deserialize_with = "null_as_default")]
    pub reactions: Count,
    #[serde(deserialize_with = "null_as_default")]
    pub labels: Nodes<Label>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub start_cursor: String,
    #[serde(deserialize_with = "null_as_default")]
    pub end_cursor: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct NotificationSubject {
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    #[serde(deserialize_with = "null_as_default")]
    pub url: String,
    #[serde(deserialize_with = "null_as_default")]
    pub latest_comment_url: String,
    #[serde(rename = "type", deserialize_with = "null_as_default")]
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct NotificationRepository {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub owner: Login,
    #[serde(deserialize_with = "null_as_default")]
    pub html_url: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Notification {
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub reason: String,
    pub updated_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub subject: NotificationSubject,
    pub repository: NotificationRepository,
    #[serde(deserialize_with = "null_as_default")]
    pub url: String,
    #[serde(rename = "subscription_url", deserialize_with = "null_as_default")]
    pub subscription: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct WorkflowRun {
    pub id: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub html_url: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct WorkflowRunsResponse {
    pub workflow_runs: Vec<WorkflowRun>,
}

pub fn is_status_waiting(s: &str) -> bool {
    matches!(s, "PENDING" | "QUEUED" | "IN_PROGRESS" | "WAITING")
}
pub fn is_conclusion_failure(s: &str) -> bool {
    matches!(s, "FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE")
}
pub fn is_conclusion_skip(s: &str) -> bool {
    s == "SKIPPED"
}
pub fn is_conclusion_neutral(s: &str) -> bool {
    s == "NEUTRAL"
}
pub fn is_conclusion_success(s: &str) -> bool {
    s == "SUCCESS"
}
