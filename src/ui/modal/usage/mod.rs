pub mod fetch;
mod input;
pub mod opencode_api;
pub mod render;
pub mod stats;
mod stats_render;

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::core::usage::offline_summary;
use crate::infra::usage::UsageService;

pub use crate::core::usage::{
    format_reset_time, parse_opencode_stats_json, OpenCodeStats, QuotaBucket, QuotaGroup,
    StatsState, UsagePage, UsagePlanInfo, UsageState, UsageSummary,
};
pub use stats_render::build_stats_lines;

#[derive(Clone, Debug)]
pub struct UsageModal {
    pub state: Arc<Mutex<UsageState>>,
    pub scroll_offset: usize,
    pub max_scroll: Cell<usize>,
    pub allow_network: bool,
    pub allow_stats_cli: bool,
    pub is_opencode: bool,
    pub page: UsagePage,
    pub stats_state: Arc<Mutex<StatsState>>,
    service: UsageService,
    usage_req_id: u64,
    stats_req_id: u64,
    opencode_fallback: Option<UsageSummary>,
    rx: Option<SharedResultRx<UsageState>>,
    stats_rx: Option<SharedResultRx<StatsState>>,
    body_cache: RefCell<render::UsageBodyCache>,
}

type SharedResultRx<T> = Arc<Mutex<Receiver<(u64, T)>>>;

impl UsageModal {
    pub fn load(allow_network: bool) -> Self {
        Self::load_with_service(UsageService::new(), allow_network)
    }

    pub fn load_with_service(service: UsageService, allow_network: bool) -> Self {
        if !allow_network {
            return Self::offline_fallback().with_usage_service(service);
        }

        if let Some(summary) = service.cached_agy(Duration::from_secs(60)) {
            let mut modal = Self::with_data(summary).with_usage_service(service);
            modal.allow_network = true;
            return modal;
        }

        let (tx, rx) = channel();
        let req_id = 1;
        service.fetch_agy_async(req_id, tx);

        Self {
            state: Arc::new(Mutex::new(UsageState::Loading)),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: true,
            allow_stats_cli: false,
            is_opencode: false,
            page: UsagePage::Main,
            stats_state: Arc::new(Mutex::new(StatsState::Idle)),
            service,
            usage_req_id: req_id,
            stats_req_id: 0,
            opencode_fallback: None,
            rx: Some(Arc::new(Mutex::new(rx))),
            stats_rx: None,
            body_cache: Default::default(),
        }
    }

    pub fn with_data(summary: UsageSummary) -> Self {
        Self {
            state: Arc::new(Mutex::new(UsageState::Ready(summary))),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: false,
            allow_stats_cli: false,
            is_opencode: false,
            page: UsagePage::Main,
            stats_state: Arc::new(Mutex::new(StatsState::Idle)),
            service: UsageService::new(),
            usage_req_id: 0,
            stats_req_id: 0,
            opencode_fallback: None,
            rx: None,
            stats_rx: None,
            body_cache: Default::default(),
        }
    }

    pub fn with_usage_service(mut self, service: UsageService) -> Self {
        self.service = service;
        self
    }

    pub fn with_stats_cli(mut self, allow: bool) -> Self {
        self.allow_stats_cli = allow;
        self
    }

    pub fn load_opencode(
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> Self {
        Self::load_opencode_with_service(UsageService::new(), model, last_turn, accumulated)
    }

    pub fn load_opencode_with_service(
        service: UsageService,
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> Self {
        let fallback = Self::local_opencode_summary(model, last_turn, accumulated);
        if let Some(summary) = service.cached_opencode(Duration::from_secs(60)) {
            let mut modal = Self::with_data(summary).with_usage_service(service);
            modal.is_opencode = true;
            modal.allow_network = true;
            modal.allow_stats_cli = true;
            modal.opencode_fallback = Some(fallback);
            return modal;
        }
        let (tx, rx) = channel();
        let req_id = 1;
        service.fetch_opencode_async(fallback.clone(), req_id, tx);
        Self {
            state: Arc::new(Mutex::new(UsageState::Loading)),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: true,
            allow_stats_cli: true,
            is_opencode: true,
            page: UsagePage::Main,
            stats_state: Arc::new(Mutex::new(StatsState::Idle)),
            service,
            usage_req_id: req_id,
            stats_req_id: 0,
            opencode_fallback: Some(fallback),
            rx: Some(Arc::new(Mutex::new(rx))),
            stats_rx: None,
            body_cache: Default::default(),
        }
    }

    pub fn for_opencode(
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> Self {
        let summary = Self::local_opencode_summary(model, last_turn, accumulated);
        let mut modal = Self::with_data(summary.clone());
        modal.is_opencode = true;
        modal.allow_stats_cli = true;
        modal.opencode_fallback = Some(summary);
        modal
    }

    pub(crate) fn local_opencode_summary(
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> UsageSummary {
        let spec = crate::model_spec::ModelSpec::resolve(&[model], None, &[]);
        let used = last_turn
            .map(|usage| usage.input_tokens.saturating_add(usage.cache_read_tokens))
            .unwrap_or(0);
        let (context_description, context_buckets) = match spec.usable() {
            Some(window) => {
                let remaining_pct =
                    (100.0 - (used as f64 / window as f64) * 100.0).clamp(0.0, 100.0);
                (
                    format!("{} / {} tokens", used, window),
                    vec![QuotaBucket {
                        name: "Context capacity".to_string(),
                        remaining_pct,
                        reset_label: "latest turn".to_string(),
                        exhausted: remaining_pct <= 0.0,
                    }],
                )
            }
            None => (format!("{} tokens · window unresolved", used), Vec::new()),
        };
        let cache_pct = accumulated.cache_hit_percentage();
        let total_tokens = accumulated.effective_total();
        UsageSummary {
            plan: UsagePlanInfo {
                tier_name: format!(
                    "OpenCode · {}",
                    crate::ui::labels::model_display_name(model)
                ),
                account_email: None,
                upgrade_url: None,
                manage_url: None,
            },
            groups: vec![
                QuotaGroup {
                    name: "Context Window".to_string(),
                    description: Some(context_description),
                    buckets: context_buckets,
                },
                QuotaGroup {
                    name: "Prompt Cache".to_string(),
                    description: None,
                    buckets: vec![QuotaBucket {
                        name: "Cache hit rate".to_string(),
                        remaining_pct: cache_pct,
                        reset_label: format!("{:.1}%", cache_pct),
                        exhausted: false,
                    }],
                },
                QuotaGroup {
                    name: "Session Tokens".to_string(),
                    description: None,
                    buckets: vec![QuotaBucket {
                        name: "Accumulated".to_string(),
                        remaining_pct: 100.0,
                        reset_label: format!("{} tokens", total_tokens),
                        exhausted: false,
                    }],
                },
            ],
        }
    }

    pub fn offline_fallback() -> Self {
        Self::with_data(offline_summary())
    }

    pub fn refresh(&mut self) {
        if self.page == UsagePage::OpenCodeStats {
            self.refresh_stats();
            return;
        }
        if !self.allow_network || self.is_loading() {
            return;
        }
        self.usage_req_id = self.usage_req_id.wrapping_add(1);
        let req_id = self.usage_req_id;
        if self.is_opencode {
            self.service.clear_opencode_cache();
            let fallback = self
                .opencode_fallback
                .clone()
                .unwrap_or_else(|| Self::local_opencode_summary("", None, Default::default()));
            let (tx, rx) = channel();
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = UsageState::Loading;
            self.rx = Some(Arc::new(Mutex::new(rx)));
            self.service.fetch_opencode_async(fallback, req_id, tx);
            return;
        }
        self.service.clear_agy_cache();
        let (tx, rx) = channel();
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = UsageState::Loading;
        self.rx = Some(Arc::new(Mutex::new(rx)));
        self.service.fetch_agy_async(req_id, tx);
    }

    pub fn open_stats_subpage(&mut self) {
        if !self.is_opencode {
            return;
        }
        self.page = UsagePage::OpenCodeStats;
        self.scroll_offset = 0;
        self.max_scroll.set(0);
        let already_loaded = matches!(
            *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()),
            StatsState::Ready(_) | StatsState::Loading
        );
        if already_loaded {
            return;
        }
        if let Some(stats) = self.service.cached_stats(Duration::from_secs(60)) {
            *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) = StatsState::Ready(stats);
            return;
        }
        if !self.allow_stats_cli {
            *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) =
                StatsState::Unavailable("Offline mode".to_string());
            return;
        }
        self.start_stats_fetch();
    }

    pub fn refresh_stats(&mut self) {
        if !self.is_opencode {
            return;
        }
        if matches!(
            *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()),
            StatsState::Loading
        ) {
            return;
        }
        if !self.allow_stats_cli {
            *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) =
                StatsState::Unavailable("Offline mode".to_string());
            return;
        }
        self.service.clear_stats_cache();
        self.start_stats_fetch();
    }

    fn start_stats_fetch(&mut self) {
        self.stats_req_id = self.stats_req_id.wrapping_add(1);
        let req_id = self.stats_req_id;
        let (tx, rx) = channel();
        *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) = StatsState::Loading;
        self.stats_rx = Some(Arc::new(Mutex::new(rx)));
        self.service.fetch_stats_async(req_id, tx);
    }

    pub fn is_loading(&self) -> bool {
        let primary_loading = matches!(
            *self.state.lock().unwrap_or_else(|e| e.into_inner()),
            UsageState::Loading
        );
        let stats_loading = self.page == UsagePage::OpenCodeStats
            && matches!(
                *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()),
                StatsState::Loading
            );
        primary_loading || stats_loading
    }

    pub fn poll_rx(&self) -> bool {
        let mut updated = false;
        if let Some(ref rx_lock) = self.rx {
            let rx = rx_lock.lock().unwrap_or_else(|e| e.into_inner());
            while let Ok((req_id, new_state)) = rx.try_recv() {
                if req_id == self.usage_req_id {
                    let mut current = self.state.lock().unwrap_or_else(|e| e.into_inner());
                    *current = new_state;
                    updated = true;
                }
            }
        }
        if let Some(ref rx_lock) = self.stats_rx {
            let rx = rx_lock.lock().unwrap_or_else(|e| e.into_inner());
            while let Ok((req_id, new_stats)) = rx.try_recv() {
                if req_id == self.stats_req_id {
                    let mut current = self.stats_state.lock().unwrap_or_else(|e| e.into_inner());
                    *current = new_stats;
                    updated = true;
                }
            }
        }
        if updated {
            self.max_scroll.set(0);
        }
        updated
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        render::render_modal(self, frame, area);
    }
}
