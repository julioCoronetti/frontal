use std::sync::Mutex;
use std::time::Instant;

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

pub(super) fn offline_summary() -> UsageSummary {
    UsageSummary {
        plan: UsagePlanInfo {
            tier_name: "Google AI Pro".to_string(),
            account_email: None,
            upgrade_url: None,
            manage_url: None,
        },
        groups: vec![
            QuotaGroup {
                name: "GEMINI MODELS".to_string(),
                description: Some("Models within this group: Gemini Flash, Gemini Pro".to_string()),
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
    }
}
