use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{QuotaBucket, QuotaGroup, UsagePlanInfo, UsageState, UsageSummary};

fn civil_to_days(mut y: i64, m: u64, d: u64) -> i64 {
    y -= (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) as i64 - 719468
}

pub fn format_reset_time(iso: &str) -> String {
    let mut parts = iso.split('T');
    let (d_str, t_str) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("").trim_end_matches('Z'),
    );
    let d: Vec<i64> = d_str.split('-').filter_map(|s| s.parse().ok()).collect();
    let t: Vec<i64> = t_str
        .split(':')
        .filter_map(|s| s.split('.').next()?.parse().ok())
        .collect();
    if d.len() < 3 || t.len() < 2 {
        return "Resets periodically".to_string();
    }
    let target = civil_to_days(d[0], d[1] as u64, d[2] as u64) * 86400
        + t[0] * 3600
        + t[1] * 60
        + t.get(2).copied().unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64)
        .unwrap_or(0);
    let diff = target - now;
    if diff <= 0 {
        return "Quota available".to_string();
    }
    let (h, m) = (diff / 3600, (diff % 3600) / 60);
    if h > 0 {
        format!("Refreshes in {}h {}m", h, m)
    } else {
        format!("Refreshes in {}m", m.max(1))
    }
}

pub fn fetch_quota_async(tx: Sender<UsageState>) {
    if super::FETCH_IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let state = execute_fetch();
        super::FETCH_IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = tx.send(state);
    });
}

fn execute_fetch() -> UsageState {
    if let Ok(summary) = super::cli::fetch_agy_cli_quota() {
        return UsageState::Ready(summary);
    }
    execute_http_fallback()
}

fn execute_http_fallback() -> UsageState {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let path = format!("{}/.gemini/antigravity-cli/antigravity-oauth-token", home);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            return UsageState::Unavailable(
                "OAuth credentials not found. Log in via agy.".to_string(),
            );
        }
    };
    let token_json: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return UsageState::Unavailable("Invalid OAuth credentials format.".to_string()),
    };
    let Some(access_token) = token_json
        .pointer("/token/access_token")
        .and_then(|a| a.as_str())
    else {
        return UsageState::Unavailable("No access token found in credentials.".to_string());
    };
    let email = token_json
        .get("id_token_email")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string());
    execute_http_fetch(access_token, email)
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
    let out = child
        .wait_with_output()
        .map_err(|e| format!("network error: {}", e))?;
    if !out.status.success() {
        return Err("request failed".to_string());
    }
    let val: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("parse error: {}", e))?;
    if val.get("error").is_some() {
        return Err("api error".to_string());
    }
    Ok(val)
}

fn execute_http_fetch(token: &str, email: Option<String>) -> UsageState {
    let ep = |url: &'static str| post_endpoint(url, token, Some("Antigravity/1.5.3"));
    let (p_res, q_res, m_res) = std::thread::scope(|s| {
        let p =
            s.spawn(|| ep("https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"));
        let q = s.spawn(|| {
            ep("https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary")
        });
        let m = s.spawn(|| {
            ep("https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels")
        });
        (
            p.join().unwrap_or(Err("err".to_string())),
            q.join().unwrap_or(Err("err".to_string())),
            m.join().unwrap_or(Err("err".to_string())),
        )
    });
    let models_val = match m_res {
        Ok(v) => v,
        Err(_) => return UsageState::Unavailable("Failed to fetch model quota".to_string()),
    };
    match build_usage_summary(p_res.ok().as_ref(), q_res.ok().as_ref(), &models_val, email) {
        Some(summary) => UsageState::Ready(summary),
        None => UsageState::Unavailable("Model quota details unavailable".to_string()),
    }
}

pub fn parse_plan_info(val: Option<&serde_json::Value>, email: Option<String>) -> UsagePlanInfo {
    let default_plan = UsagePlanInfo {
        tier_name: "Google AI Pro".to_string(),
        account_email: email.clone(),
        upgrade_url: None,
        manage_url: None,
    };
    let Some(val) = val else { return default_plan };
    let tier_name = val
        .pointer("/paidTier/name")
        .or_else(|| val.pointer("/currentTier/name"))
        .or_else(|| val.pointer("/allowedTiers/0/name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Google AI Pro")
        .to_string();
    let manage_url = val
        .get("manageSubscriptionUri")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    UsagePlanInfo {
        tier_name,
        account_email: email,
        upgrade_url: None,
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
        let q_val = item.get("quotaInfo").unwrap_or(item);
        let remaining_fraction = q_val.get("remainingFraction").and_then(|f| f.as_f64());
        let reset_time = q_val
            .get("resetTime")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        let disabled = item
            .get("disabled")
            .and_then(|d| d.as_bool())
            .unwrap_or(false)
            || q_val
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
        results.extend(obj.iter().filter_map(|(k, v)| parse_entry(k, v)));
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
            let reset_label = if frac >= 1.0 {
                "Quota available".to_string()
            } else if let Some(ref rt) = r.reset_time {
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

    let flash = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("flash") && (r.provider.contains("GOOGLE") || s.contains("gemini"))
    });
    let pro = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("pro") && (r.provider.contains("GOOGLE") || s.contains("gemini"))
    });
    let argon = raws.iter().find(|r| {
        format!("{} {}", r.id, r.name)
            .to_lowercase()
            .contains("argon")
    });
    let sonnet = raws.iter().find(|r| {
        format!("{} {}", r.id, r.name)
            .to_lowercase()
            .contains("sonnet")
    });
    let opus = raws
        .iter()
        .find(|r| {
            format!("{} {}", r.id, r.name)
                .to_lowercase()
                .contains("opus")
        })
        .or(sonnet);
    let sonnet = sonnet.or(opus);
    let gpt = raws.iter().find(|r| {
        let s = format!("{} {}", r.id, r.name).to_lowercase();
        s.contains("gpt-oss") || s.contains("120b") || r.provider.contains("OPENAI")
    });

    let gemini_weekly = match pro.or(flash) {
        Some(r) if r.remaining_fraction.unwrap_or(1.0) < 1.0 => {
            make_bucket("Weekly Limit Remaining", Some(r))
        }
        _ => make_bucket("Weekly Limit Remaining", None),
    };
    let gemini_five = make_bucket("Five Hour Limit Remaining", flash);
    let gemini_group = QuotaGroup {
        name: "GEMINI MODELS".to_string(),
        description: Some("Models within this group: Gemini Flash, Gemini Pro".to_string()),
        buckets: vec![gemini_weekly, gemini_five],
    };

    let claude_weekly = make_bucket("Weekly Limit Remaining", sonnet);
    let claude_five = match gpt.or(opus) {
        Some(r) => make_bucket("Five Hour Limit Remaining", Some(r)),
        None => make_bucket("Five Hour Limit Remaining", None),
    };
    let claude_group = QuotaGroup {
        name: "CLAUDE AND GPT MODELS".to_string(),
        description: Some(
            "Models within this group: Claude Opus, Claude Sonnet, GPT-OSS".to_string(),
        ),
        buckets: vec![claude_weekly, claude_five],
    };

    let argon_b = match argon {
        Some(r) => make_bucket("Gemini 4 Argon Weekly Limit", Some(r)),
        None => QuotaBucket {
            name: "Gemini 4 Argon Weekly Limit".to_string(),
            remaining_pct: 100.0,
            reset_label: "Quota available (Weekly)".to_string(),
            exhausted: false,
        },
    };
    let argon_group = QuotaGroup {
        name: "GEMINI 4 ARGON".to_string(),
        description: Some("For paid Pro plans, Argon quota is separate. Higher Argon quota (included as part of the larger Gemini quota bucket) is available on Ultra plans.".to_string()),
        buckets: vec![argon_b],
    };

    Some(vec![gemini_group, claude_group, argon_group])
}

pub fn parse_quota_summary_groups(val: &serde_json::Value) -> Vec<QuotaGroup> {
    let arr = val
        .pointer("/command/data/groups")
        .or_else(|| val.pointer("/data/groups"))
        .or_else(|| val.get("groups"))
        .and_then(|g| g.as_array());
    let Some(arr) = arr else { return Vec::new() };
    arr.iter()
        .filter_map(|item| {
            let name = item
                .get("displayName")
                .or_else(|| item.get("name"))
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_uppercase();
            let description = item
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            let buckets: Vec<QuotaBucket> = item
                .get("buckets")
                .and_then(|b| b.as_array())?
                .iter()
                .filter_map(|b| {
                    let b_name = b
                        .get("displayName")
                        .or_else(|| b.get("name"))
                        .and_then(|d| d.as_str())?
                        .to_string();
                    let frac = b
                        .get("remainingFraction")
                        .or_else(|| b.get("remaining_fraction"))
                        .and_then(|f| f.as_f64())
                        .unwrap_or(1.0);
                    let exhausted = frac <= 0.0
                        || b.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false)
                        || b.get("isExhausted")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                    let reset_label = if frac >= 1.0 {
                        "Quota available".to_string()
                    } else if let Some(rt) = b
                        .get("resetTime")
                        .or_else(|| b.get("reset_time"))
                        .and_then(|s| s.as_str())
                    {
                        format_reset_time(rt)
                    } else if exhausted {
                        "Quota exhausted".to_string()
                    } else {
                        "Quota available".to_string()
                    };
                    Some(QuotaBucket {
                        name: b_name,
                        remaining_pct: (frac * 100.0).clamp(0.0, 100.0),
                        reset_label,
                        exhausted,
                    })
                })
                .collect();
            if buckets.is_empty() {
                None
            } else {
                Some(QuotaGroup {
                    name,
                    description,
                    buckets,
                })
            }
        })
        .collect()
}

fn merge_quota_groups(api_groups: Vec<QuotaGroup>, fallback: &mut [QuotaGroup]) -> Vec<QuotaGroup> {
    fallback
        .iter_mut()
        .map(|fb| {
            api_groups
                .iter()
                .find(|ag| ag.name.eq_ignore_ascii_case(&fb.name))
                .cloned()
                .unwrap_or_else(|| fb.clone())
        })
        .collect()
}

pub fn build_usage_summary(
    plan_json: Option<&serde_json::Value>,
    quota_json: Option<&serde_json::Value>,
    models_json: &serde_json::Value,
    email: Option<String>,
) -> Option<UsageSummary> {
    let mut fallback_groups = parse_available_models(models_json)?;
    let api_groups = quota_json
        .map(parse_quota_summary_groups)
        .unwrap_or_default();
    let groups = merge_quota_groups(api_groups, &mut fallback_groups);
    let plan = parse_plan_info(plan_json, email);
    Some(UsageSummary { plan, groups })
}
