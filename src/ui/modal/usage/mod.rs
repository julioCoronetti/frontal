pub mod cli;
pub mod fetch;
pub mod opencode_api;
mod quota_parse;
pub mod render;
pub mod stats;
mod stats_render;

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::Frame;

pub use fetch::format_reset_time;
pub use stats::{
    build_stats_lines, parse_opencode_stats_json, OpenCodeStats, StatsState, UsagePage,
    OPENCODE_STATS_CACHE,
};

use crate::ui::modal::ModalAction;

pub static USAGE_CACHE: Mutex<Option<(Instant, UsageSummary)>> = Mutex::new(None);
pub static OPENCODE_USAGE_CACHE: Mutex<Option<(Instant, UsageSummary)>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq)]
pub struct UsagePlanInfo {
    pub tier_name: String,
    pub account_email: Option<String>,
    pub upgrade_url: Option<String>,
    pub manage_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuotaBucket {
    pub name: String,
    pub remaining_pct: f64,
    pub reset_label: String,
    pub exhausted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuotaGroup {
    pub name: String,
    pub description: Option<String>,
    pub buckets: Vec<QuotaBucket>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageSummary {
    pub plan: UsagePlanInfo,
    pub groups: Vec<QuotaGroup>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UsageState {
    Loading,
    Ready(UsageSummary),
    Unavailable(String),
}

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
    opencode_fallback: Option<UsageSummary>,
    rx: Option<Arc<Mutex<Receiver<UsageState>>>>,
    stats_rx: Option<Arc<Mutex<Receiver<StatsState>>>>,
    body_cache: RefCell<render::UsageBodyCache>,
}

impl UsageModal {
    pub fn load(allow_network: bool) -> Self {
        if !allow_network {
            return Self::offline_fallback();
        }

        {
            let guard = USAGE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((instant, ref summary)) = *guard {
                if instant.elapsed() < Duration::from_secs(60) {
                    let mut modal = Self::with_data(summary.clone());
                    modal.allow_network = true;
                    return modal;
                }
            }
        }

        let (tx, rx) = channel();
        fetch::fetch_quota_async(tx);

        Self {
            state: Arc::new(Mutex::new(UsageState::Loading)),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: true,
            allow_stats_cli: false,
            is_opencode: false,
            page: UsagePage::Main,
            stats_state: Arc::new(Mutex::new(StatsState::Idle)),
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
            opencode_fallback: None,
            rx: None,
            stats_rx: None,
            body_cache: Default::default(),
        }
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
        let fallback = Self::local_opencode_summary(model, last_turn, accumulated);
        {
            let guard = OPENCODE_USAGE_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((instant, ref summary)) = *guard {
                if instant.elapsed() < Duration::from_secs(60) {
                    let mut modal = Self::with_data(summary.clone());
                    modal.is_opencode = true;
                    modal.allow_network = true;
                    modal.allow_stats_cli = true;
                    modal.opencode_fallback = Some(fallback);
                    return modal;
                }
            }
        }
        let (tx, rx) = channel();
        opencode_api::fetch_async(fallback.clone(), tx);
        Self {
            state: Arc::new(Mutex::new(UsageState::Loading)),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: true,
            allow_stats_cli: true,
            is_opencode: true,
            page: UsagePage::Main,
            stats_state: Arc::new(Mutex::new(StatsState::Idle)),
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
        opencode_api::local_opencode_summary(model, last_turn, accumulated)
    }

    pub fn offline_fallback() -> Self {
        let summary = UsageSummary {
            plan: UsagePlanInfo {
                tier_name: "Google AI Pro".to_string(),
                account_email: None,
                upgrade_url: None,
                manage_url: None,
            },
            groups: vec![
                QuotaGroup {
                    name: "GEMINI MODELS".to_string(),
                    description: Some(
                        "Models within this group: Gemini Flash, Gemini Pro".to_string(),
                    ),
                    buckets: vec![
                        QuotaBucket {
                            name: "Weekly Limit Remaining".to_string(),
                            remaining_pct: 100.0,
                            reset_label: "Quota available".to_string(),
                            exhausted: false,
                        },
                        QuotaBucket {
                            name: "Five Hour Limit Remaining".to_string(),
                            remaining_pct: 100.0,
                            reset_label: "Quota available".to_string(),
                            exhausted: false,
                        },
                    ],
                },
                QuotaGroup {
                    name: "CLAUDE AND GPT MODELS".to_string(),
                    description: Some(
                        "Models within this group: Claude Opus, Claude Sonnet, GPT-OSS".to_string(),
                    ),
                    buckets: vec![
                        QuotaBucket {
                            name: "Weekly Limit Remaining".to_string(),
                            remaining_pct: 100.0,
                            reset_label: "Quota available".to_string(),
                            exhausted: false,
                        },
                        QuotaBucket {
                            name: "Five Hour Limit Remaining".to_string(),
                            remaining_pct: 100.0,
                            reset_label: "Quota available".to_string(),
                            exhausted: false,
                        },
                    ],
                },
                QuotaGroup {
                    name: "GEMINI 4 ARGON".to_string(),
                    description: Some("For paid Pro plans, Argon quota is separate.".to_string()),
                    buckets: vec![QuotaBucket {
                        name: "Gemini 4 Argon Weekly Limit".to_string(),
                        remaining_pct: 100.0,
                        reset_label: "Quota available".to_string(),
                        exhausted: false,
                    }],
                },
            ],
        };

        Self::with_data(summary)
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self
            .scroll_offset
            .saturating_sub(n)
            .min(self.max_scroll.get());
    }

    pub fn scroll_down(&mut self, n: usize) {
        let limit = self.max_scroll.get();
        self.scroll_offset = (self.scroll_offset + n).min(limit);
    }

    pub fn refresh(&mut self) {
        if self.page == UsagePage::OpenCodeStats {
            self.refresh_stats();
            return;
        }
        if !self.allow_network || self.is_loading() {
            return;
        }
        if self.is_opencode {
            {
                let mut cache = OPENCODE_USAGE_CACHE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                *cache = None;
            }
            let fallback = self
                .opencode_fallback
                .clone()
                .unwrap_or_else(|| Self::local_opencode_summary("", None, Default::default()));
            let (tx, rx) = channel();
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = UsageState::Loading;
            self.rx = Some(Arc::new(Mutex::new(rx)));
            opencode_api::fetch_async(fallback, tx);
            return;
        }
        {
            let mut cache = USAGE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            *cache = None;
        }
        let (tx, rx) = channel();
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = UsageState::Loading;
        self.rx = Some(Arc::new(Mutex::new(rx)));
        fetch::fetch_quota_async(tx);
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
        {
            let guard = OPENCODE_STATS_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((instant, ref stats)) = *guard {
                if instant.elapsed() < Duration::from_secs(60) {
                    *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) =
                        StatsState::Ready(stats.clone());
                    return;
                }
            }
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
        {
            let mut cache = OPENCODE_STATS_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cache = None;
        }
        self.start_stats_fetch();
    }

    fn start_stats_fetch(&mut self) {
        let (tx, rx) = channel();
        *self.stats_state.lock().unwrap_or_else(|e| e.into_inner()) = StatsState::Loading;
        self.stats_rx = Some(Arc::new(Mutex::new(rx)));
        stats::fetch_stats_async(tx);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => {
                if self.page == UsagePage::OpenCodeStats {
                    self.page = UsagePage::Main;
                    self.scroll_offset = 0;
                    self.max_scroll.set(0);
                    ModalAction::None
                } else {
                    ModalAction::Close
                }
            }
            KeyCode::Enter => {
                if self.is_opencode && self.page == UsagePage::Main {
                    self.open_stats_subpage();
                    ModalAction::None
                } else if self.page == UsagePage::OpenCodeStats {
                    ModalAction::None
                } else {
                    ModalAction::Close
                }
            }
            KeyCode::Char('q') => ModalAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                ModalAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
                ModalAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(5);
                ModalAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(5);
                ModalAction::None
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                ModalAction::None
            }
            KeyCode::End => {
                self.scroll_offset = self.max_scroll.get();
                ModalAction::None
            }
            KeyCode::Char('r') => {
                self.refresh();
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> ModalAction {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(2);
                ModalAction::None
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(2);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
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
            if let Ok(new_state) = rx.try_recv() {
                let mut current = self.state.lock().unwrap_or_else(|e| e.into_inner());
                *current = new_state;
                updated = true;
            }
        }
        if let Some(ref rx_lock) = self.stats_rx {
            let rx = rx_lock.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(new_stats) = rx.try_recv() {
                let mut current = self.stats_state.lock().unwrap_or_else(|e| e.into_inner());
                *current = new_stats;
                updated = true;
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
