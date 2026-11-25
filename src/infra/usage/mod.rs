pub mod cli;
pub mod fetch;
pub mod opencode_api;
pub mod stats;

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::usage::{OpenCodeStats, StatsState, UsageState, UsageSummary};

#[derive(Debug)]
struct CoalescedFetch<T> {
    in_flight: bool,
    waiters: Vec<(u64, Sender<(u64, T)>)>,
}

impl<T> Default for CoalescedFetch<T> {
    fn default() -> Self {
        Self {
            in_flight: false,
            waiters: Vec::new(),
        }
    }
}

struct CoalescedFetchGuard<T: Clone> {
    state: Arc<Mutex<CoalescedFetch<T>>>,
    interrupted_value: T,
}

impl<T: Clone> Drop for CoalescedFetchGuard<T> {
    fn drop(&mut self) {
        let waiters = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !guard.in_flight {
                return;
            }
            guard.in_flight = false;
            std::mem::take(&mut guard.waiters)
        };
        for (req_id, waiter) in waiters {
            let _ = waiter.send((req_id, self.interrupted_value.clone()));
        }
    }
}

#[derive(Debug, Default)]
struct UsageServiceInner {
    agy_cache: Mutex<Option<(Instant, UsageSummary)>>,
    opencode_usage_cache: Mutex<Option<(Instant, UsageSummary)>>,
    opencode_stats_cache: Mutex<Option<(Instant, OpenCodeStats)>>,
    agy_fetch: Arc<Mutex<CoalescedFetch<UsageState>>>,
    opencode_fetch: Arc<Mutex<CoalescedFetch<UsageState>>>,
    stats_fetch: Arc<Mutex<CoalescedFetch<StatsState>>>,
}

#[derive(Clone, Debug, Default)]
pub struct UsageService {
    inner: Arc<UsageServiceInner>,
}

impl UsageService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cached_agy(&self, ttl: Duration) -> Option<UsageSummary> {
        let guard = self
            .inner
            .agy_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .and_then(|(instant, summary)| (instant.elapsed() < ttl).then(|| summary.clone()))
    }

    pub fn clear_agy_cache(&self) {
        let mut guard = self
            .inner
            .agy_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    pub fn cached_opencode(&self, ttl: Duration) -> Option<UsageSummary> {
        let guard = self
            .inner
            .opencode_usage_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .and_then(|(instant, summary)| (instant.elapsed() < ttl).then(|| summary.clone()))
    }

    pub fn clear_opencode_cache(&self) {
        let mut guard = self
            .inner
            .opencode_usage_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    pub fn cached_stats(&self, ttl: Duration) -> Option<OpenCodeStats> {
        let guard = self
            .inner
            .opencode_stats_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .and_then(|(instant, stats)| (instant.elapsed() < ttl).then(|| stats.clone()))
    }

    pub fn clear_stats_cache(&self) {
        let mut guard = self
            .inner
            .opencode_stats_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    pub fn fetch_agy_async(&self, req_id: u64, tx: Sender<(u64, UsageState)>) {
        if !enqueue_waiter(&self.inner.agy_fetch, req_id, tx) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            let _guard = CoalescedFetchGuard {
                state: Arc::clone(&inner.agy_fetch),
                interrupted_value: UsageState::Unavailable("Quota worker interrupted.".to_string()),
            };
            let state = fetch::execute_fetch();
            if let UsageState::Ready(ref summary) = state {
                let mut cache = inner.agy_cache.lock().unwrap_or_else(|e| e.into_inner());
                *cache = Some((Instant::now(), summary.clone()));
            }
            complete_waiters(&inner.agy_fetch, state);
        });
    }

    pub fn fetch_opencode_async(
        &self,
        fallback: UsageSummary,
        req_id: u64,
        tx: Sender<(u64, UsageState)>,
    ) {
        if !enqueue_waiter(&self.inner.opencode_fetch, req_id, tx) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            let _guard = CoalescedFetchGuard {
                state: Arc::clone(&inner.opencode_fetch),
                interrupted_value: UsageState::Ready(fallback.clone()),
            };
            let state = opencode_api::execute_fetch(&fallback);
            if let UsageState::Ready(ref summary) = state {
                let mut cache = inner
                    .opencode_usage_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                *cache = Some((Instant::now(), summary.clone()));
            }
            complete_waiters(&inner.opencode_fetch, state);
        });
    }

    pub fn fetch_stats_async(&self, req_id: u64, tx: Sender<(u64, StatsState)>) {
        if !enqueue_waiter(&self.inner.stats_fetch, req_id, tx) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            let _guard = CoalescedFetchGuard {
                state: Arc::clone(&inner.stats_fetch),
                interrupted_value: StatsState::Unavailable("Stats worker interrupted".to_string()),
            };
            let state = stats::execute_stats_fetch();
            if let StatsState::Ready(ref stats) = state {
                let mut cache = inner
                    .opencode_stats_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                *cache = Some((Instant::now(), stats.clone()));
            }
            complete_waiters(&inner.stats_fetch, state);
        });
    }
}

fn enqueue_waiter<T>(slot: &Mutex<CoalescedFetch<T>>, req_id: u64, tx: Sender<(u64, T)>) -> bool {
    let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
    state.waiters.push((req_id, tx));
    if state.in_flight {
        return false;
    }
    state.in_flight = true;
    true
}

fn complete_waiters<T: Clone>(slot: &Mutex<CoalescedFetch<T>>, result: T) {
    let waiters = {
        let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
        state.in_flight = false;
        std::mem::take(&mut state.waiters)
    };
    for (req_id, waiter) in waiters {
        let _ = waiter.send((req_id, result.clone()));
    }
}
