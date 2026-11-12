use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
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
        let percent = window.get("percent").and_then(|p| p.as_f64())?;
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
    let mut parts = iso.split('T');
    let date = parts.next()?;
    let time = parts.next()?.trim_end_matches('Z');
    let date: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    let time: Vec<i64> = time
        .split(':')
        .filter_map(|p| p.split('.').next()?.parse().ok())
        .collect();
    if date.len() < 3 || time.len() < 2 {
        return None;
    }
    let seconds = civil_to_days(date[0], date[1] as u64, date[2] as u64) * 86_400
        + time[0] * 3_600
        + time[1] * 60
        + time.get(2).copied().unwrap_or(0);
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
    let bucket = |name: &str, window: &GoWindow| QuotaBucket {
        name: name.to_string(),
        remaining_pct: (100.0 - window.percent).clamp(0.0, 100.0),
        reset_label: format!(
            "{:.0}% used · {}",
            window.percent,
            format_reset_epoch(window.resets_at_ms)
        ),
        exhausted: window.percent >= 100.0,
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
            description: Some(
                "5-hour 20% · weekly 50% · monthly 100% of each model allowance".to_string(),
            ),
            buckets: vec![
                bucket("5-hour window", &usage.rolling),
                bucket("Weekly window", &usage.weekly),
                bucket("Monthly window", &usage.monthly),
            ],
        }],
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

pub fn fetch_async(token: String, fallback: UsageSummary, tx: Sender<UsageState>) {
    std::thread::spawn(move || {
        let state = match fetch_go_usage(&token) {
            Ok(usage) => UsageState::Ready(summary_from_go(&usage)),
            Err(_) => UsageState::Ready(fallback),
        };
        if let UsageState::Ready(ref summary) = state {
            if let Ok(mut cache) = super::OPENCODE_USAGE_CACHE.lock() {
                *cache = Some((std::time::Instant::now(), summary.clone()));
            }
        }
        let _ = tx.send(state);
    });
}

fn http_get(url: &str, token: &str) -> Result<String, String> {
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--connect-timeout",
            "3",
            "--max-time",
            "6",
            "--config",
            "-",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(stdin) = child.stdin.as_mut() {
        let config = format!("header = \"Authorization: Bearer {}\"\n", token.trim());
        stdin
            .write_all(config.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("usage request failed".to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
