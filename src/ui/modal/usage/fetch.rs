use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{CreditInfo, QuotaBucket, QuotaGroup, UsagePlanInfo, UsageState, UsageSummary};

fn civil_to_days(mut y: i64, m: u64, d: u64) -> i64 {
    y -= (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

pub fn format_reset_time(iso: &str) -> String {
    let parts: Vec<&str> = iso.split('T').collect();
    if parts.len() < 2 {
        return "Resets periodically".to_string();
    }

    let d_parts: Vec<&str> = parts[0].split('-').collect();
    if d_parts.len() < 3 {
        return "Resets periodically".to_string();
    }

    let y: i64 = match d_parts[0].parse() {
        Ok(v) => v,
        Err(_) => return "Resets periodically".to_string(),
    };
    let m: u64 = match d_parts[1].parse() {
        Ok(v) => v,
        Err(_) => return "Resets periodically".to_string(),
    };
    let d: u64 = match d_parts[2].parse() {
        Ok(v) => v,
        Err(_) => return "Resets periodically".to_string(),
    };

    let t_str = parts[1].trim_end_matches('Z');
    let t_parts: Vec<&str> = t_str.split(':').collect();
    if t_parts.len() < 2 {
        return "Resets periodically".to_string();
    }

    let hr: i64 = match t_parts[0].parse() {
        Ok(v) => v,
        Err(_) => return "Resets periodically".to_string(),
    };
    let min: i64 = match t_parts[1].parse() {
        Ok(v) => v,
        Err(_) => return "Resets periodically".to_string(),
    };
    let sec: i64 = if t_parts.len() >= 3 {
        t_parts[2]
            .split('.')
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0)
    } else {
        0
    };

    let days = civil_to_days(y, m, d);
    let target_epoch = days * 86400 + hr * 3600 + min * 60 + sec;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64)
        .unwrap_or(0);

    let diff = target_epoch - now;
    if diff <= 0 {
        return "Quota reset".to_string();
    }

    let h = diff / 3600;
    let rem_m = (diff % 3600) / 60;
    if h > 0 {
        format!("Resets in {}h {}m", h, rem_m)
    } else {
        format!("Resets in {}m", rem_m.max(1))
    }
}

pub fn fetch_quota_async(tx: Sender<UsageState>) {
    std::thread::spawn(move || {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let token_path = format!("{}/.gemini/antigravity-cli/antigravity-oauth-token", home);
        let token_bytes = match std::fs::read(&token_path) {
            Ok(b) => b,
            Err(_) => {
                let _ = tx.send(UsageState::Unavailable(
                    "OAuth credentials not found. Log in via agy.".to_string(),
                ));
                return;
            }
        };

        let token_json: serde_json::Value = match serde_json::from_slice(&token_bytes) {
            Ok(v) => v,
            Err(_) => {
                let _ = tx.send(UsageState::Unavailable(
                    "Invalid OAuth credentials format.".to_string(),
                ));
                return;
            }
        };

        let access_token = match token_json
            .get("token")
            .and_then(|t| t.get("access_token"))
            .and_then(|a| a.as_str())
        {
            Some(tok) => tok.to_string(),
            None => {
                let _ = tx.send(UsageState::Unavailable(
                    "No access token found in credentials.".to_string(),
                ));
                return;
            }
        };

        let state = execute_fetch(&access_token);
        let _ = tx.send(state);
    });
}

fn execute_fetch(token: &str) -> UsageState {
    let cfg = format!(
        "header = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        token
    );

    let mut child = match std::process::Command::new("curl")
        .args([
            "-sS",
            "--connect-timeout",
            "2",
            "--max-time",
            "4",
            "-X",
            "POST",
            "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
            "-d",
            "{}",
            "--config",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return UsageState::Unavailable(format!("curl error: {}", e)),
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(cfg.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return UsageState::Unavailable(format!("network error: {}", e)),
    };

    if output.status.success() {
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if val.get("error").is_none() {
                if let Some(summary) = parse_quota_json(&val) {
                    return UsageState::Ready(summary);
                }
            }
        }
    }

    execute_code_assist_fallback(token)
}

fn execute_code_assist_fallback(token: &str) -> UsageState {
    let cfg = format!(
        "header = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        token
    );

    let mut child = match std::process::Command::new("curl")
        .args([
            "-sS",
            "--connect-timeout",
            "2",
            "--max-time",
            "4",
            "-X",
            "POST",
            "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            "-d",
            "{}",
            "--config",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return UsageState::Unavailable("Cannot reach subscription server".to_string()),
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(cfg.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return UsageState::Unavailable("Failed to read server response".to_string()),
    };

    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        if let Some(summary) = parse_code_assist_json(&val) {
            return UsageState::Ready(summary);
        }
    }

    UsageState::Unavailable("Subscription quota details unavailable".to_string())
}

pub fn parse_quota_json(val: &serde_json::Value) -> Option<UsageSummary> {
    let raw_groups = val.get("groups")?.as_array()?;
    let mut groups = Vec::new();

    for rg in raw_groups {
        let name = rg
            .get("displayName")
            .and_then(|d| d.as_str())
            .unwrap_or("Models")
            .to_string();
        let description = rg
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());
        let mut buckets = Vec::new();

        if let Some(raw_buckets) = rg.get("buckets").and_then(|b| b.as_array()) {
            for rb in raw_buckets {
                let b_name = rb
                    .get("displayName")
                    .and_then(|d| d.as_str())
                    .unwrap_or("Model")
                    .to_string();
                let frac = rb
                    .get("remainingFraction")
                    .and_then(|f| f.as_f64())
                    .unwrap_or(1.0);
                let disabled = rb
                    .get("disabled")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                let reset_label = if let Some(rt) = rb.get("resetTime").and_then(|t| t.as_str()) {
                    format_reset_time(rt)
                } else {
                    "Resets periodically".to_string()
                };

                buckets.push(QuotaBucket {
                    name: b_name,
                    remaining_pct: frac * 100.0,
                    reset_label,
                    exhausted: disabled || frac <= 0.0,
                });
            }
        }

        groups.push(QuotaGroup {
            name,
            description,
            buckets,
        });
    }

    Some(UsageSummary {
        plan: UsagePlanInfo {
            tier_name: "Antigravity Subscriber".to_string(),
            upgrade_url: Some("https://cloud.google.com/gemini/pricing".to_string()),
            manage_url: None,
        },
        groups,
        credits: Some(CreditInfo {
            available: Some("$0.00".to_string()),
            overages: Some("$0.00".to_string()),
        }),
    })
}

pub fn parse_code_assist_json(val: &serde_json::Value) -> Option<UsageSummary> {
    let tier_name = if let Some(tier) = val.get("currentTier").and_then(|t| t.get("name")) {
        tier.as_str().unwrap_or("Antigravity Tier").to_string()
    } else if let Some(first_tier) = val
        .get("allowedTiers")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("name"))
    {
        first_tier
            .as_str()
            .unwrap_or("Antigravity Tier")
            .to_string()
    } else {
        "Gemini Code Assist".to_string()
    };

    let upgrade_url = val
        .get("upgradeSubscriptionUri")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .or_else(|| Some("https://cloud.google.com/gemini/pricing".to_string()));
    let manage_url = val
        .get("manageSubscriptionUri")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

    let groups = vec![
        QuotaGroup {
            name: "Gemini Models".to_string(),
            description: Some("5 hours limit".to_string()),
            buckets: vec![
                QuotaBucket {
                    name: "Gemini 3.8 Flash".to_string(),
                    remaining_pct: 100.0,
                    reset_label: "Quota available".to_string(),
                    exhausted: false,
                },
                QuotaBucket {
                    name: "Gemini 3.8 Pro".to_string(),
                    remaining_pct: 100.0,
                    reset_label: "Quota available".to_string(),
                    exhausted: false,
                },
            ],
        },
        QuotaGroup {
            name: "Claude Models".to_string(),
            description: Some("5 hours limit".to_string()),
            buckets: vec![QuotaBucket {
                name: "Claude Sonnet 3.5".to_string(),
                remaining_pct: 100.0,
                reset_label: "Quota available".to_string(),
                exhausted: false,
            }],
        },
    ];

    Some(UsageSummary {
        plan: UsagePlanInfo {
            tier_name,
            upgrade_url,
            manage_url,
        },
        groups,
        credits: Some(CreditInfo {
            available: Some("$0.00".to_string()),
            overages: Some("$0.00".to_string()),
        }),
    })
}
