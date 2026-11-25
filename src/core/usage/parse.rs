use std::time::{SystemTime, UNIX_EPOCH};

use super::{QuotaBucket, QuotaGroup, UsagePlanInfo, UsageSummary};

pub fn clean_text(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect()
}

pub fn is_valid_calendar(d: &[i64], t: &[i64]) -> bool {
    if d.len() < 3 || t.len() < 2 {
        return false;
    }
    let sec = t.get(2).copied().unwrap_or(0);
    (1970..=9999).contains(&d[0])
        && (1..=12).contains(&d[1])
        && (1..=31).contains(&d[2])
        && (0..=23).contains(&t[0])
        && (0..=59).contains(&t[1])
        && (0..=60).contains(&sec)
}

pub fn civil_to_days(mut y: i64, m: u64, d: u64) -> i64 {
    y -= (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) as i64 - 719468
}

pub fn parse_iso_ymd_hms(iso: &str) -> Option<([i64; 3], [i64; 3])> {
    let (d_str, rest) = iso.split_once('T')?;
    let t_str = rest.trim_end_matches('Z');
    let d_parts: Vec<&str> = d_str.split('-').collect();
    let t_parts: Vec<&str> = t_str.split(':').collect();
    if d_parts.len() != 3 || !(2..=3).contains(&t_parts.len()) {
        return None;
    }
    let d: Vec<i64> = d_parts.iter().filter_map(|s| s.parse().ok()).collect();
    let t: Vec<i64> = t_parts
        .iter()
        .filter_map(|s| s.split('.').next()?.parse().ok())
        .collect();
    if d.len() != 3 || t.len() != t_parts.len() || !is_valid_calendar(&d, &t) {
        return None;
    }
    Some((
        [d[0], d[1], d[2]],
        [t[0], t[1], t.get(2).copied().unwrap_or(0)],
    ))
}

pub fn format_reset_time(iso: &str) -> String {
    let Some((d, t)) = parse_iso_ymd_hms(iso) else {
        return "Resets periodically".to_string();
    };
    let target =
        civil_to_days(d[0], d[1] as u64, d[2] as u64) * 86400 + t[0] * 3600 + t[1] * 60 + t[2];
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

pub fn parse_plan_info(val: Option<&serde_json::Value>, email: Option<String>) -> UsagePlanInfo {
    let clean_email = email.map(|e| clean_text(&e));
    let default_plan = UsagePlanInfo {
        tier_name: "Google AI Pro".to_string(),
        account_email: clean_email.clone(),
        upgrade_url: None,
        manage_url: None,
    };
    let Some(val) = val else { return default_plan };
    let tier_name = val
        .pointer("/paidTier/name")
        .or_else(|| val.pointer("/currentTier/name"))
        .or_else(|| val.pointer("/allowedTiers/0/name"))
        .and_then(|n| n.as_str())
        .map(clean_text)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Google AI Pro".to_string());
    let manage_url = val
        .get("manageSubscriptionUri")
        .and_then(|m| m.as_str())
        .map(clean_text);
    UsagePlanInfo {
        tier_name,
        account_email: clean_email,
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
        let remaining_fraction = q_val
            .get("remainingFraction")
            .and_then(|f| f.as_f64())
            .filter(|f| f.is_finite());
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

fn build_quota_bucket(
    name: String,
    frac_opt: Option<f64>,
    disabled: bool,
    reset_time: Option<&str>,
) -> QuotaBucket {
    let default_frac = if disabled { 0.0 } else { 1.0 };
    let frac = frac_opt
        .filter(|f| f.is_finite())
        .unwrap_or(default_frac)
        .clamp(0.0, 1.0);
    let exhausted = disabled || frac <= 0.0;
    let reset_label = if frac >= 1.0 {
        "Quota available".to_string()
    } else if let Some(rt) = reset_time {
        format_reset_time(rt)
    } else if exhausted {
        "Quota exhausted".to_string()
    } else {
        "Quota available".to_string()
    };
    QuotaBucket {
        name,
        remaining_pct: (frac * 100.0).clamp(0.0, 100.0),
        reset_label,
        exhausted,
    }
}

fn make_bucket(name: &str, raw: Option<&RawQuota>) -> QuotaBucket {
    match raw {
        Some(r) => build_quota_bucket(
            name.to_string(),
            r.remaining_fraction,
            r.disabled,
            r.reset_time.as_deref(),
        ),
        None => build_quota_bucket(name.to_string(), Some(1.0), false, None),
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
    let claude_five = make_bucket("Five Hour Limit Remaining", gpt.or(opus));
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
        description: Some("For paid Pro plans, Argon quota is separate.".to_string()),
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
            let name = clean_text(
                item.get("displayName")
                    .or_else(|| item.get("name"))
                    .and_then(|d| d.as_str())
                    .unwrap_or(""),
            )
            .to_uppercase();
            let description = item
                .get("description")
                .and_then(|d| d.as_str())
                .map(clean_text);
            let buckets: Vec<QuotaBucket> = item
                .get("buckets")
                .and_then(|b| b.as_array())?
                .iter()
                .filter_map(|b| {
                    let b_name = clean_text(
                        b.get("displayName")
                            .or_else(|| b.get("name"))
                            .and_then(|d| d.as_str())?,
                    );
                    let frac = b
                        .get("remainingFraction")
                        .or_else(|| b.get("remaining_fraction"))
                        .and_then(|f| f.as_f64());
                    let disabled = b.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false)
                        || b.get("isExhausted")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                    let rt = b
                        .get("resetTime")
                        .or_else(|| b.get("reset_time"))
                        .and_then(|s| s.as_str());
                    Some(build_quota_bucket(b_name, frac, disabled, rt))
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

fn merge_quota_groups(api_groups: Vec<QuotaGroup>, fallback: &[QuotaGroup]) -> Vec<QuotaGroup> {
    fallback
        .iter()
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
    let fallback_groups = parse_available_models(models_json)?;
    let api_groups = quota_json
        .map(parse_quota_summary_groups)
        .unwrap_or_default();
    let groups = merge_quota_groups(api_groups, &fallback_groups);
    let plan = parse_plan_info(plan_json, email);
    Some(UsageSummary { plan, groups })
}
