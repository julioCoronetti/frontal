use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::EngineConfig;
use crate::protocol::{media_uri_to_path, PromptPayload};

pub fn resolve_opencode_binary() -> PathBuf {
    if let Ok(path_str) = env::var("FRONTAL_OPENCODE_BIN") {
        let candidate = PathBuf::from(path_str);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("opencode")
}

pub fn is_valid_opencode_session_id(s: &str) -> bool {
    s.starts_with("ses_")
        && (5..=128).contains(&s.len())
        && s.as_bytes()[4..]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn opencode_variant(effort: &str) -> Option<&'static str> {
    match effort.trim().to_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("high"),
        "high" | "xhigh" | "max" => Some("max"),
        _ => None,
    }
}

pub fn model_with_variant(model: &str, effort: Option<&str>) -> String {
    let model = crate::config::sanitize_model_id(model)
        .trim_start_matches('-')
        .to_string();
    if model.is_empty() || model.contains('#') {
        return model;
    }
    match effort.and_then(opencode_variant) {
        Some(variant) => format!("{}#{}", model, variant),
        None => model,
    }
}

pub fn build_opencode_args(
    config: &EngineConfig,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    build_opencode_payload_args(config, session_id, &PromptPayload::from_text(prompt))
}

pub fn build_opencode_payload_args(
    config: &EngineConfig,
    session_id: Option<&str>,
    payload: &PromptPayload,
) -> Vec<String> {
    let normalized_mode = config
        .mode
        .as_deref()
        .and_then(crate::config::BackendKind::normalize_mode);
    let mut args = vec![
        "run".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--thinking".to_string(),
    ];
    if config.allows_auto_permissions() {
        args.push("--auto".to_string());
    }
    if normalized_mode == Some("plan") {
        args.push("--agent".to_string());
        args.push("plan".to_string());
    }
    if let Some(model) = config.model.as_deref() {
        let resolved_model = model_with_variant(model, config.effort.as_deref());
        if !resolved_model.is_empty() {
            args.push("--model".to_string());
            args.push(resolved_model);
        }
    }
    if let Some(sid) = session_id.filter(|s| is_valid_opencode_session_id(s)) {
        args.push("--session".to_string());
        args.push(sid.to_string());
    }
    for item in &payload.media {
        let path = media_uri_to_path(&item.uri);
        if !path.is_empty() && !path.chars().any(|c| c.is_control()) {
            let safe_path = if path.starts_with('-') {
                format!("./{}", path)
            } else {
                path.to_string()
            };
            args.push("--file".to_string());
            args.push(safe_path);
        }
    }
    let prompt_arg = if payload.text.trim().is_empty() && !payload.media.is_empty() {
        "See attached media."
    } else {
        payload.text.as_str()
    };
    args.push("--".to_string());
    args.push(prompt_arg.to_string());
    args
}

pub fn parse_opencode_models_output(raw: &str) -> Vec<(String, String)> {
    let mut models = Vec::new();
    for line in raw.lines() {
        let cleaned = strip_ansi(line);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.split('/').count() != 2 {
            continue;
        }
        let Some((provider, model)) = trimmed.split_once('/') else {
            continue;
        };
        if provider.is_empty()
            || model.is_empty()
            || provider
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || model.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            continue;
        }
        models.push((trimmed.to_string(), model.to_string()));
    }
    models
}

pub async fn fetch_opencode_models(bin: &Path) -> Vec<(String, String)> {
    let bin_owned = bin.to_path_buf();
    let Ok(Ok(output)) = tokio::task::spawn_blocking(move || {
        run_opencode_command_with_bin(&bin_owned, &["models"], Duration::from_secs(3))
    })
    .await
    else {
        return Vec::new();
    };
    parse_opencode_models_output(&String::from_utf8_lossy(&output))
}

pub fn run_opencode_command(args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let bin = resolve_opencode_binary();
    run_opencode_command_with_bin(&bin, args, timeout)
}

pub fn run_opencode_command_with_bin(
    bin: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let parent_pid = unsafe { libc::getpid() };
        cmd.process_group(0);
        unsafe {
            cmd.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                if libc::getppid() != parent_pid {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id() as i32;
    crate::engine::register_child_pgid(pid);

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let mut exited = None;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exited = Some(Ok(status));
                break;
            }
            Ok(None) => {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining = deadline.saturating_duration_since(now);
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(err) => {
                exited = Some(Err(err));
                break;
            }
        }
    }

    let status = match exited {
        Some(Ok(status)) => {
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
            crate::engine::unregister_child_pgid(pid);
            status
        }
        Some(Err(err)) => {
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let _ = child.wait();
            crate::engine::unregister_child_pgid(pid);
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(err.to_string());
        }
        None => {
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid, libc::SIGTERM);
            }
            let sigterm_deadline = Instant::now() + Duration::from_millis(50);
            let mut terminated = false;
            while Instant::now() < sigterm_deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    terminated = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
            if !terminated {
                #[cfg(not(unix))]
                {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
            crate::engine::unregister_child_pgid(pid);
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(format!("opencode {} timed out", args.join(" ")));
        }
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        let err_msg = strip_ansi(&String::from_utf8_lossy(&stderr))
            .trim()
            .to_string();
        if err_msg.is_empty() {
            return Err(format!("opencode {} failed", args.join(" ")));
        }
        return Err(err_msg);
    }

    Ok(stdout)
}

pub fn delete_opencode_session(session_id: &str) -> Result<(), String> {
    if !is_valid_opencode_session_id(session_id) {
        return Err("invalid opencode session id".to_string());
    }
    run_opencode_command(&["session", "delete", session_id], Duration::from_secs(5)).map(|_| ())
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('P' | 'X' | '^' | '_') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('(' | ')' | '*' | '+' | '#' | ' ') => {
                    chars.next();
                    let _ = chars.next();
                }
                Some('\x40'..='\x5f') | Some('\x60'..='\x7e') => {
                    chars.next();
                }
                _ => {}
            }
        } else if !ch.is_control() || ch == '\n' || ch == '\t' {
            out.push(ch);
        }
    }
    out
}
