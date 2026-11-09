use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::ui::cache::RenderCache;

#[derive(Deserialize)]
struct TranscriptToolCall {
    name: String,
    #[serde(default)]
    args: Option<Value>,
}

#[derive(Deserialize)]
struct TranscriptEntry {
    #[serde(default)]
    step_index: Option<u64>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<TranscriptToolCall>>,
}

pub fn clean_user_prompt(raw: &str) -> String {
    if let Some(start) = raw.find("<USER_REQUEST>") {
        let after = &raw[start + "<USER_REQUEST>".len()..];
        if let Some(end) = after.find("</USER_REQUEST>") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(meta_idx) = raw.find("<ADDITIONAL_METADATA>") {
        return raw[..meta_idx].trim().to_string();
    }
    raw.trim().to_string()
}

pub fn transcript_path_for_conversation(conv_id: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home)
        .join(".gemini/antigravity-cli/brain")
        .join(conv_id)
        .join(".system_generated/logs/transcript.jsonl");
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

pub fn format_tool_args(args: &Option<Value>) -> Option<String> {
    match args {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(map)) => {
            let mut parts = Vec::new();
            for (k, v) in map {
                let v_str = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                parts.push(format!("{}={}", k, v_str));
            }
            Some(parts.join(", "))
        }
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

struct PendingHistoricalTool {
    step_index: Option<u64>,
    name: String,
    args: Option<String>,
}

pub fn hydrate_cache_from_reader<R: BufRead>(reader: R, cache: &mut RenderCache) -> usize {
    let initial_count = cache.block_count();
    let mut pending_tools: Vec<PendingHistoricalTool> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: TranscriptEntry = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match entry.r#type.as_deref() {
            Some("USER_INPUT") => {
                for p in pending_tools.drain(..) {
                    cache.push_historical_tool_call(p.step_index, &p.name, p.args.as_deref(), None);
                }
                if let Some(raw) = entry.content {
                    let cleaned = clean_user_prompt(&raw);
                    if !cleaned.is_empty() {
                        cache.push_user_message(&cleaned);
                    }
                }
            }
            Some("PLANNER_RESPONSE") => {
                if let Some(thought) = entry.thinking {
                    let t_trimmed = thought.trim();
                    if !t_trimmed.is_empty() {
                        cache.push_historical_thought(t_trimmed);
                    }
                }
                if let Some(calls) = entry.tool_calls {
                    for call in calls {
                        pending_tools.push(PendingHistoricalTool {
                            step_index: entry.step_index,
                            name: call.name,
                            args: format_tool_args(&call.args),
                        });
                    }
                }
                if let Some(content) = entry.content {
                    let c_trimmed = content.trim();
                    if !c_trimmed.is_empty() {
                        for p in pending_tools.drain(..) {
                            cache.push_historical_tool_call(
                                p.step_index,
                                &p.name,
                                p.args.as_deref(),
                                None,
                            );
                        }
                        cache.push_historical_assistant_message(c_trimmed);
                    }
                }
            }
            Some("GENERIC") if !pending_tools.is_empty() => {
                let p = pending_tools.remove(0);
                cache.push_historical_tool_call(
                    p.step_index,
                    &p.name,
                    p.args.as_deref(),
                    entry.content.as_deref(),
                );
            }
            _ => {}
        }
    }

    for p in pending_tools.drain(..) {
        cache.push_historical_tool_call(p.step_index, &p.name, p.args.as_deref(), None);
    }
    cache.consolidate_active_turn(None);
    cache.block_count().saturating_sub(initial_count)
}

pub fn hydrate_cache_from_transcript(conv_id: &str, cache: &mut RenderCache) -> bool {
    let path = match transcript_path_for_conversation(conv_id) {
        Some(p) => p,
        None => return false,
    };
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let reader = BufReader::new(file);
    let count = hydrate_cache_from_reader(reader, cache);
    count > 0
}
