use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};

use super::fetch::civil_to_days;
use super::{QuotaBucket, QuotaGroup, UsagePlanInfo, UsageState, UsageSummary};

const GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoWindow {
    pub percent: f64,
    pub resets_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoUsage {
    pub rolling: GoWindow,
    pub weekly: GoWindow,
    pub monthly: GoWindow,
}

pub fn parse_go_usage(json: &str) -> Option<GoUsage> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let usage = value.get("usage").unwrap_or(&value);
    let window = |name: &str| -> Option<GoWindow> {
        let window = usage.get(name)?;
        let percent = window
            .get("percent")
            .and_then(|p| p.as_f64())
            .filter(|p| p.is_finite())?;
        let resets_at_ms = window
            .get("resetsAt")
            .and_then(|value| value.as_str())
            .and_then(parse_iso_ms);
        Some(GoWindow {
            percent,
            resets_at_ms,
        })
    };
    Some(GoUsage {
        rolling: window("rolling")?,
        weekly: window("weekly")?,
        monthly: window("monthly")?,
    })
}

pub fn parse_iso_ms(iso: &str) -> Option<i64> {
    let (d, t) = super::fetch::parse_iso_ymd_hms(iso)?;
    let seconds =
        civil_to_days(d[0], d[1] as u64, d[2] as u64) * 86_400 + t[0] * 3_600 + t[1] * 60 + t[2];
    Some(seconds * 1000)
}

pub fn format_reset_epoch(resets_at_ms: Option<i64>) -> String {
    let Some(target_ms) = resets_at_ms else {
        return "Resets periodically".to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff = target_ms - now;
    if diff <= 0 {
        return "Quota available".to_string();
    }
    let minutes = diff / 60_000;
    let (days, hours, mins) = (minutes / 1_440, (minutes % 1_440) / 60, minutes % 60);
    if days > 0 {
        format!("Refreshes in {}d {}h", days, hours)
    } else if hours > 0 {
        format!("Refreshes in {}h {}m", hours, mins)
    } else {
        format!("Refreshes in {}m", mins.max(1))
    }
}

pub fn summary_from_go(usage: &GoUsage) -> UsageSummary {
    let bucket = |name: &str, window: &GoWindow| {
        let used_pct = if window.percent.is_finite() {
            window.percent.clamp(0.0, 100.0)
        } else {
            0.0
        };
        QuotaBucket {
            name: name.to_string(),
            remaining_pct: (100.0 - used_pct).clamp(0.0, 100.0),
            reset_label: format!(
                "{:.0}% used · {}",
                used_pct,
                format_reset_epoch(window.resets_at_ms)
            ),
            exhausted: used_pct >= 100.0,
        }
    };
    UsageSummary {
        plan: UsagePlanInfo {
            tier_name: "OpenCode Go".to_string(),
            account_email: None,
            upgrade_url: Some("https://opencode.ai/console".to_string()),
            manage_url: None,
        },
        groups: vec![QuotaGroup {
            name: "GO USAGE WINDOWS".to_string(),
            description: None,
            buckets: vec![
                bucket("5-hour window", &usage.rolling),
                bucket("Weekly window", &usage.weekly),
                bucket("Monthly window", &usage.monthly),
            ],
        }],
    }
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
            let remaining_pct = (100.0 - (used as f64 / window as f64) * 100.0).clamp(0.0, 100.0);
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

pub fn resolve_go_token() -> Option<String> {
    if let Ok(value) = std::env::var("OPENCODE_GO_API_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let data_dir = std::env::var("XDG_DATA_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    let db_path = data_dir.join("opencode").join("opencode.db");
    if !db_path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(200));
    let mut stmt = conn
        .prepare(
            "SELECT value FROM credential
             WHERE integration_id = 'opencode-go' AND active = 1 LIMIT 1",
        )
        .ok()?;
    let mut rows = stmt.query([]).ok()?;
    let row = rows.next().ok()??;
    let value: String = row.get(0).ok()?;
    parse_credential_value(&value)
}

pub fn parse_credential_value(value: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    parsed
        .get("key")
        .and_then(|key| key.as_str())
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

pub fn fetch_go_usage(token: &str) -> Result<GoUsage, String> {
    let body = http_get(GO_USAGE_URL, token)?;
    parse_go_usage(&body).ok_or_else(|| "unexpected usage payload".to_string())
}

struct OpenCodeFetchState {
    in_flight: bool,
    waiters: Vec<Sender<UsageState>>,
}

static OPENCODE_FETCH_STATE: Mutex<OpenCodeFetchState> = Mutex::new(OpenCodeFetchState {
    in_flight: false,
    waiters: Vec::new(),
});

struct OpenCodeFetchGuard(UsageSummary);

impl Drop for OpenCodeFetchGuard {
    fn drop(&mut self) {
        let waiters = {
            let mut state = OPENCODE_FETCH_STATE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !state.in_flight {
                return;
            }
            state.in_flight = false;
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            let _ = waiter.send(UsageState::Ready(self.0.clone()));
        }
    }
}

pub fn fetch_async(fallback: UsageSummary, tx: Sender<UsageState>) {
    {
        let mut state = OPENCODE_FETCH_STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.waiters.push(tx);
        if state.in_flight {
            return;
        }
        state.in_flight = true;
    }
    std::thread::spawn(move || {
        let _guard = OpenCodeFetchGuard(fallback.clone());
        let state = match resolve_go_token() {
            Some(token) => match fetch_go_usage(&token) {
                Ok(usage) => {
                    let summary = summary_from_go(&usage);
                    let mut cache = super::OPENCODE_USAGE_CACHE
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    *cache = Some((std::time::Instant::now(), summary.clone()));
                    UsageState::Ready(summary)
                }
                Err(_) => UsageState::Ready(fallback),
            },
            None => UsageState::Ready(fallback),
        };
        let waiters: Vec<Sender<UsageState>> = {
            let mut fetch_state = OPENCODE_FETCH_STATE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            fetch_state.in_flight = false;
            std::mem::take(&mut fetch_state.waiters)
        };
        for waiter in waiters {
            let _ = waiter.send(state.clone());
        }
    });
}

fn http_get(url: &str, token: &str) -> Result<String, String> {
    let clean = token.trim();
    if clean.is_empty()
        || clean
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\\'))
    {
        return Err("invalid credential token".to_string());
    }
    let config = format!("header = \"Authorization: Bearer {}\"\n", clean);
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "--connect-timeout",
        "3",
        "--max-time",
        "6",
        "--config",
        "-",
        url,
    ]);
    let output =
        super::cli::run_bounded_command(cmd, Some(config.into_bytes()), Duration::from_secs(7))?;
    if !output.status.success() {
        return Err("usage request failed".to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
