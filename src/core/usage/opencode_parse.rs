use std::time::{SystemTime, UNIX_EPOCH};

use super::parse::{civil_to_days, clean_text, parse_iso_ymd_hms};
use super::{
    OpenCodeModelStat, OpenCodeStats, OpenCodeToolStat, QuotaBucket, QuotaGroup, UsagePlanInfo,
    UsageSummary,
};

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

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next();
                    for seq in chars.by_ref() {
                        if ('@'..='~').contains(&seq) {
                            break;
                        }
                    }
                    continue;
                }
                if next == ']' {
                    chars.next();
                    while let Some(seq) = chars.next() {
                        if seq == '\x07' {
                            break;
                        }
                        if seq == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
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
    let (d, t) = parse_iso_ymd_hms(iso)?;
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

pub fn parse_credential_value(value: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    parsed
        .get("key")
        .and_then(|key| key.as_str())
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn sanitize_non_neg_f64(val: f64) -> f64 {
    if val.is_finite() && val >= 0.0 {
        val
    } else {
        0.0
    }
}

pub fn parse_opencode_stats_json(raw: &str) -> Result<OpenCodeStats, String> {
    let cleaned = strip_ansi(raw);
    let val: serde_json::Value =
        serde_json::from_str(cleaned.trim()).map_err(|e| format!("Invalid stats JSON: {}", e))?;

    let tokens = val.get("tokens");
    let cache = tokens.and_then(|t| t.get("cache"));
    let tools_obj = val.get("tools");
    let totals = tools_obj.and_then(|t| t.get("totals"));

    let mut models = Vec::new();
    if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
        for item in arr {
            let model_meta = item.get("model");
            let id = clean_text(
                model_meta
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            );
            let provider_id = clean_text(
                model_meta
                    .and_then(|m| m.get("providerID"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            );
            let variant = model_meta
                .and_then(|m| m.get("variant"))
                .and_then(|v| v.as_str())
                .map(clean_text)
                .filter(|v| !v.is_empty() && v != "default");
            let m_tokens = item.get("tokens");
            let m_cache = m_tokens.and_then(|t| t.get("cache"));
            models.push(OpenCodeModelStat {
                id,
                provider_id,
                variant,
                steps: item.get("steps").and_then(|v| v.as_u64()).unwrap_or(0),
                input_tokens: m_tokens
                    .and_then(|t| t.get("input"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                output_tokens: m_tokens
                    .and_then(|t| t.get("output"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                reasoning_tokens: m_tokens
                    .and_then(|t| t.get("reasoning"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_read_tokens: m_cache
                    .and_then(|c| c.get("read"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cost: sanitize_non_neg_f64(
                    item.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0),
                ),
            });
        }
    }
    models.sort_by_key(|a| std::cmp::Reverse(a.steps));

    let mut tools = Vec::new();
    if let Some(arr) = tools_obj
        .and_then(|t| t.get("usage"))
        .and_then(|u| u.as_array())
    {
        for item in arr {
            let name = clean_text(
                item.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            );
            tools.push(OpenCodeToolStat {
                name,
                calls: item.get("calls").and_then(|v| v.as_u64()).unwrap_or(0),
                succeeded: item.get("succeeded").and_then(|v| v.as_u64()).unwrap_or(0),
                failed: item.get("failed").and_then(|v| v.as_u64()).unwrap_or(0),
                duration_p50_ms: item
                    .get("durationP50")
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite() && *v >= 0.0),
            });
        }
    }
    tools.sort_by_key(|a| std::cmp::Reverse(a.calls));

    Ok(OpenCodeStats {
        sessions: val.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0),
        subagents: val.get("subagents").and_then(|v| v.as_u64()).unwrap_or(0),
        prompts: val.get("prompts").and_then(|v| v.as_u64()).unwrap_or(0),
        steps: val.get("steps").and_then(|v| v.as_u64()).unwrap_or(0),
        input_tokens: tokens
            .and_then(|t| t.get("input"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output_tokens: tokens
            .and_then(|t| t.get("output"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        reasoning_tokens: tokens
            .and_then(|t| t.get("reasoning"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_read_tokens: cache
            .and_then(|c| c.get("read"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_write_tokens: cache
            .and_then(|c| c.get("write"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cost: sanitize_non_neg_f64(val.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0)),
        active_days: val.get("activeDays").and_then(|v| v.as_u64()).unwrap_or(0),
        streak: val.get("streak").and_then(|v| v.as_u64()).unwrap_or(0),
        tool_calls: totals
            .and_then(|t| t.get("calls"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_succeeded: totals
            .and_then(|t| t.get("succeeded"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_failed: totals
            .and_then(|t| t.get("failed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_unfinished: totals
            .and_then(|t| t.get("unfinished"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        models,
        tools,
    })
}
