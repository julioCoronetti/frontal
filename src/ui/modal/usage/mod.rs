pub mod cli;
pub mod fetch;
pub mod opencode_api;
pub mod render;

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::Frame;

pub use fetch::format_reset_time;

use crate::ui::modal::ModalAction;

pub static USAGE_CACHE: Mutex<Option<(Instant, UsageSummary)>> = Mutex::new(None);
pub static OPENCODE_USAGE_CACHE: Mutex<Option<(Instant, UsageSummary)>> = Mutex::new(None);
pub static FETCH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

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
    rx: Option<Arc<Mutex<Receiver<UsageState>>>>,
}

impl UsageModal {
    pub fn load(allow_network: bool) -> Self {
        if !allow_network {
            return Self::offline_fallback();
        }

        if let Ok(guard) = USAGE_CACHE.lock() {
            if let Some((instant, ref summary)) = *guard {
                if instant.elapsed() < Duration::from_secs(60) {
                    return Self::with_data(summary.clone());
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
            rx: Some(Arc::new(Mutex::new(rx))),
        }
    }

    pub fn with_data(summary: UsageSummary) -> Self {
        Self {
            state: Arc::new(Mutex::new(UsageState::Ready(summary))),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: false,
            rx: None,
        }
    }

    pub fn load_opencode(
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> Self {
        let Some(token) = opencode_api::resolve_go_token() else {
            return Self::for_opencode(model, last_turn, accumulated);
        };
        if let Ok(guard) = OPENCODE_USAGE_CACHE.lock() {
            if let Some((instant, ref summary)) = *guard {
                if instant.elapsed() < Duration::from_secs(60) {
                    return Self::with_data(summary.clone());
                }
            }
        }
        let fallback = Self::local_opencode_summary(model, last_turn, accumulated);
        let (tx, rx) = channel();
        opencode_api::fetch_async(token, fallback, tx);
        Self {
            state: Arc::new(Mutex::new(UsageState::Loading)),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            allow_network: true,
            rx: Some(Arc::new(Mutex::new(rx))),
        }
    }

    pub fn for_opencode(
        model: &str,
        last_turn: Option<crate::protocol::TokenUsage>,
        accumulated: crate::protocol::TokenUsage,
    ) -> Self {
        Self::with_data(Self::local_opencode_summary(model, last_turn, accumulated))
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
        let summary = UsageSummary {
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
        };
        summary
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
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let limit = self.max_scroll.get();
        self.scroll_offset = (self.scroll_offset + n).min(limit);
    }

    pub fn refresh(&mut self) {
        if !self.allow_network || self.is_loading() {
            return;
        }
        if let Ok(mut cache) = USAGE_CACHE.lock() {
            *cache = None;
        }
        let (tx, rx) = channel();
        *self.state.lock().unwrap() = UsageState::Loading;
        self.rx = Some(Arc::new(Mutex::new(rx)));
        fetch::fetch_quota_async(tx);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => ModalAction::Close,
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
        if let Ok(st) = self.state.lock() {
            matches!(*st, UsageState::Loading)
        } else {
            false
        }
    }

    pub fn poll_rx(&self) -> bool {
        if let Some(ref rx_lock) = self.rx {
            if let Ok(rx) = rx_lock.lock() {
                if let Ok(new_state) = rx.try_recv() {
                    if let UsageState::Ready(ref summary) = new_state {
                        if let Ok(mut cache) = USAGE_CACHE.lock() {
                            *cache = Some((Instant::now(), summary.clone()));
                        }
                    }
                    if let Ok(mut current) = self.state.lock() {
                        *current = new_state;
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        render::render_modal(self, frame, area);
    }
}
