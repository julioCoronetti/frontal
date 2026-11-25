use std::time::Duration;

use crate::core::usage::{parse_opencode_stats_json, StatsState};

pub fn execute_stats_fetch() -> StatsState {
    match crate::transport::run_opencode_command(
        &["stats", "--json", "--models", "--tools", "--cost"],
        Duration::from_secs(5),
    ) {
        Ok(bytes) => {
            let raw = String::from_utf8_lossy(&bytes);
            match parse_opencode_stats_json(&raw) {
                Ok(stats) => StatsState::Ready(stats),
                Err(err) => StatsState::Unavailable(err),
            }
        }
        Err(err) => StatsState::Unavailable(err),
    }
}
