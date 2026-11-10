//! Cached PR and issue details by URL. One request per URL at a time, a
//! background lane that fetches a prioritized queue in batches, and freshness
//! driven by evidence (`mark_stale`) instead of timers.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry<T> {
    pub value: T,
    pub fetched_at: DateTime<Utc>,
    pub stale: bool,
}

pub type FetchResult<T> = anyhow::Result<Vec<(String, anyhow::Result<T>)>>;

/// What to do with a batch whose request failed as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retry {
    /// Put it back at the front of the queue (rate limited).
    Later,
    /// Retry each URL in a request of its own (timed out).
    OneByOne,
    /// Deliver the error.
    Fail,
}

type Fetch<T> = Arc<dyn Fn(&[String], bool) -> FetchResult<T> + Send + Sync>;
type Deliver<T> = Arc<dyn Fn(String, Result<T, String>) + Send + Sync>;

/// How the background lane paces itself.
#[derive(Clone)]
pub struct LanePolicy {
    pub batch_size: usize,
    /// How long to wait before the next batch, if at all.
    pub wait: Arc<dyn Fn() -> Option<Duration> + Send + Sync>,
    pub retry: Arc<dyn Fn(&anyhow::Error) -> Retry + Send + Sync>,
}

struct Inner<T> {
    cache: HashMap<String, Entry<T>>,
    in_flight: HashSet<String>,
    stale_during: HashSet<String>,
    queue: VecDeque<String>,
    solo: HashSet<String>,
    active: bool,
    closed: bool,
}

impl<T> Inner<T> {
    fn fresh(&self, url: &str) -> bool {
        self.cache.get(url).is_some_and(|e| !e.stale)
    }
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    wake: Condvar,
    fetch: Fetch<T>,
    deliver: Deliver<T>,
    policy: LanePolicy,
}

pub struct DetailStore<T> {
    shared: Arc<Shared<T>>,
}

impl<T: Clone + Send + 'static> DetailStore<T> {
    /// `fetch(urls, background)` is the batched transport. `deliver` receives
    /// every finished URL, whoever asked for it.
    pub fn new(
        fetch: impl Fn(&[String], bool) -> FetchResult<T> + Send + Sync + 'static,
        deliver: impl Fn(String, Result<T, String>) + Send + Sync + 'static,
        policy: LanePolicy,
    ) -> Self {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                cache: HashMap::new(),
                in_flight: HashSet::new(),
                stale_during: HashSet::new(),
                queue: VecDeque::new(),
                solo: HashSet::new(),
                active: true,
                closed: false,
            }),
            wake: Condvar::new(),
            fetch: Arc::new(fetch),
            deliver: Arc::new(deliver),
            policy,
        });
        let lane = shared.clone();
        std::thread::spawn(move || run_lane(lane));
        DetailStore { shared }
    }

    /// Cached value, stale or not. Never blocks on the network.
    pub fn get(&self, url: &str) -> Option<Entry<T>> {
        self.shared.inner.lock().unwrap().cache.get(url).cloned()
    }

    /// Reads a cached entry in place, without cloning it.
    pub fn peek<R>(&self, url: &str, f: impl FnOnce(&Entry<T>) -> R) -> Option<R> {
        self.shared.inner.lock().unwrap().cache.get(url).map(f)
    }

    /// The user is waiting: fetch this URL now on its own, unless it is in flight already.
    pub fn fetch_now(&self, url: &str) {
        {
            let mut g = self.shared.inner.lock().unwrap();
            if !g.in_flight.insert(url.to_string()) {
                return;
            }
        }
        let shared = self.shared.clone();
        let batch = vec![url.to_string()];
        std::thread::spawn(move || {
            let res = (shared.fetch)(&batch, false);
            complete(&shared, &batch, res, false);
        });
    }

    /// Replaces the pending background queue with `urls`, in priority order.
    /// Fresh and in-flight URLs are skipped.
    pub fn prefetch(&self, urls: Vec<String>) {
        let mut g = self.shared.inner.lock().unwrap();
        let mut seen = HashSet::new();
        let queue: VecDeque<String> = urls
            .into_iter()
            .filter(|u| seen.insert(u.clone()))
            .filter(|u| !g.in_flight.contains(u) && !g.fresh(u))
            .collect();
        g.queue = queue;
        drop(g);
        self.shared.wake.notify_all();
    }

    /// Keeps showing the value, but loads it again on the next prefetch.
    pub fn mark_stale(&self, url: &str) {
        let mut g = self.shared.inner.lock().unwrap();
        if let Some(e) = g.cache.get_mut(url) {
            e.stale = true;
        }
        if g.in_flight.contains(url) {
            g.stale_during.insert(url.to_string());
        }
    }

    pub fn mark_all_stale(&self) {
        let mut g = self.shared.inner.lock().unwrap();
        for e in g.cache.values_mut() {
            e.stale = true;
        }
        let in_flight: Vec<String> = g.in_flight.iter().cloned().collect();
        g.stale_during.extend(in_flight);
    }

    /// When inactive, the background lane finishes its current batch and starts no new one.
    pub fn set_active(&self, active: bool) {
        self.shared.inner.lock().unwrap().active = active;
        self.shared.wake.notify_all();
    }

    /// Loads entries saved by an earlier session. Entries already present win.
    pub fn seed(&self, entries: Vec<(String, Entry<T>)>) {
        let mut g = self.shared.inner.lock().unwrap();
        for (url, e) in entries {
            g.cache.entry(url).or_insert(e);
        }
    }

    /// The `limit` most recently fetched entries.
    pub fn snapshot(&self, limit: usize) -> Vec<(String, Entry<T>)> {
        let g = self.shared.inner.lock().unwrap();
        let mut v: Vec<(String, Entry<T>)> = g
            .cache
            .iter()
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect();
        v.sort_by_key(|(_, e)| std::cmp::Reverse(e.fetched_at));
        v.truncate(limit);
        v
    }

    /// True while a request is in flight.
    pub fn busy(&self) -> bool {
        !self.shared.inner.lock().unwrap().in_flight.is_empty()
    }
}

impl<T> Drop for DetailStore<T> {
    fn drop(&mut self) {
        self.shared.inner.lock().unwrap().closed = true;
        self.shared.wake.notify_all();
    }
}

fn run_lane<T: Clone + Send + 'static>(shared: Arc<Shared<T>>) {
    loop {
        let batch = {
            let mut g = shared.inner.lock().unwrap();
            loop {
                if g.closed {
                    return;
                }
                if !g.active || g.queue.is_empty() {
                    g = shared.wake.wait(g).unwrap();
                    continue;
                }
                if let Some(wait) = (shared.policy.wait)() {
                    let wait = wait.min(Duration::from_secs(1));
                    g = shared.wake.wait_timeout(g, wait).unwrap().0;
                    continue;
                }
                let mut batch: Vec<String> = vec![];
                while let Some(url) = g.queue.pop_front() {
                    if g.in_flight.contains(&url) || g.fresh(&url) {
                        continue;
                    }
                    if g.solo.contains(&url) {
                        if batch.is_empty() {
                            batch.push(url);
                        } else {
                            g.queue.push_front(url);
                        }
                        break;
                    }
                    batch.push(url);
                    if batch.len() >= shared.policy.batch_size {
                        break;
                    }
                }
                if batch.is_empty() {
                    continue;
                }
                for u in &batch {
                    g.in_flight.insert(u.clone());
                }
                break batch;
            }
        };
        let res = (shared.fetch)(&batch, true);
        complete(&shared, &batch, res, true);
    }
}

fn complete<T: Clone>(shared: &Shared<T>, batch: &[String], res: FetchResult<T>, background: bool) {
    let mut deliveries: Vec<(String, Result<T, String>)> = vec![];
    {
        let mut g = shared.inner.lock().unwrap();
        for u in batch {
            g.in_flight.remove(u);
        }
        match res {
            Ok(results) => {
                let mut answered = HashSet::new();
                for (url, r) in results {
                    answered.insert(url.clone());
                    g.solo.remove(&url);
                    match r {
                        Ok(v) => {
                            let stale = g.stale_during.remove(&url);
                            g.cache.insert(
                                url.clone(),
                                Entry {
                                    value: v.clone(),
                                    fetched_at: Utc::now(),
                                    stale,
                                },
                            );
                            deliveries.push((url, Ok(v)));
                        }
                        Err(e) => deliveries.push((url, Err(e.to_string()))),
                    }
                }
                for u in batch.iter().filter(|u| !answered.contains(*u)) {
                    deliveries.push((u.clone(), Err("missing from the response".into())));
                }
            }
            Err(e) => {
                let retry = (shared.policy.retry)(&e);
                match retry {
                    Retry::Later if background => {
                        for u in batch.iter().rev() {
                            g.queue.push_front(u.clone());
                        }
                    }
                    Retry::OneByOne if batch.len() > 1 => {
                        for u in batch.iter().rev() {
                            g.solo.insert(u.clone());
                            g.queue.push_front(u.clone());
                        }
                    }
                    _ => {
                        for u in batch {
                            g.solo.remove(u);
                            deliveries.push((u.clone(), Err(e.to_string())));
                        }
                    }
                }
            }
        }
    }
    shared.wake.notify_all();
    for (url, r) in deliveries {
        (shared.deliver)(url, r);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Instant;

    use anyhow::anyhow;

    use super::*;

    type Calls = Arc<Mutex<Vec<Vec<String>>>>;

    struct Harness {
        store: DetailStore<String>,
        calls: Calls,
        delivered: Receiver<(String, Result<String, String>)>,
    }

    /// A store whose transport answers "v:<url>", fails URLs containing "bad",
    /// and runs `before` first (to block or fail a request).
    fn harness(
        before: impl Fn(&[String]) -> Option<anyhow::Error> + Send + Sync + 'static,
        wait: Arc<dyn Fn() -> Option<Duration> + Send + Sync>,
    ) -> Harness {
        let calls: Calls = Arc::default();
        let (tx, delivered) = channel();
        let tx = Mutex::new(tx);
        let c = calls.clone();
        let store = DetailStore::new(
            move |urls: &[String], _bg| {
                c.lock().unwrap().push(urls.to_vec());
                if let Some(e) = before(urls) {
                    return Err(e);
                }
                Ok(urls
                    .iter()
                    .map(|u| {
                        let r = if u.contains("bad") {
                            Err(anyhow!("boom"))
                        } else {
                            Ok(format!("v:{u}"))
                        };
                        (u.clone(), r)
                    })
                    .collect())
            },
            move |url, r| {
                let _ = tx.lock().unwrap().send((url, r));
            },
            LanePolicy {
                batch_size: 5,
                wait,
                retry: Arc::new(|e| {
                    if e.to_string() == "limited" {
                        Retry::Later
                    } else {
                        Retry::Fail
                    }
                }),
            },
        );
        Harness {
            store,
            calls,
            delivered,
        }
    }

    fn simple() -> Harness {
        harness(|_| None, Arc::new(|| None))
    }

    fn urls(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("u{i}")).collect()
    }

    fn collect(h: &Harness, n: usize) -> Vec<(String, Result<String, String>)> {
        (0..n)
            .map(|_| h.delivered.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect()
    }

    fn nothing_delivered(h: &Harness) -> bool {
        h.delivered
            .recv_timeout(Duration::from_millis(150))
            .is_err()
    }

    #[test]
    fn prefetch_fetches_in_batches_of_five_in_queue_order() {
        let h = simple();
        h.store.prefetch(urls(7));
        collect(&h, 7);
        assert_eq!(
            *h.calls.lock().unwrap(),
            vec![urls(5), vec!["u5".into(), "u6".into()]]
        );
        assert_eq!(h.store.get("u6").unwrap().value, "v:u6");
    }

    #[test]
    fn a_new_prefetch_replaces_the_queue() {
        let h = simple();
        h.store.set_active(false);
        h.store.prefetch(vec!["a".into(), "b".into(), "c".into()]);
        h.store.prefetch(vec!["c".into(), "d".into()]);
        h.store.set_active(true);
        collect(&h, 2);
        assert!(nothing_delivered(&h));
        assert_eq!(
            *h.calls.lock().unwrap(),
            vec![vec!["c".to_string(), "d".to_string()]]
        );
    }

    #[test]
    fn fresh_entries_are_skipped_and_stale_ones_reloaded() {
        let h = simple();
        h.store.prefetch(vec!["a".into()]);
        collect(&h, 1);
        h.store.prefetch(vec!["a".into()]);
        assert!(nothing_delivered(&h));
        h.store.mark_stale("a");
        assert!(h.store.get("a").unwrap().stale);
        h.store.prefetch(vec!["a".into()]);
        collect(&h, 1);
        assert!(!h.store.get("a").unwrap().stale);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn failed_urls_are_delivered_and_not_cached() {
        let h = simple();
        h.store.prefetch(vec!["bad".into(), "ok".into()]);
        let got = collect(&h, 2);
        assert!(got.iter().any(|(u, r)| u == "bad" && r.is_err()));
        assert!(h.store.get("bad").is_none());
        assert!(h.store.get("ok").is_some());
    }

    #[test]
    fn a_url_in_flight_is_fetched_once() {
        let release = Arc::new(AtomicBool::new(false));
        let r = release.clone();
        let h = harness(
            move |_| {
                while !r.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                None
            },
            Arc::new(|| None),
        );
        h.store.prefetch(vec!["a".into()]);
        while !h.store.busy() {
            std::thread::sleep(Duration::from_millis(5));
        }
        h.store.fetch_now("a");
        h.store.prefetch(vec!["a".into()]);
        release.store(true, Ordering::SeqCst);
        collect(&h, 1);
        assert!(nothing_delivered(&h));
        assert_eq!(h.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_rate_limited_batch_is_retried_later() {
        let failed = Arc::new(AtomicBool::new(false));
        let f = failed.clone();
        let h = harness(
            move |_| (!f.swap(true, Ordering::SeqCst)).then(|| anyhow!("limited")),
            Arc::new(|| None),
        );
        h.store.prefetch(vec!["a".into(), "b".into()]);
        let got = collect(&h, 2);
        assert!(got.iter().all(|(_, r)| r.is_ok()));
        let calls = h.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
    }

    #[test]
    fn the_lane_waits_while_told_to() {
        let blocked = Arc::new(AtomicBool::new(true));
        let b = blocked.clone();
        let h = harness(
            |_| None,
            Arc::new(move || {
                b.load(Ordering::SeqCst)
                    .then_some(Duration::from_millis(20))
            }),
        );
        h.store.prefetch(vec!["a".into()]);
        assert!(nothing_delivered(&h));
        blocked.store(false, Ordering::SeqCst);
        collect(&h, 1);
    }

    #[test]
    fn an_inactive_store_still_fetches_what_the_user_asks_for() {
        let h = simple();
        h.store.set_active(false);
        h.store.prefetch(vec!["queued".into()]);
        h.store.fetch_now("now");
        let got = collect(&h, 1);
        assert_eq!(got[0].0, "now");
        assert!(nothing_delivered(&h));
        h.store.set_active(true);
        assert_eq!(collect(&h, 1)[0].0, "queued");
    }

    #[test]
    fn snapshot_keeps_the_newest_entries() {
        let h = simple();
        let old = Entry {
            value: "old".to_string(),
            fetched_at: Utc::now() - chrono::Duration::hours(1),
            stale: true,
        };
        h.store.seed(vec![("x".into(), old)]);
        let started = Instant::now();
        h.store.prefetch(vec!["y".into()]);
        collect(&h, 1);
        assert!(started.elapsed() < Duration::from_secs(2));
        let snap = h.store.snapshot(1);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, "y");
        assert!(h.store.get("x").unwrap().stale);
    }
}
