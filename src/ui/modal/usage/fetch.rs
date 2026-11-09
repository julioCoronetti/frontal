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

fn post_endpoint(
    url: &str,
    token: &str,
    user_agent: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut cfg = format!(
        "header = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        token.trim()
    );
    if let Some(ua) = user_agent {
        cfg.push_str(&format!("header = \"User-Agent: {}\"\n", ua));
    }

    let mut child = std::process::Command::new("curl")
        .args([
            "-sS",
            "--connect-timeout",
            "2",
            "--max-time",
            "4",
            "-X",
            "POST",
            url,
            "-d",
            "{}",
            "--config",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl error: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(cfg.as_bytes());
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("network error: {}", e))?;

    if !output.status.success() {
        return Err("request failed".to_string());
    }

    let val: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("parse error: {}", e))?;

    if val.get("error").is_some() {
        return Err("api error".to_string());
    }

    Ok(val)
}

fn execute_fetch(token: &str) -> UsageState {
    let (plan_res, models_res) = std::thread::scope(|s| {
        let plan_handle = s.spawn(|| {
            post_endpoint(
                "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
                token,
                Some("Antigravity/1.5.3"),
            )
        });
        let models_handle = s.spawn(|| {
            post_endpoint(
                "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
                token,
                Some("Antigravity/1.5.3"),
            )
        });
        (
            plan_handle
                .join()
                .unwrap_or(Err("plan thread failed".to_string())),
            models_handle
                .join()
                .unwrap_or(Err("models thread failed".to_string())),
        )
    });

    let models_val = match models_res {
        Ok(v) => v,
        Err(_) => return UsageState::Unavailable("Failed to fetch model quota".to_string()),
    };

    let plan_val = plan_res.ok();
    match build_usage_summary(plan_val.as_ref(), &models_val) {
        Some(summary) => UsageState::Ready(summary),
        None => UsageState::Unavailable("Model quota details unavailable".to_string()),
    }
}

pub fn parse_plan_info(val: Option<&serde_json::Value>) -> UsagePlanInfo {
    let default_plan = UsagePlanInfo {
        tier_name: "Antigravity Subscriber".to_string(),
        upgrade_url: Some("https://cloud.google.com/gemini/pricing".to_string()),
        manage_url: None,
    };

    let val = match val {
        Some(v) => v,
        None => return default_plan,
    };

    let tier_name = if let Some(tier) = val.get("currentTier").and_then(|t| t.get("name")) {
        tier.as_str()
            .unwrap_or("Antigravity Subscriber")
            .to_string()
    } else if let Some(first_tier) = val
        .get("allowedTiers")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("name"))
    {
        first_tier
            .as_str()
            .unwrap_or("Antigravity Subscriber")
            .to_string()
    } else {
        "Antigravity Subscriber".to_string()
    };

    let upgrade_url = val
        .get("currentTier")
        .and_then(|t| t.get("upgradeSubscriptionUri"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            val.get("upgradeSubscriptionUri")
                .and_then(|u| u.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| Some("https://cloud.google.com/gemini/pricing".to_string()));

    let manage_url = val
        .get("manageSubscriptionUri")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

    UsagePlanInfo {
        tier_name,
        upgrade_url,
        manage_url,
    }
}

#[derive(Clone, Debug)]
struct RawQuota {
    id: String,
    name: String,
    provider: String,
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
    disabled: bool,
}

fn extract_raw_quotas(val: &serde_json::Value) -> Vec<RawQuota> {
    let mut results = Vec::new();

    let parse_entry = |key: &str, item: &serde_json::Value| -> Option<RawQuota> {
        let name = item
            .get("displayName")
            .and_then(|d| d.as_str())
            .unwrap_or(key)
            .to_string();
        let provider = item
            .get("modelProvider")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();

        let quota_val = item.get("quotaInfo").unwrap_or(item);
        let remaining_fraction = quota_val.get("remainingFraction").and_then(|f| f.as_f64());
        let reset_time = quota_val
            .get("resetTime")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        let disabled = item
            .get("disabled")
            .and_then(|d| d.as_bool())
            .unwrap_or(false)
            || quota_val
                .get("isExhausted")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);

        Some(RawQuota {
            id: key.to_string(),
            name,
            provider,
            remaining_fraction,
            reset_time,
            disabled,
        })
    };

    if let Some(obj) = val.get("models").and_then(|m| m.as_object()) {
        for (k, v) in obj {
            if let Some(q) = parse_entry(k, v) {
                results.push(q);
            }
        }
    } else if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
        for v in arr {
            let key = v
                .get("model")
                .or_else(|| v.get("id"))
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if let Some(q) = parse_entry(key, v) {
                results.push(q);
            }
        }
    }

    results
}

fn make_bucket(name: &str, raw: Option<&RawQuota>) -> QuotaBucket {
    match raw {
        Some(r) => {
            let frac = r
                .remaining_fraction
                .unwrap_or(if r.disabled { 0.0 } else { 1.0 });
            let exhausted = r.disabled || frac <= 0.0;
            let reset_label = if let Some(ref rt) = r.reset_time {
                format_reset_time(rt)
            } else if exhausted {
                "Quota exhausted".to_string()
            } else {
                "Quota available".to_string()
            };
            QuotaBucket {
                name: name.to_string(),
                remaining_pct: (frac * 100.0).clamp(0.0, 100.0),
                reset_label,
                exhausted,
            }
        }
        None => QuotaBucket {
            name: name.to_string(),
            remaining_pct: 100.0,
            reset_label: "Quota available".to_string(),
            exhausted: false,
        },
    }
}

pub fn parse_available_models(val: &serde_json::Value) -> Option<Vec<QuotaGroup>> {
    let raws = extract_raw_quotas(val);
    if raws.is_empty() {
        return None;
    }

    let flash_raw = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("flash") && (r.provider.contains("GOOGLE") || s.contains("gemini"))
    });

    let argon_raw = raws
        .iter()
        .find(|r| {
            let s = format!("{} {}", r.id, r.name).to_lowercase();
            s.contains("argon")
        })
        .or_else(|| {
            raws.iter().find(|r| {
                let s = format!("{} {}", r.id, r.name).to_lowercase();
                s.contains("pro") && (r.provider.contains("GOOGLE") || s.contains("gemini"))
            })
        })
        .or(flash_raw);

    let sonnet_raw = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("sonnet")
    });

    let opus_raw = raws
        .iter()
        .find(|r| {
            let s = format!("{} {}", r.id, r.name).to_lowercase();
            s.contains("opus")
        })
        .or(sonnet_raw);

    let sonnet_raw = sonnet_raw.or(opus_raw);

    let gpt_raw = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("gpt-oss") || s.contains("120b") || r.provider.contains("OPENAI")
    });

    let gemini_group = QuotaGroup {
        name: "Gemini Models".to_string(),
        description: Some("5 hours limit".to_string()),
        buckets: vec![
            make_bucket("Gemini 3.8 Flash", flash_raw),
            make_bucket("Gemini 4 Argon", argon_raw),
        ],
    };

    let claude_group = QuotaGroup {
        name: "Claude Models".to_string(),
        description: Some("5 hours limit".to_string()),
        buckets: vec![
            make_bucket("Claude Sonnet 5.5", sonnet_raw),
            make_bucket("Claude Opus 5.5", opus_raw),
        ],
    };

    let other_group = QuotaGroup {
        name: "Other Models".to_string(),
        description: Some("5 hours limit".to_string()),
        buckets: vec![make_bucket("GPT-OSS 120B", gpt_raw)],
    };

    Some(vec![gemini_group, claude_group, other_group])
}

pub fn build_usage_summary(
    plan_json: Option<&serde_json::Value>,
    models_json: &serde_json::Value,
) -> Option<UsageSummary> {
    let groups = parse_available_models(models_json)?;
    let plan = parse_plan_info(plan_json);

    Some(UsageSummary {
        plan,
        groups,
        credits: Some(CreditInfo {
            available: Some("$0.00".to_string()),
            overages: Some("$0.00".to_string()),
        }),
    })
}
