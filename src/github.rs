//! GitHub API access: GraphQL search and
//! resource queries for PRs and issues, REST for notifications, and the gh CLI
//! for label lists. Authentication follows gh: $GH_TOKEN / $GITHUB_TOKEN,
//! otherwise `gh auth token`.
//!
//! Requests run in a lane. Background requests stop while GitHub rate limits
//! us; requests the user starts always run.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::model::*;

/// Background work stops when fewer GraphQL points than this remain.
const REMAINING_FLOOR: i64 = 500;
/// Background GraphQL response time allowed per rolling minute.
pub const BACKGROUND_BUDGET: Duration = Duration::from_secs(20);
const BUDGET_WINDOW: Duration = Duration::from_secs(60);
const MAX_CONDITIONAL: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    Foreground,
    Background,
}

thread_local! {
    static LANE: Cell<Lane> = const { Cell::new(Lane::Foreground) };
}

/// Runs `f` with its GitHub requests in the background lane.
pub fn background<R>(f: impl FnOnce() -> R) -> R {
    let prev = LANE.with(|l| l.replace(Lane::Background));
    let r = f();
    LANE.with(|l| l.set(prev));
    r
}

fn lane() -> Lane {
    LANE.with(|l| l.get())
}

/// GitHub asked us to wait. Background requests fail fast with this until `until`.
#[derive(Debug, Clone, Copy)]
pub struct RateLimited {
    pub until: Instant,
}

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let wait = self.until.saturating_duration_since(Instant::now());
        let at = chrono::Local::now() + chrono::Duration::from_std(wait).unwrap_or_default();
        write!(
            f,
            "GitHub rate limit reached, retrying at {}",
            at.format("%H:%M")
        )
    }
}

impl std::error::Error for RateLimited {}

/// GitHub didn't answer within its 10 s limit.
#[derive(Debug, Clone, Copy)]
pub struct Timeout;

impl std::fmt::Display for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GitHub took too long to respond")
    }
}

impl std::error::Error for Timeout {}

pub fn is_rate_limited(e: &anyhow::Error) -> bool {
    e.downcast_ref::<RateLimited>().is_some()
}

pub fn is_timeout(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Timeout>().is_some()
}

/// The rate limit headers of a response.
#[derive(Default, Debug, Clone)]
pub struct RateHeaders {
    pub retry_after: Option<u64>,
    pub remaining: Option<i64>,
    pub reset: Option<i64>,
}

/// How long a rate limited response asks us to wait.
#[derive(Debug, PartialEq, Eq)]
pub enum Wait {
    Exactly(Duration),
    /// No usable header: back off.
    Backoff,
}

/// Recognizes primary and secondary rate limit responses.
pub fn detect_rate_limit(status: u16, h: &RateHeaders, body: &str, now_epoch: i64) -> Option<Wait> {
    let lower = body.to_ascii_lowercase();
    let secondary = lower.contains("secondary rate limit");
    let graphql_limited = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("errors").cloned())
        .and_then(|e| e.as_array().cloned())
        .map(|errs| {
            errs.iter()
                .any(|e| e.get("type").and_then(|t| t.as_str()) == Some("RATE_LIMITED"))
        })
        .unwrap_or(false);
    let limited_status = matches!(status, 403 | 429)
        && (status == 429 || h.retry_after.is_some() || h.remaining == Some(0) || secondary);
    let limited = limited_status || graphql_limited || (status == 200 && secondary);
    if !limited {
        return None;
    }
    if let Some(s) = h.retry_after {
        return Some(Wait::Exactly(Duration::from_secs(s)));
    }
    if h.remaining == Some(0)
        && let Some(reset) = h.reset
    {
        return Some(Wait::Exactly(Duration::from_secs(
            (reset - now_epoch).max(1) as u64,
        )));
    }
    Some(Wait::Backoff)
}

/// 60 s, doubling with each consecutive rate limit response, up to 15 minutes.
pub fn backoff(level: u32) -> Duration {
    let secs = 60u64.saturating_mul(1u64 << level.min(10));
    Duration::from_secs(secs.min(15 * 60))
}

/// A cached REST response for conditional requests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conditional {
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: String,
    pub at: DateTime<Utc>,
}

#[derive(Default)]
struct State {
    paused_until: Option<Instant>,
    backoff_level: u32,
    low_until: Option<Instant>,
    conditional: HashMap<String, Conditional>,
    background_time: VecDeque<(Instant, Duration)>,
    notifications_polled: Option<(DateTime<Utc>, u64)>,
}

pub struct Client {
    host: String,
    token: String,
    agent: ureq::Agent,
    state: Mutex<State>,
}

static CLIENT: OnceLock<std::result::Result<Client, String>> = OnceLock::new();
type Cache<T> = OnceLock<Mutex<HashMap<String, T>>>;

static USER_CACHE: Cache<Vec<(String, String)>> = OnceLock::new();
static LABEL_CACHE: Cache<Vec<Label>> = OnceLock::new();

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
                .user_agent("ghtui")
                .build()
                .into();
            Ok(Client {
                host,
                token,
                agent,
                state: Mutex::new(State::default()),
            })
        })
        .as_ref()
        .map_err(|e| anyhow!(e.clone()))
}

/// 64-bit FNV-1a, stable across Rust releases.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Names the signed-in account without revealing its token.
pub fn account_key() -> Result<String> {
    let c = client()?;
    Ok(account_key_for(&c.host, &c.token))
}

pub fn account_key_for(host: &str, token: &str) -> String {
    format!("{:016x}", fnv1a(format!("{host}\0{token}").as_bytes()))
}

/// How long background work must wait: a rate limit pause, the remaining-points
/// floor, or the background lane's response time budget.
pub fn background_wait() -> Option<Duration> {
    let c = client().ok()?;
    c.background_wait(Instant::now())
}

/// Notifications may be polled again: the last poll is older than `X-Poll-Interval`.
pub fn notifications_poll_due() -> bool {
    let Ok(c) = client() else { return true };
    let st = c.state.lock().unwrap();
    match st.notifications_polled {
        Some((at, secs)) => Utc::now() >= at + chrono::Duration::seconds(secs as i64),
        None => true,
    }
}

pub fn notifications_polled() -> Option<(DateTime<Utc>, u64)> {
    client().ok()?.state.lock().unwrap().notifications_polled
}

pub fn conditional_snapshot() -> Vec<Conditional> {
    let Ok(c) = client() else { return vec![] };
    let st = c.state.lock().unwrap();
    let mut v: Vec<Conditional> = st.conditional.values().cloned().collect();
    v.sort_by_key(|c| std::cmp::Reverse(c.at));
    v.truncate(MAX_CONDITIONAL);
    v
}

/// Loads conditional request state saved by an earlier session.
pub fn seed_conditional(entries: Vec<Conditional>, polled: Option<(DateTime<Utc>, u64)>) {
    let Ok(c) = client() else { return };
    let mut st = c.state.lock().unwrap();
    for e in entries {
        st.conditional.entry(e.url.clone()).or_insert(e);
    }
    if st.notifications_polled.is_none() {
        st.notifications_polled = polled;
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
    #[serde(default)]
    path: Vec<Value>,
}

/// An aliased query's data with the errors of each alias.
pub struct Partial {
    pub data: Map<String, Value>,
    pub errors: HashMap<String, String>,
}

/// Decodes a GraphQL response, keeping the data of aliases that succeeded.
pub fn decode_partial(text: &str) -> Result<Partial> {
    let parsed: GraphQlResponse<Map<String, Value>> =
        serde_json::from_str(text).map_err(|_| anyhow!(Timeout))?;
    let mut errors: HashMap<String, String> = HashMap::new();
    let mut global = vec![];
    for e in parsed.errors {
        match e.path.first().and_then(|p| p.as_str()) {
            Some(alias) => {
                errors.insert(alias.to_string(), e.message);
            }
            None => global.push(e.message),
        }
    }
    match parsed.data {
        Some(data) => Ok(Partial { data, errors }),
        None => bail!("GraphQL: {}", global.join(", ")),
    }
}

fn header(resp: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

fn rate_headers(resp: &ureq::http::Response<ureq::Body>) -> RateHeaders {
    RateHeaders {
        retry_after: header(resp, "retry-after").and_then(|v| v.trim().parse().ok()),
        remaining: header(resp, "x-ratelimit-remaining").and_then(|v| v.trim().parse().ok()),
        reset: header(resp, "x-ratelimit-reset").and_then(|v| v.trim().parse().ok()),
    }
}

fn operation_name(query: &str) -> &str {
    query
        .trim_start()
        .strip_prefix("query ")
        .and_then(|r| r.split(['(', ' ', '{']).next())
        .unwrap_or("query")
}

fn instant_at_epoch(epoch: i64) -> Instant {
    let now = Utc::now().timestamp();
    Instant::now() + Duration::from_secs((epoch - now).max(0) as u64)
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

    fn background_wait(&self, now: Instant) -> Option<Duration> {
        let mut st = self.state.lock().unwrap();
        let pause = [st.paused_until, st.low_until]
            .into_iter()
            .flatten()
            .filter(|t| *t > now)
            .max();
        if let Some(t) = pause {
            return Some(t - now);
        }
        while st
            .background_time
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > BUDGET_WINDOW)
        {
            st.background_time.pop_front();
        }
        let used: Duration = st.background_time.iter().map(|(_, d)| *d).sum();
        if used >= BACKGROUND_BUDGET {
            let oldest = st.background_time.front().map(|(t, _)| *t).unwrap_or(now);
            return Some(
                (oldest + BUDGET_WINDOW)
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(100)),
            );
        }
        None
    }

    /// Background requests fail fast while GitHub asked us to wait.
    fn check_paused(&self) -> Result<()> {
        if lane() == Lane::Foreground {
            return Ok(());
        }
        let now = Instant::now();
        let st = self.state.lock().unwrap();
        let until = [st.paused_until, st.low_until]
            .into_iter()
            .flatten()
            .filter(|t| *t > now)
            .max();
        match until {
            Some(until) => Err(anyhow!(RateLimited { until })),
            None => Ok(()),
        }
    }

    /// Records the outcome of a response and turns rate limits into `RateLimited`.
    fn observe(
        &self,
        status: u16,
        h: &RateHeaders,
        body: &str,
        elapsed: Duration,
        graphql: bool,
    ) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        if graphql && lane() == Lane::Background {
            st.background_time.push_back((Instant::now(), elapsed));
        }
        if let Some(wait) = detect_rate_limit(status, h, body, Utc::now().timestamp()) {
            let wait = match wait {
                Wait::Exactly(d) => d,
                Wait::Backoff => {
                    let d = backoff(st.backoff_level);
                    st.backoff_level += 1;
                    d
                }
            };
            let until = Instant::now() + wait;
            if graphql {
                st.paused_until = Some(st.paused_until.map_or(until, |p| p.max(until)));
            }
            crate::warn!("rate limited status={status} wait={}s", wait.as_secs());
            return Err(anyhow!(RateLimited { until }));
        }
        if graphql && (200..300).contains(&status) {
            st.backoff_level = 0;
            st.low_until = match (h.remaining, h.reset) {
                (Some(r), Some(reset)) if r < REMAINING_FLOOR => Some(instant_at_epoch(reset)),
                _ => None,
            };
        }
        Ok(())
    }

    /// Sends a GraphQL request and returns the response body.
    fn post_graphql(&self, query: &str, variables: Value) -> Result<String> {
        self.check_paused()?;
        let body = json!({ "query": query, "variables": variables });
        let started = Instant::now();
        let mut resp = self
            .agent
            .post(&self.graphql_url())
            .header("Authorization", &format!("bearer {}", self.token))
            .send_json(&body)?;
        let status = resp.status().as_u16();
        let h = rate_headers(&resp);
        let text = resp.body_mut().read_to_string()?;
        let elapsed = started.elapsed();
        crate::info!(
            "api graphql op={} lane={:?} status={status} ms={} remaining={}",
            operation_name(query),
            lane(),
            elapsed.as_millis(),
            h.remaining.map(|r| r.to_string()).unwrap_or_default()
        );
        self.observe(status, &h, &text, elapsed, true)?;
        if text.contains("couldn't respond to your request in time") {
            bail!(Timeout);
        }
        if !(200..300).contains(&status) {
            bail!("HTTP {status}: {}", text.trim());
        }
        Ok(text)
    }

    pub fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        let text = self.post_graphql(query, variables)?;
        let parsed: GraphQlResponse<T> =
            serde_json::from_str(&text).map_err(|_| anyhow!(Timeout))?;
        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.into_iter().map(|e| e.message).collect();
            bail!("GraphQL: {}", msgs.join(", "));
        }
        parsed
            .data
            .ok_or_else(|| anyhow!("GraphQL response had no data"))
    }

    /// For aliased queries: one failing alias doesn't discard the others.
    pub fn graphql_partial(&self, query: &str, variables: Value) -> Result<Partial> {
        let text = self.post_graphql(query, variables)?;
        decode_partial(&text)
    }

    fn rest(&self, method: &str, path: &str) -> Result<(u16, String)> {
        let url = self.rest_url(path);
        let auth = format!("token {}", self.token);
        let cached = (method == "GET")
            .then(|| self.state.lock().unwrap().conditional.get(&url).cloned())
            .flatten();
        let started = Instant::now();
        let mut resp = match method {
            "GET" => {
                let mut req = self
                    .agent
                    .get(&url)
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json");
                if let Some(c) = &cached {
                    if let Some(e) = &c.etag {
                        req = req.header("If-None-Match", e);
                    }
                    if let Some(m) = &c.last_modified {
                        req = req.header("If-Modified-Since", m);
                    }
                }
                req.call()?
            }
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
        let h = rate_headers(&resp);
        let etag = header(&resp, "etag");
        let last_modified = header(&resp, "last-modified");
        let poll = header(&resp, "x-poll-interval").and_then(|v| v.trim().parse::<u64>().ok());
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        crate::info!(
            "api rest {method} {path} status={status} ms={}",
            started.elapsed().as_millis()
        );
        if let Some(secs) = poll
            && is_notification_list(path)
        {
            self.state.lock().unwrap().notifications_polled = Some((Utc::now(), secs));
        }
        if status == 304
            && let Some(c) = cached
        {
            return Ok((200, c.body));
        }
        self.observe(status, &h, &text, started.elapsed(), false)?;
        if status >= 400 {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or(text);
            bail!("HTTP {status}: {msg} ({url})");
        }
        if method == "GET" && (etag.is_some() || last_modified.is_some()) {
            self.state.lock().unwrap().conditional.insert(
                url.clone(),
                Conditional {
                    url,
                    etag,
                    last_modified,
                    body: text.clone(),
                    at: Utc::now(),
                },
            );
        }
        Ok((status, text))
    }

    pub fn rest_get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (_, text) = self.rest("GET", path)?;
        serde_json::from_str(&text).with_context(|| format!("decoding {path}"))
    }
}

fn is_notification_list(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "notifications" || (p.starts_with("repos/") && p.ends_with("/notifications"))
}

const REPOSITORY_FIELDS: &str = "name owner { login } nameWithOwner isArchived \
branchProtectionRules(first: 1) { nodes { requiredApprovingReviewCount requiresApprovingReviews \
requiresCodeOwnerReviews requiresStatusChecks requiredStatusCheckContexts } }";

fn pr_fields() -> String {
    format!(
        "id number title author {{ login }} authorAssociation updatedAt createdAt url state mergeable \
reviewDecision additions deletions headRefName baseRefName headRepository {{ name }} headRef {{ name }} \
repository {{ {REPOSITORY_FIELDS} }} assignees(first: 3) {{ nodes {{ login }} }} comments {{ totalCount }} \
reviewThreads {{ totalCount }} reviews {{ totalCount }} reviewRequests {{ totalCount }} isDraft isInMergeQueue \
commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} \
labels(first: 6) {{ nodes {{ color name description }} }} mergeStateStatus"
    )
}

const COUNTS_BY_STATE: &str = "totalCount checkRunCount checkRunCountsByState { count state } \
statusContextCount statusContextCountsByState { count state }";

fn pr_details_fields() -> String {
    format!(
        "id url number title body state isDraft author {{ login }} authorAssociation updatedAt createdAt \
mergeable reviewDecision additions deletions headRefName baseRefName headRepository {{ name }} \
headRef {{ name }} labels(first: 6) {{ nodes {{ color name description }} }} \
assignees(first: 3) {{ nodes {{ login }} }} repository {{ {REPOSITORY_FIELDS} }} \
commits(last: 1) {{ totalCount nodes {{ commit {{ oid commitUrl statusCheckRollup {{ state contexts(last: 100) {{ \
{COUNTS_BY_STATE} nodes {{ __typename ... on CheckRun {{ name status conclusion checkSuite {{ creator {{ login }} \
workflowRun {{ workflow {{ name }} }} }} }} ... on StatusContext {{ context state creator {{ login }} }} }} }} }} \
checkSuites(last: 20) {{ totalCount nodes {{ status conclusion app {{ name }} workflowRun {{ workflow {{ name }} }} }} }} }} }} }} \
allCommits: commits(last: 100) {{ nodes {{ commit {{ abbreviatedOid committedDate messageHeadline \
author {{ name user {{ login }} }} statusCheckRollup {{ state contexts(last: 1) {{ {COUNTS_BY_STATE} }} }} }} }} }} \
comments(last: 50) {{ totalCount nodes {{ author {{ login }} body createdAt updatedAt }} }} \
reviewThreads(last: 50) {{ totalCount nodes {{ id isOutdated line path comments(first: 20) {{ totalCount nodes {{ author {{ login }} body createdAt updatedAt }} }} }} }} \
reviewRequests(last: 100) {{ totalCount nodes {{ asCodeOwner requestedReviewer {{ __typename ... on User {{ login }} \
... on Team {{ slug name }} ... on Bot {{ login }} ... on Mannequin {{ login }} }} }} }} \
reviews(last: 100) {{ totalCount nodes {{ author {{ login }} body state updatedAt submittedAt }} }} \
suggestedReviewers {{ isAuthor isCommenter reviewer {{ login }} }} \
files(first: 20) {{ totalCount nodes {{ additions deletions path changeType }} }} \
mergeStateStatus isInMergeQueue {}",
        timeline_items(&[
            "ClosedEvent",
            "ReopenedEvent",
            "MergedEvent",
            "ReadyForReviewEvent",
            "ConvertToDraftEvent",
            "HeadRefForcePushedEvent",
            "ReviewRequestedEvent",
            "AssignedEvent",
            "UnassignedEvent",
            "LabeledEvent",
            "UnlabeledEvent",
            "RenamedTitleEvent",
        ])
    )
}

/// The newest 30 timeline events of the GraphQL `types`, each with the fields
/// its `model::TimelineEvent` variant decodes.
fn timeline_items(types: &[&str]) -> String {
    let item_type = |ty: &str| {
        let mut out = String::new();
        for (i, c) in ty.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_uppercase());
        }
        out
    };
    let fields = |ty: &str| match ty {
        "ReviewRequestedEvent" => {
            "requestedReviewer { __typename ... on User { login } ... on Team { slug name } \
... on Bot { login } ... on Mannequin { login } }"
        }
        "AssignedEvent" | "UnassignedEvent" => {
            "assignee { ... on User { login } ... on Bot { login } ... on Mannequin { login } \
... on Organization { login } }"
        }
        "LabeledEvent" | "UnlabeledEvent" => "label { color name description }",
        "RenamedTitleEvent" => "previousTitle currentTitle",
        _ => "",
    };
    let item_types: Vec<String> = types.iter().map(|t| item_type(t)).collect();
    let fragments: Vec<String> = types
        .iter()
        .map(|t| format!("... on {t} {{ actor {{ login }} createdAt {} }}", fields(t)))
        .collect();
    format!(
        "timelineItems(last: 30, itemTypes: [{}]) {{ nodes {{ __typename {} }} }}",
        item_types.join(", "),
        fragments.join(" ")
    )
}

fn issue_fields() -> String {
    format!(
        "id number title body state author {{ login }} authorAssociation updatedAt createdAt url \
repository {{ {REPOSITORY_FIELDS} }} assignees(first: 3) {{ nodes {{ login }} }} \
comments(last: 15) {{ totalCount nodes {{ author {{ login }} body createdAt updatedAt }} }} \
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
        "endCursor": page.and_then(|p| p.end_cursor.clone()),
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

/// Details for each URL, from one aliased `resource(url:)` query.
fn fetch_by_url<T: DeserializeOwned>(
    name: &str,
    kind: &str,
    fields: &str,
    urls: &[String],
) -> Result<Vec<(String, Result<T>)>> {
    if urls.is_empty() {
        return Ok(vec![]);
    }
    let params: Vec<String> = (0..urls.len()).map(|i| format!("$u{i}: URI!")).collect();
    let body: Vec<String> = (0..urls.len())
        .map(|i| format!("r{i}: resource(url: $u{i}) {{ ... on {kind} {{ {fields} }} }}"))
        .collect();
    let doc = format!(
        "query {name}({}) {{ {} }}",
        params.join(", "),
        body.join(" ")
    );
    let mut vars = Map::new();
    for (i, u) in urls.iter().enumerate() {
        vars.insert(format!("u{i}"), json!(u));
    }
    let mut partial = client()?.graphql_partial(&doc, Value::Object(vars))?;
    Ok(urls
        .iter()
        .enumerate()
        .map(|(i, url)| {
            let alias = format!("r{i}");
            let res = match (partial.data.remove(&alias), partial.errors.remove(&alias)) {
                (_, Some(e)) => Err(anyhow!("GraphQL: {e}")),
                (Some(v), None) if !v.is_null() => {
                    serde_json::from_value(v).map_err(|e| anyhow!("decoding {url}: {e}"))
                }
                _ => Err(anyhow!("not found: {url}")),
            };
            (url.clone(), res)
        })
        .collect())
}

pub fn fetch_pull_requests_by_url(
    urls: &[String],
) -> Result<Vec<(String, Result<PullRequestDetails>)>> {
    fetch_by_url(
        "FetchPullRequests",
        "PullRequest",
        &pr_details_fields(),
        urls,
    )
}

pub fn fetch_issues_by_url(urls: &[String]) -> Result<Vec<(String, Result<Issue>)>> {
    let fields = format!(
        "{} {}",
        issue_fields(),
        timeline_items(&[
            "ClosedEvent",
            "ReopenedEvent",
            "AssignedEvent",
            "UnassignedEvent",
            "LabeledEvent",
            "UnlabeledEvent",
            "RenamedTitleEvent",
        ])
    );
    fetch_by_url("FetchIssues", "Issue", &fields, urls)
}

/// Rows by node ID, 10 per request. Missing nodes are left out.
fn fetch_by_id<T: DeserializeOwned>(
    name: &str,
    kind: &str,
    fields: &str,
    ids: &[String],
) -> Result<Vec<T>> {
    let doc = format!(
        "query {name}($ids: [ID!]!) {{ nodes(ids: $ids) {{ ... on {kind} {{ {fields} }} }} }}"
    );
    let mut out = vec![];
    for chunk in ids.chunks(10) {
        let mut partial = client()?.graphql_partial(&doc, json!({ "ids": chunk }))?;
        let nodes = partial.data.remove("nodes").unwrap_or(Value::Null);
        for node in nodes.as_array().cloned().unwrap_or_default() {
            if node.is_null() || node.as_object().is_some_and(|o| o.is_empty()) {
                continue;
            }
            out.push(serde_json::from_value(node)?);
        }
    }
    Ok(out)
}

pub fn fetch_prs_by_id(ids: &[String]) -> Result<Vec<PullRequest>> {
    fetch_by_id("FetchPullRequestRows", "PullRequest", &pr_fields(), ids)
}

pub fn fetch_issues_by_id(ids: &[String]) -> Result<Vec<Issue>> {
    fetch_by_id("FetchIssueRows", "Issue", &issue_fields(), ids)
}

/// One section's search, as `fetch_pull_requests` or `fetch_issues` would run it.
#[derive(Clone, Debug)]
pub struct SyncQuery {
    pub query: String,
    pub limit: usize,
}

impl SyncQuery {
    pub fn prs(filters: &str, limit: usize) -> SyncQuery {
        SyncQuery {
            query: format!("is:pr archived:false {filters} sort:updated"),
            limit,
        }
    }

    pub fn issues(filters: &str, limit: usize) -> SyncQuery {
        SyncQuery {
            query: format!("is:issue archived:false {filters} sort:updated"),
            limit,
        }
    }
}

/// What a section's search contains now: row IDs and update times, in order.
#[derive(Clone, Debug)]
pub struct Membership {
    pub total_count: usize,
    pub page_info: PageInfo,
    pub rows: Vec<(String, DateTime<Utc>)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncNode {
    id: Option<String>,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncSearch {
    issue_count: usize,
    page_info: PageInfo,
    nodes: Vec<Option<SyncNode>>,
}

const MAX_SYNC_ALIASES: usize = 10;

/// The aliased query documents for `queries`, at most 10 searches each.
pub fn sync_documents(queries: &[SyncQuery]) -> Vec<(String, Value)> {
    queries
        .chunks(MAX_SYNC_ALIASES)
        .map(|chunk| {
            let params: Vec<String> = (0..chunk.len())
                .map(|i| format!("$q{i}: String!, $n{i}: Int!"))
                .collect();
            let body: Vec<String> = (0..chunk.len())
                .map(|i| {
                    format!(
                        "s{i}: search(type: ISSUE, first: $n{i}, query: $q{i}) {{ issueCount \
pageInfo {{ hasNextPage startCursor endCursor }} \
nodes {{ ... on PullRequest {{ id updatedAt }} ... on Issue {{ id updatedAt }} }} }}"
                    )
                })
                .collect();
            let doc = format!(
                "query SyncSections({}) {{ {} }}",
                params.join(", "),
                body.join(" ")
            );
            let mut vars = Map::new();
            for (i, q) in chunk.iter().enumerate() {
                vars.insert(format!("q{i}"), json!(q.query));
                vars.insert(format!("n{i}"), json!(q.limit.clamp(1, 100)));
            }
            (doc, Value::Object(vars))
        })
        .collect()
}

fn sync_chunk(
    queries: &[SyncQuery],
    split_on_timeout: bool,
) -> Result<Vec<Result<Membership, String>>> {
    let (doc, vars) = sync_documents(queries).remove(0);
    let mut partial = match client()?.graphql_partial(&doc, vars) {
        Err(e) if is_timeout(&e) && split_on_timeout && queries.len() > 1 => {
            let (a, b) = queries.split_at(queries.len() / 2);
            let mut out = sync_chunk(a, false)?;
            out.extend(sync_chunk(b, false)?);
            return Ok(out);
        }
        r => r?,
    };
    Ok((0..queries.len())
        .map(|i| {
            let alias = format!("s{i}");
            if let Some(e) = partial.errors.remove(&alias) {
                return Err(e);
            }
            let v = partial.data.remove(&alias).unwrap_or(Value::Null);
            let s: SyncSearch = serde_json::from_value(v).map_err(|e| e.to_string())?;
            Ok(Membership {
                total_count: s.issue_count,
                page_info: s.page_info,
                rows: s
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(|n| Some((n.id?, n.updated_at?)))
                    .collect(),
            })
        })
        .collect())
}

/// Each section's membership, from one request per 10 sections.
pub fn sync_sections(queries: &[SyncQuery]) -> Result<Vec<Result<Membership, String>>> {
    let mut out = vec![];
    for chunk in queries.chunks(MAX_SYNC_ALIASES) {
        out.extend(sync_chunk(chunk, true)?);
    }
    Ok(out)
}

/// What GitHub reports for a PR now: enough to tell whether cached details changed.
#[derive(Clone, Debug, PartialEq)]
pub struct PrVersion {
    pub id: String,
    pub url: String,
    pub updated_at: DateTime<Utc>,
    pub head_oid: Option<String>,
    pub rollup_state: Option<CheckState>,
    pub check_counts: Vec<CountByState>,
}

/// True when the cached detail no longer matches what GitHub reports.
pub fn is_outdated(detail: &PullRequestDetails, v: &PrVersion) -> bool {
    let head = detail.head();
    let rollup = head.and_then(|c| c.status_check_rollup.as_ref());
    detail.updated_at != v.updated_at
        || head.map(|c| &c.oid) != v.head_oid.as_ref()
        || rollup.map(|r| r.state) != v.rollup_state
        || rollup
            .map(|r| r.contexts.counts.by_state())
            .unwrap_or_default()
            != v.check_counts
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckCommit {
    oid: String,
    status_check_rollup: Option<StatusCheckCounts>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckNode {
    id: String,
    url: String,
    updated_at: DateTime<Utc>,
    commits: Nodes<CommitNode<CheckCommit>>,
}

/// One `nodes(ids:)` query per 100 PRs.
pub fn check_pull_requests(ids: &[String]) -> Result<Vec<PrVersion>> {
    let doc = "query CheckPullRequests($ids: [ID!]!) { nodes(ids: $ids) { ... on PullRequest { id url updatedAt \
commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(last: 1) { \
totalCount checkRunCount checkRunCountsByState { state count } \
statusContextCount statusContextCountsByState { state count } } } } } } } } }";
    let mut out = vec![];
    for chunk in ids.chunks(100) {
        let mut partial = client()?.graphql_partial(doc, json!({ "ids": chunk }))?;
        let nodes = partial.data.remove("nodes").unwrap_or(Value::Null);
        for node in nodes.as_array().cloned().unwrap_or_default() {
            let Ok(n) = serde_json::from_value::<CheckNode>(node) else {
                continue;
            };
            let head = n.commits.nodes.into_iter().last().map(|c| c.commit);
            let rollup = head.as_ref().and_then(|c| c.status_check_rollup.as_ref());
            out.push(PrVersion {
                id: n.id,
                url: n.url,
                updated_at: n.updated_at,
                rollup_state: rollup.map(|r| r.state),
                check_counts: rollup.map(|r| r.contexts.by_state()).unwrap_or_default(),
                head_oid: head.map(|c| c.oid),
            });
        }
    }
    Ok(out)
}

pub fn latest_version() -> Result<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Release {
        tag_name: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repo {
        latest_release: Option<Release>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repo>,
    }
    let d: Data = client()?.graphql(
        "query LatestVersion($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { latestRelease { tagName } } }",
        json!({ "owner": "juanibiapina", "name": "ghtui" }),
    )?;
    Ok(d.repository
        .and_then(|r| r.latest_release)
        .map(|r| r.tag_name)
        .unwrap_or_default())
}

pub fn current_login() -> Result<String> {
    #[derive(Deserialize)]
    struct Viewer {
        viewer: Login,
    }
    let v: Viewer = client()?.graphql("query UserCurrent { viewer { login } }", json!({}))?;
    Ok(v.viewer.login)
}

#[derive(Deserialize)]
struct User {
    login: String,
    name: Option<String>,
}

/// Mentionable users as (login, name), cached per repo.
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
repository(owner: $owner, name: $name) { mentionableUsers(first: $limit) { nodes { login name } } } }",
        json!({ "owner": owner, "name": name, "limit": 100 }),
    )?;
    let users: Vec<(String, String)> = d
        .repository
        .map(|r| {
            r.mentionable_users
                .nodes
                .into_iter()
                .map(|u| (u.login, u.name.unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();
    cache.lock().unwrap().insert(key, users.clone());
    Ok(users)
}

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

pub struct NotificationsPage {
    pub notifications: Vec<Notification>,
    pub page_info: PageInfo,
}

/// Pages are numbered; `end_cursor` carries the next page number.
pub fn fetch_notifications(
    limit: usize,
    repo_filters: &[String],
    page: Option<&PageInfo>,
) -> Result<NotificationsPage> {
    let c = client()?;
    let filter = "&is_done=false&all=true";
    let page_no: usize = page
        .and_then(|p| p.end_cursor.as_deref()?.parse().ok())
        .unwrap_or(1);
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
    Ok(NotificationsPage {
        notifications: all,
        page_info: PageInfo {
            has_next_page: has_next,
            start_cursor: None,
            end_cursor: Some((page_no + 1).to_string()),
        },
    })
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

pub fn fetch_recent_workflow_run(repo: &str, at: DateTime<Utc>) -> Result<String> {
    let resp: WorkflowRunsResponse =
        client()?.rest_get(&format!("repos/{repo}/actions/runs?per_page=20"))?;
    Ok(best_workflow_run(&resp.workflow_runs, at)
        .map(|r| r.html_url.clone())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(
        retry_after: Option<u64>,
        remaining: Option<i64>,
        reset: Option<i64>,
    ) -> RateHeaders {
        RateHeaders {
            retry_after,
            remaining,
            reset,
        }
    }

    #[test]
    fn retry_after_sets_the_wait() {
        let h = headers(Some(30), None, None);
        assert_eq!(
            detect_rate_limit(403, &h, "", 0),
            Some(Wait::Exactly(Duration::from_secs(30)))
        );
        assert_eq!(
            detect_rate_limit(429, &h, "", 0),
            Some(Wait::Exactly(Duration::from_secs(30)))
        );
    }

    #[test]
    fn exhausted_points_wait_for_the_reset() {
        let h = headers(None, Some(0), Some(1_000));
        assert_eq!(
            detect_rate_limit(403, &h, "", 900),
            Some(Wait::Exactly(Duration::from_secs(100)))
        );
    }

    #[test]
    fn a_429_without_headers_backs_off() {
        assert_eq!(
            detect_rate_limit(429, &RateHeaders::default(), "", 0),
            Some(Wait::Backoff)
        );
    }

    #[test]
    fn a_forbidden_response_is_not_a_rate_limit() {
        let body = r#"{"message":"Resource not accessible by integration"}"#;
        assert_eq!(
            detect_rate_limit(403, &RateHeaders::default(), body, 0),
            None
        );
    }

    #[test]
    fn graphql_rate_limited_errors_are_rate_limits() {
        let body = r#"{"data":null,"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        assert_eq!(
            detect_rate_limit(200, &RateHeaders::default(), body, 0),
            Some(Wait::Backoff)
        );
    }

    #[test]
    fn secondary_limits_reported_with_status_200_are_rate_limits() {
        let body = r#"{"errors":[{"message":"You have exceeded a secondary rate limit."}]}"#;
        assert_eq!(
            detect_rate_limit(200, &RateHeaders::default(), body, 0),
            Some(Wait::Backoff)
        );
        assert_eq!(
            detect_rate_limit(200, &RateHeaders::default(), "{}", 0),
            None
        );
    }

    #[test]
    fn backoff_doubles_up_to_fifteen_minutes() {
        assert_eq!(backoff(0), Duration::from_secs(60));
        assert_eq!(backoff(1), Duration::from_secs(120));
        assert_eq!(backoff(3), Duration::from_secs(480));
        assert_eq!(backoff(4), Duration::from_secs(900));
        assert_eq!(backoff(30), Duration::from_secs(900));
    }

    #[test]
    fn partial_responses_keep_the_aliases_that_succeeded() {
        let body = r#"{"data":{"a":{"number":9000},"b":null,"c":null},"errors":[{"type":"NOT_FOUND","path":["c"],"locations":[{"line":1,"column":196}],"message":"Could not resolve to a node with the global id of 'PR_doesnotexist'"}]}"#;
        let p = decode_partial(body).unwrap();
        assert_eq!(p.data["a"]["number"], 9000);
        assert!(p.data["b"].is_null());
        assert!(!p.errors.contains_key("b"));
        assert!(p.errors["c"].contains("Could not resolve"));
    }

    #[test]
    fn a_response_that_is_not_json_is_a_timeout() {
        let e = decode_partial("<html>We couldn't respond</html>")
            .err()
            .unwrap();
        assert!(is_timeout(&e));
    }

    #[test]
    fn sync_sends_at_most_ten_searches_per_request() {
        let queries: Vec<SyncQuery> = (0..12)
            .map(|i| SyncQuery::prs(&format!("repo:a/{i}"), 20))
            .collect();
        let docs = sync_documents(&queries);
        assert_eq!(docs.len(), 2);
        assert!(docs[0].0.contains("s9:"));
        assert!(!docs[0].0.contains("s10:"));
        assert!(docs[1].0.contains("s1:"));
        assert!(!docs[1].0.contains("s2:"));
        assert_eq!(
            docs[1].1["q1"],
            "is:pr archived:false repo:a/11 sort:updated"
        );
    }

    #[test]
    fn different_tokens_name_different_accounts() {
        assert_ne!(
            account_key_for("github.com", "a"),
            account_key_for("github.com", "b")
        );
        assert_eq!(
            account_key_for("github.com", "a"),
            account_key_for("github.com", "a")
        );
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
    }
}
