use std::process::{Command, Stdio};
use std::time::Duration;

use super::{UsagePlanInfo, UsageSummary};
use crate::transport::EngineConfig;

pub fn fetch_agy_cli_quota() -> Result<UsageSummary, String> {
    let agy_bin = EngineConfig::resolve_agy_binary();
    let mut cmd = Command::new(&agy_bin);
    cmd.args(["-p", "/usage", "--output-format", "json"])
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

    let child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id() as i32;
    crate::engine::register_child_pgid(pid);

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });

    let output = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(res) => {
            crate::engine::unregister_child_pgid(pid);
            res.map_err(|e| e.to_string())?
        }
        Err(_) => {
            #[cfg(unix)]
            unsafe {
                libc::killpg(pid, libc::SIGTERM);
                libc::killpg(pid, libc::SIGKILL);
            }
            crate::engine::unregister_child_pgid(pid);
            return Err("timeout executing agy /usage".to_string());
        }
    };

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
        .map(|s| s.to_string())
}
