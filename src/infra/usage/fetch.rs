use crate::core::usage::{build_usage_summary, UsageState};

pub fn execute_fetch() -> UsageState {
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
