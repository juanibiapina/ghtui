//! GitHub API access. Mirrors internal/data/*api.go: GraphQL search and
//! resource queries for PRs and issues, REST for notifications, and the gh CLI
//! for label lists. Authentication follows go-gh: $GH_TOKEN / $GITHUB_TOKEN,
//! otherwise `gh auth token`.

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::*;

pub struct Client {
    host: String,
    token: String,
    agent: ureq::Agent,
}

static CLIENT: OnceLock<std::result::Result<Client, String>> = OnceLock::new();
static ENRICHED_CACHE: OnceLock<Mutex<HashMap<String, EnrichedPullRequest>>> = OnceLock::new();
static ISSUE_CACHE: OnceLock<Mutex<HashMap<String, Issue>>> = OnceLock::new();
static USER_CACHE: OnceLock<Mutex<HashMap<String, Vec<(String, String)>>>> = OnceLock::new();
static LABEL_CACHE: OnceLock<Mutex<HashMap<String, Vec<Label>>>> = OnceLock::new();

/// The ctrl+f "refresh" key in autocomplete drops the cached lists for a repo.
pub fn clear_suggestion_cache(repo: &str) {
    if let Some(c) = USER_CACHE.get() {
        c.lock().unwrap().remove(repo);
    }
    if let Some(c) = LABEL_CACHE.get() {
        c.lock().unwrap().remove(repo);
    }
}

fn host() -> String {
    std::env::var("GH_HOST")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "github.com".into())
}

fn resolve_token(host: &str) -> Result<String> {
    let env_names: &[&str] = if host == "github.com" {
        &["GH_TOKEN", "GITHUB_TOKEN"]
    } else {
        &["GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "GH_TOKEN"]
    };
    for name in env_names {
        if let Ok(t) = std::env::var(name)
            && !t.is_empty()
        {
            return Ok(t);
        }
    }
    let out = Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .output()
        .context("running `gh auth token`")?;
    if !out.status.success() {
        bail!("authentication token not found for host {host}; run `gh auth login`");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn client() -> Result<&'static Client> {
    CLIENT
        .get_or_init(|| {
            let host = host();
            let token = resolve_token(&host).map_err(|e| e.to_string())?;
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .user_agent("gh-dash-rs")
                .build()
                .into();
            Ok(Client { host, token, agent })
        })
        .as_ref()
        .map_err(|e| anyhow!(e.clone()))
}

pub fn clear_enrichment_cache() {
    if let Some(c) = ENRICHED_CACHE.get() {
        c.lock().unwrap().clear();
    }
    if let Some(c) = ISSUE_CACHE.get() {
        c.lock().unwrap().clear();
    }
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

impl Client {
    fn graphql_url(&self) -> String {
        if self.host == "github.com" {
            "https://api.github.com/graphql".into()
        } else {
            format!("https://{}/api/graphql", self.host)
        }
    }

    fn rest_url(&self, path: &str) -> String {
        if path.starts_with("https://") {
            return path.to_string();
        }
        if self.host == "github.com" {
            format!("https://api.github.com/{path}")
        } else {
            format!("https://{}/api/v3/{path}", self.host)
        }
    }

    pub fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let body = json!({ "query": query, "variables": variables });
        let mut resp = self
            .agent
            .post(&self.graphql_url())
            .header("Authorization", &format!("bearer {}", self.token))
            .send_json(&body)?;
        let status = resp.status();
        let text = resp.body_mut().read_to_string()?;
        if !status.is_success() {
            bail!("HTTP {}: {}", status.as_u16(), text.trim());
        }
        let parsed: GraphQlResponse<T> =
            serde_json::from_str(&text).with_context(|| "decoding GraphQL response")?;
        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.into_iter().map(|e| e.message).collect();
            bail!("GraphQL: {}", msgs.join(", "));
        }
        parsed
            .data
            .ok_or_else(|| anyhow!("GraphQL response had no data"))
    }

    fn rest(&self, method: &str, path: &str) -> Result<(u16, String)> {
        let url = self.rest_url(path);
        let auth = format!("token {}", self.token);
        let mut resp = match method {
            "GET" => self
                .agent
                .get(&url)
                .header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .call()?,
            "DELETE" => self
                .agent
                .delete(&url)
                .header("Authorization", &auth)
                .call()?,
            "PATCH" => self
                .agent
                .patch(&url)
                .header("Authorization", &auth)
                .send_empty()?,
            "PUT" => self
                .agent
                .put(&url)
                .header("Authorization", &auth)
                .send_empty()?,
            _ => bail!("unsupported method {method}"),
        };
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        if status >= 400 {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or(text);
            bail!("HTTP {status}: {msg} ({url})");
        }
        Ok((status, text))
    }

    pub fn rest_get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (_, text) = self.rest("GET", path)?;
        serde_json::from_str(&text).with_context(|| format!("decoding {path}"))
    }
}

// ---------------- GraphQL documents ----------------

const REPOSITORY_FIELDS: &str = "name owner { login } nameWithOwner isArchived \
branchProtectionRules(first: 1) { nodes { requiredApprovingReviewCount requiresApprovingReviews \
requiresCodeOwnerReviews requiresStatusChecks requiredStatusCheckContexts } }";

fn pr_fields() -> String {
    format!(
        "number title author {{ login }} authorAssociation updatedAt createdAt url state mergeable \
reviewDecision additions deletions headRefName baseRefName headRepository {{ name }} headRef {{ name }} \
repository {{ {REPOSITORY_FIELDS} }} assignees(first: 3) {{ nodes {{ login }} }} comments {{ totalCount }} \
reviewThreads {{ totalCount }} reviews {{ totalCount }} reviewRequests {{ totalCount }} isDraft isInMergeQueue \
commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} \
labels(first: 6) {{ nodes {{ color name description }} }} mergeStateStatus"
    )
}

const COUNTS_BY_STATE: &str = "totalCount checkRunCount checkRunCountsByState { count state } \
statusContextCount statusContextCountsByState { count state }";

fn enriched_pr_fields() -> String {
    format!(
        "url number title body state isDraft author {{ login }} authorAssociation updatedAt createdAt \
mergeable reviewDecision additions deletions headRefName baseRefName headRepository {{ name }} \
headRef {{ name }} labels(first: 6) {{ nodes {{ color name description }} }} \
assignees(first: 3) {{ nodes {{ login }} }} repository {{ {REPOSITORY_FIELDS} }} \
commits(last: 1) {{ totalCount nodes {{ commit {{ commitUrl statusCheckRollup {{ state contexts(last: 100) {{ \
{COUNTS_BY_STATE} nodes {{ __typename ... on CheckRun {{ name status conclusion checkSuite {{ creator {{ login }} \
workflowRun {{ workflow {{ name }} }} }} }} ... on StatusContext {{ context state creator {{ login }} }} }} }} }} \
checkSuites(last: 20) {{ totalCount nodes {{ status conclusion app {{ name }} workflowRun {{ workflow {{ name }} }} }} }} }} }} }} \
allCommits: commits(last: 100) {{ nodes {{ commit {{ abbreviatedOid committedDate messageHeadline \
author {{ name user {{ login }} }} statusCheckRollup {{ state contexts(last: 1) {{ {COUNTS_BY_STATE} }} }} }} }} }} \
comments(last: 50, orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ totalCount nodes {{ author {{ login }} body updatedAt }} }} \
reviewThreads(last: 50) {{ nodes {{ id isOutdated line path comments(first: 20) {{ totalCount nodes {{ author {{ login }} body updatedAt }} }} }} }} \
reviewRequests(last: 100) {{ totalCount nodes {{ asCodeOwner requestedReviewer {{ ... on User {{ login }} \
... on Team {{ slug name }} ... on Bot {{ login }} ... on Mannequin {{ login }} }} }} }} \
reviews(last: 100) {{ totalCount nodes {{ author {{ login }} body state updatedAt }} }} \
suggestedReviewers {{ isAuthor isCommenter reviewer {{ login }} }} \
files(first: 20) {{ totalCount nodes {{ additions deletions path changeType }} }} \
mergeStateStatus isInMergeQueue"
    )
}

fn issue_fields() -> String {
    format!(
        "number title body state author {{ login }} authorAssociation updatedAt createdAt url \
repository {{ {REPOSITORY_FIELDS} }} assignees(first: 3) {{ nodes {{ login }} }} \
comments(last: 15) {{ totalCount nodes {{ author {{ login }} body updatedAt }} }} \
reactions(first: 1) {{ totalCount }} labels(first: 20) {{ nodes {{ color name description }} }}"
    )
}

#[derive(Deserialize)]
struct SearchData<T> {
    search: SearchResult<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult<T> {
    nodes: Vec<Option<T>>,
    issue_count: usize,
    page_info: PageInfo,
}

pub struct SearchPage<T> {
    pub items: Vec<T>,
    pub total_count: usize,
    pub page_info: PageInfo,
}

fn search<T: DeserializeOwned>(
    name: &str,
    kind: &str,
    fields: &str,
    query: &str,
    limit: usize,
    page: Option<&PageInfo>,
) -> Result<SearchPage<T>> {
    let doc = format!(
        "query {name}($query: String!, $limit: Int!, $endCursor: String) {{ \
search(type: ISSUE, first: $limit, after: $endCursor, query: $query) {{ \
nodes {{ ... on {kind} {{ {fields} }} }} issueCount pageInfo {{ hasNextPage startCursor endCursor }} }} }}"
    );
    let vars = json!({
        "query": query,
        "limit": limit,
        "endCursor": page.map(|p| p.end_cursor.clone()),
    });
    // Nodes that aren't of `kind` decode as empty objects; tolerate them.
    let data: SearchData<Value> = client()?.graphql(&doc, vars)?;
    let mut items = vec![];
    for node in data.search.nodes.into_iter().flatten() {
        if node.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            continue;
        }
        items.push(serde_json::from_value(node)?);
    }
    Ok(SearchPage {
        items,
        total_count: data.search.issue_count,
        page_info: data.search.page_info,
    })
}

pub fn fetch_pull_requests(
    query: &str,
    limit: usize,
    page: Option<&PageInfo>,
) -> Result<SearchPage<PullRequest>> {
    let q = format!("is:pr archived:false {query} sort:updated");
    search(
        "SearchPullRequests",
        "PullRequest",
        &pr_fields(),
        &q,
        limit,
        page,
    )
}

pub fn fetch_issues(
    query: &str,
    limit: usize,
    page: Option<&PageInfo>,
) -> Result<SearchPage<Issue>> {
    let q = format!("is:issue archived:false {query} sort:updated");
    search("SearchIssues", "Issue", &issue_fields(), &q, limit, page)
}

#[derive(Deserialize)]
struct ResourceData<T> {
    resource: Option<T>,
}

pub fn fetch_pull_request(url: &str) -> Result<EnrichedPullRequest> {
    let cache = ENRICHED_CACHE.get_or_init(Default::default);
    if let Some(pr) = cache.lock().unwrap().get(url) {
        return Ok(pr.clone());
    }
    let doc = format!(
        "query FetchPullRequest($url: URI!) {{ resource(url: $url) {{ ... on PullRequest {{ {} }} }} }}",
        enriched_pr_fields()
    );
    let data: ResourceData<EnrichedPullRequest> = client()?.graphql(&doc, json!({ "url": url }))?;
    let pr = data
        .resource
        .ok_or_else(|| anyhow!("pull request not found: {url}"))?;
    cache.lock().unwrap().insert(url.to_string(), pr.clone());
    Ok(pr)
}

pub fn fetch_issue(url: &str) -> Result<Issue> {
    let cache = ISSUE_CACHE.get_or_init(Default::default);
    if let Some(i) = cache.lock().unwrap().get(url) {
        return Ok(i.clone());
    }
    let doc = format!(
        "query FetchIssue($url: URI!) {{ resource(url: $url) {{ ... on Issue {{ {} }} }} }}",
        issue_fields()
    );
    let data: ResourceData<Issue> = client()?.graphql(&doc, json!({ "url": url }))?;
    let issue = data
        .resource
        .ok_or_else(|| anyhow!("issue not found: {url}"))?;
    cache.lock().unwrap().insert(url.to_string(), issue.clone());
    Ok(issue)
}

pub fn current_login() -> Result<String> {
    #[derive(Deserialize)]
    struct Viewer {
        viewer: Login,
    }
    let v: Viewer = client()?.graphql("query UserCurrent { viewer { login } }", json!({}))?;
    Ok(v.viewer.login)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct User {
    login: String,
    #[serde(deserialize_with = "null_str")]
    name: String,
}

fn null_str<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// data.FetchRepoUsers: mentionable users as (login, name), cached per repo.
pub fn fetch_repo_users(owner: &str, name: &str) -> Result<Vec<(String, String)>> {
    let key = format!("{owner}/{name}");
    let cache = USER_CACHE.get_or_init(Default::default);
    if let Some(u) = cache.lock().unwrap().get(&key) {
        return Ok(u.clone());
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repo {
        mentionable_users: Nodes<User>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repo>,
    }
    let d: Data = client()?.graphql(
        "query GetMentionableUsers($owner: String!, $name: String!, $limit: Int!) { \
repository(owner: $owner, name: $name) { mentionableUsers(first: $limit) { nodes { login } } } }",
        json!({ "owner": owner, "name": name, "limit": 100 }),
    )?;
    let users: Vec<(String, String)> = d
        .repository
        .map(|r| {
            r.mentionable_users
                .nodes
                .into_iter()
                .filter(|u| !u.login.is_empty())
                .map(|u| (u.login, u.name))
                .collect()
        })
        .unwrap_or_default();
    cache.lock().unwrap().insert(key, users.clone());
    Ok(users)
}

/// data.FetchRepoLabels: `gh label list`.
pub fn fetch_repo_labels(repo: &str) -> Result<Vec<Label>> {
    let cache = LABEL_CACHE.get_or_init(Default::default);
    if let Some(l) = cache.lock().unwrap().get(repo) {
        return Ok(l.clone());
    }
    let out = Command::new("gh")
        .args([
            "label",
            "list",
            "-R",
            repo,
            "--json",
            "name,description,color",
            "--limit",
            "300",
        ])
        .output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let labels: Vec<Label> = serde_json::from_slice(&out.stdout)?;
    let labels: Vec<Label> = labels
        .into_iter()
        .filter(|l| !l.name.trim().is_empty())
        .collect();
    cache
        .lock()
        .unwrap()
        .insert(repo.to_string(), labels.clone());
    Ok(labels)
}

// ---------------- REST: notifications ----------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadState {
    Unread,
    Read,
    All,
}

pub struct NotificationsPage {
    pub notifications: Vec<Notification>,
    pub page_info: PageInfo,
}

/// data.FetchNotifications. Pages are numbered; EndCursor carries the next page number.
pub fn fetch_notifications(
    limit: usize,
    repo_filters: &[String],
    read_state: ReadState,
    page: Option<&PageInfo>,
) -> Result<NotificationsPage> {
    let c = client()?;
    let mut filter = "&is_done=false".to_string();
    if matches!(read_state, ReadState::Read | ReadState::All) {
        filter.push_str("&all=true");
    }
    let page_no: usize = page.and_then(|p| p.end_cursor.parse().ok()).unwrap_or(1);
    let mut all: Vec<Notification> = vec![];
    if repo_filters.is_empty() {
        all = c.rest_get(&format!(
            "notifications?per_page={limit}&page={page_no}{filter}"
        ))?;
    } else {
        for repo in repo_filters {
            if let Ok(mut v) = c.rest_get::<Vec<Notification>>(&format!(
                "repos/{repo}/notifications?per_page={limit}&page={page_no}{filter}"
            )) {
                all.append(&mut v);
            }
        }
    }
    let has_next = all.len() >= limit;
    let notifications = match read_state {
        ReadState::Read => all.into_iter().filter(|n| !n.unread).collect(),
        ReadState::Unread => all.into_iter().filter(|n| n.unread).collect(),
        ReadState::All => all,
    };
    Ok(NotificationsPage {
        notifications,
        page_info: PageInfo {
            has_next_page: has_next,
            start_cursor: String::new(),
            end_cursor: (page_no + 1).to_string(),
        },
    })
}

pub fn fetch_notification_by_thread(id: &str) -> Result<Notification> {
    client()?.rest_get(&format!("notifications/threads/{id}"))
}

pub fn mark_notification_done(id: &str) -> Result<()> {
    client()?
        .rest("DELETE", &format!("notifications/threads/{id}"))
        .map(|_| ())
}

pub fn mark_notification_read(id: &str) -> Result<()> {
    client()?
        .rest("PATCH", &format!("notifications/threads/{id}"))
        .map(|_| ())
}

pub fn unsubscribe_thread(id: &str) -> Result<()> {
    client()?
        .rest(
            "DELETE",
            &format!("notifications/threads/{id}/subscription"),
        )
        .map(|_| ())
}

pub fn mark_all_notifications_read() -> Result<()> {
    client()?.rest("PUT", "notifications").map(|_| ())
}

/// data.FetchCommentAuthor
pub fn fetch_comment_author(api_url: &str) -> Result<String> {
    if api_url.is_empty() {
        return Ok(String::new());
    }
    #[derive(Deserialize)]
    struct C {
        user: Login,
    }
    let path = api_url
        .strip_prefix("https://api.github.com/")
        .unwrap_or(api_url);
    let c: C = client()?.rest_get(path)?;
    Ok(c.user.login)
}

/// data.FindBestWorkflowRunMatch
pub fn best_workflow_run(runs: &[WorkflowRun], at: DateTime<Utc>) -> Option<&WorkflowRun> {
    let mut best: Option<&WorkflowRun> = None;
    let mut best_diff = chrono::Duration::days(365);
    for r in runs {
        let diff = (at - r.updated_at).abs();
        if diff < best_diff && diff < chrono::Duration::hours(1) {
            best_diff = diff;
            best = Some(r);
        }
    }
    best.or_else(|| runs.first())
}

/// data.FetchRecentWorkflowRun
pub fn fetch_recent_workflow_run(repo: &str, at: DateTime<Utc>) -> Result<String> {
    let resp: WorkflowRunsResponse =
        client()?.rest_get(&format!("repos/{repo}/actions/runs?per_page=20"))?;
    Ok(best_workflow_run(&resp.workflow_runs, at)
        .map(|r| r.html_url.clone())
        .unwrap_or_default())
}
