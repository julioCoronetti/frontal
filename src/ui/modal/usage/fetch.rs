use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::UsageState;

pub use super::quota_parse::{
    build_usage_summary, parse_available_models, parse_plan_info, parse_quota_summary_groups,
};

struct QuotaFetchState {
    in_flight: bool,
    waiters: Vec<Sender<UsageState>>,
}

static QUOTA_FETCH_STATE: Mutex<QuotaFetchState> = Mutex::new(QuotaFetchState {
    in_flight: false,
    waiters: Vec::new(),
});

struct QuotaFetchGuard;

impl Drop for QuotaFetchGuard {
    fn drop(&mut self) {
        let waiters = {
            let mut state = QUOTA_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
            if !state.in_flight {
                return;
            }
            state.in_flight = false;
            std::mem::take(&mut state.waiters)
        };
        let err_state = UsageState::Unavailable("Quota worker interrupted.".to_string());
        for waiter in waiters {
            let _ = waiter.send(err_state.clone());
        }
    }
}

pub(super) fn clean_text(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect()
}

pub(crate) fn is_valid_calendar(d: &[i64], t: &[i64]) -> bool {
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

pub(crate) fn civil_to_days(mut y: i64, m: u64, d: u64) -> i64 {
    y -= (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) as i64 - 719468
}

pub(crate) fn parse_iso_ymd_hms(iso: &str) -> Option<([i64; 3], [i64; 3])> {
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

pub fn fetch_quota_async(tx: Sender<UsageState>) {
    {
        let mut state = QUOTA_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.waiters.push(tx);
        if state.in_flight {
            return;
        }
        state.in_flight = true;
    }
    std::thread::spawn(move || {
        let _guard = QuotaFetchGuard;
        let state = execute_fetch();
        if let UsageState::Ready(ref summary) = state {
            let mut cache = super::USAGE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some((std::time::Instant::now(), summary.clone()));
        }
        let waiters: Vec<Sender<UsageState>> = {
            let mut fetch_state = QUOTA_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
            fetch_state.in_flight = false;
            std::mem::take(&mut fetch_state.waiters)
        };
        for waiter in waiters {
            let _ = waiter.send(state.clone());
        }
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

fn execute_http_fetch(token: &str, email: Option<String>) -> UsageState {
    let ep = |url: &'static str| super::cli::post_endpoint(url, token, Some("Antigravity/1.5.3"));
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
