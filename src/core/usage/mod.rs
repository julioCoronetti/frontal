pub mod opencode_parse;
pub mod parse;

pub use opencode_parse::{
    format_reset_epoch, parse_credential_value, parse_go_usage, parse_iso_ms,
    parse_opencode_stats_json, strip_ansi, summary_from_go, GoUsage, GoWindow,
};
pub use parse::{
    build_usage_summary, civil_to_days, clean_text, format_reset_time, is_valid_calendar,
    parse_available_models, parse_iso_ymd_hms, parse_plan_info, parse_quota_summary_groups,
};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UsagePage {
    #[default]
    Main,
    OpenCodeStats,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCodeModelStat {
    pub id: String,
    pub provider_id: String,
    pub variant: Option<String>,
    pub steps: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCodeToolStat {
    pub name: String,
    pub calls: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub duration_p50_ms: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenCodeStats {
    pub sessions: u64,
    pub subagents: u64,
    pub prompts: u64,
    pub steps: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost: f64,
    pub active_days: u64,
    pub streak: u64,
    pub tool_calls: u64,
    pub tool_succeeded: u64,
    pub tool_failed: u64,
    pub tool_unfinished: u64,
    pub models: Vec<OpenCodeModelStat>,
    pub tools: Vec<OpenCodeToolStat>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatsState {
    Idle,
    Loading,
    Ready(OpenCodeStats),
    Unavailable(String),
}

pub fn offline_summary() -> UsageSummary {
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
