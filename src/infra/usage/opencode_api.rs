use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::core::usage::{
    parse_credential_value, parse_go_usage, summary_from_go, GoUsage, UsageState, UsageSummary,
};

const GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

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

pub fn fetch_go_usage(token: &str) -> Result<GoUsage, String> {
    let body = http_get(GO_USAGE_URL, token)?;
    parse_go_usage(&body).ok_or_else(|| "unexpected usage payload".to_string())
}

pub fn execute_fetch(fallback: &UsageSummary) -> UsageState {
    match resolve_go_token() {
        Some(token) => match fetch_go_usage(&token) {
            Ok(usage) => UsageState::Ready(summary_from_go(&usage)),
            Err(_) => UsageState::Ready(fallback.clone()),
        },
        None => UsageState::Ready(fallback.clone()),
    }
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
