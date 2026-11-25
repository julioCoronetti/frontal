use std::process::{Command, Stdio};
use std::time::Duration;

use super::{UsagePlanInfo, UsageSummary};
use crate::transport::EngineConfig;

pub(super) fn run_bounded_command(
    mut cmd: Command,
    stdin_data: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    cmd.stdin(if stdin_data.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let (mut child, guard) =
        crate::engine::spawn_std_isolated(&mut cmd).map_err(|e| e.to_string())?;
    let guard = std::sync::Arc::new(std::sync::Mutex::new(Some(guard)));
    let thread_guard = std::sync::Arc::clone(&guard);

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let (Some(bytes), Some(mut stdin)) = (stdin_data, child.stdin.take()) {
            use std::io::Write;
            if let Err(err) = stdin.write_all(&bytes) {
                if let Some(mut guard) = thread_guard
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    guard.kill_sigkill_and_disarm();
                }
                #[cfg(not(unix))]
                let _ = child.kill();
                let _ = child.wait();
                let _ = tx.send(Err(err));
                return;
            }
        }
        let out = child.wait_with_output();
        if let Some(mut guard) = thread_guard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            guard.disarm();
        }
        let _ = tx.send(out);
    });

    match rx.recv_timeout(timeout) {
        Ok(res) => res.map_err(|e| e.to_string()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(res) = rx.try_recv() {
                return res.map_err(|e| e.to_string());
            }
            if let Some(ref g) = *guard.lock().unwrap_or_else(|e| e.into_inner()) {
                g.signal(libc::SIGTERM);
            }
            if rx.recv_timeout(Duration::from_millis(50)).is_err() {
                if let Some(mut g) = guard.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    g.kill_sigkill_and_disarm();
                }
                let _ = rx.recv_timeout(Duration::from_millis(100));
            }
            Err("command timed out".to_string())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            if let Some(mut g) = guard.lock().unwrap_or_else(|e| e.into_inner()).take() {
                g.disarm();
            }
            Err("command wait thread disconnected".to_string())
        }
    }
}

pub fn fetch_agy_cli_quota() -> Result<UsageSummary, String> {
    let agy_bin = EngineConfig::resolve_agy_binary();
    let mut cmd = Command::new(&agy_bin);
    cmd.args(["-p", "/usage", "--output-format", "json"]);
    let output = run_bounded_command(cmd, None, Duration::from_secs(5)).map_err(|err| {
        if err == "command timed out" {
            "timeout executing agy /usage".to_string()
        } else {
            err
        }
    })?;

    if !output.status.success() {
        return Err("agy /usage returned failure status".to_string());
    }

    let val: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;

    let groups = super::fetch::parse_quota_summary_groups(&val);
    if groups.len() < 3 {
        return Err("agy /usage returned incomplete groups".to_string());
    }

    let email = read_oauth_email();
    let plan = UsagePlanInfo {
        tier_name: "Google AI Pro".to_string(),
        account_email: email,
        upgrade_url: None,
        manage_url: None,
    };

    Ok(UsageSummary { plan, groups })
}

fn read_oauth_email() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = format!("{}/.gemini/antigravity-cli/antigravity-oauth-token", home);
    let bytes = std::fs::read(&path).ok()?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    val.get("id_token_email")
        .and_then(|e| e.as_str())
        .map(super::fetch::clean_text)
        .filter(|s| !s.is_empty())
}

pub(super) fn post_endpoint(
    url: &str,
    token: &str,
    user_agent: Option<&str>,
) -> Result<serde_json::Value, String> {
    let clean_token = token.trim();
    if clean_token.is_empty()
        || clean_token
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\\'))
    {
        return Err("invalid credential token".to_string());
    }
    let mut cfg = format!(
        "header = \"Authorization: Bearer {}\"\nheader = \"Content-Type: application/json\"\n",
        clean_token
    );
    if let Some(ua) = user_agent {
        let clean_ua: String = ua
            .chars()
            .filter(|c| !c.is_control() && !matches!(c, '"' | '\\'))
            .collect();
        cfg.push_str(&format!("header = \"User-Agent: {}\"\n", clean_ua));
    }
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
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
    ]);
    let out = run_bounded_command(cmd, Some(cfg.into_bytes()), Duration::from_secs(5))
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
