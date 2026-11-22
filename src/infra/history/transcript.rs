use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::infra::history::TelemetrySnapshot;
use crate::protocol::TokenUsage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptBlock {
    UserMessage(String),
    Thought(String),
    ToolCall {
        step_index: Option<u64>,
        name: String,
        args: Option<String>,
        output: Option<String>,
    },
    AssistantMessage(String),
}

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
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_tokens: Option<u64>,
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

fn parse_transcript_inner<R: BufRead>(
    reader: R,
    collect_blocks: bool,
    collect_telemetry: bool,
) -> (Vec<TranscriptBlock>, Option<TelemetrySnapshot>) {
    let mut blocks = Vec::new();
    let mut pending_tools: VecDeque<PendingHistoricalTool> = VecDeque::new();
    let mut turns: u64 = 0;
    let mut in_turn = false;
    let mut latest_model: Option<String> = None;
    let mut step_usages: Vec<TokenUsage> = Vec::new();
    let mut step_indices: HashMap<u64, usize> = HashMap::new();

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
                if collect_telemetry {
                    in_turn = true;
                }
                if collect_blocks {
                    for p in pending_tools.drain(..) {
                        blocks.push(TranscriptBlock::ToolCall {
                            step_index: p.step_index,
                            name: p.name,
                            args: p.args,
                            output: None,
                        });
                    }
                    if let Some(raw) = entry.content {
                        let cleaned = clean_user_prompt(&raw);
                        if !cleaned.is_empty() {
                            blocks.push(TranscriptBlock::UserMessage(cleaned));
                        }
                    }
                }
            }
            Some("PLANNER_RESPONSE") => {
                if collect_telemetry {
                    if in_turn {
                        turns = turns.saturating_add(1);
                        in_turn = false;
                    } else if turns == 0 {
                        turns = 1;
                    }
                    if let Some(raw_model) = entry.model.as_deref() {
                        let clean = crate::config::sanitize_model_id(raw_model);
                        if !clean.is_empty() {
                            latest_model = Some(clean);
                        }
                    }
                    let input = entry.input_tokens.unwrap_or(0);
                    let output = entry.output_tokens.unwrap_or(0);
                    let cache_read = entry.cache_read_tokens.unwrap_or(0);
                    if input > 0 || output > 0 || cache_read > 0 {
                        let usage = TokenUsage {
                            input_tokens: input,
                            output_tokens: output,
                            thinking_tokens: 0,
                            cache_read_tokens: cache_read,
                            total_tokens: input.saturating_add(output).saturating_add(cache_read),
                        };
                        if let Some(idx) = entry.step_index {
                            if let Some(&pos) = step_indices.get(&idx) {
                                step_usages[pos] = usage;
                            } else {
                                step_indices.insert(idx, step_usages.len());
                                step_usages.push(usage);
                            }
                        } else {
                            step_usages.push(usage);
                        }
                    }
                }
                if collect_blocks {
                    if let Some(thought) = entry.thinking {
                        let t_trimmed = thought.trim();
                        if !t_trimmed.is_empty() {
                            blocks.push(TranscriptBlock::Thought(t_trimmed.to_string()));
                        }
                    }
                    if let Some(calls) = entry.tool_calls {
                        for call in calls {
                            let formatted = format_tool_args(&call.args);
                            pending_tools.push_back(PendingHistoricalTool {
                                step_index: entry.step_index,
                                name: call.name,
                                args: formatted,
                            });
                        }
                    }
                    if let Some(content) = entry.content {
                        let c_trimmed = content.trim();
                        if !c_trimmed.is_empty() {
                            for p in pending_tools.drain(..) {
                                blocks.push(TranscriptBlock::ToolCall {
                                    step_index: p.step_index,
                                    name: p.name,
                                    args: p.args,
                                    output: None,
                                });
                            }
                            blocks.push(TranscriptBlock::AssistantMessage(c_trimmed.to_string()));
                        }
                    }
                }
            }
            Some("GENERIC") if collect_blocks => {
                if let Some(p) = pending_tools.pop_front() {
                    blocks.push(TranscriptBlock::ToolCall {
                        step_index: p.step_index,
                        name: p.name,
                        args: p.args,
                        output: entry.content,
                    });
                }
            }
            _ => {}
        }
    }

    if collect_blocks {
        for p in pending_tools.drain(..) {
            blocks.push(TranscriptBlock::ToolCall {
                step_index: p.step_index,
                name: p.name,
                args: p.args,
                output: None,
            });
        }
    }

    let telemetry = if collect_telemetry && (!step_usages.is_empty() || latest_model.is_some()) {
        let last = step_usages.last().copied().unwrap_or_default();
        let mut accumulated = TokenUsage::default();
        for usage in &step_usages {
            accumulated.accumulate(usage);
        }
        let steps = step_usages.len() as u64;
        Some(TelemetrySnapshot {
            last,
            accumulated,
            steps,
            turns,
            duration_millis: None,
            context_window: None,
            model: latest_model,
        })
    } else {
        None
    };

    (blocks, telemetry)
}

pub fn parse_transcript_all<R: BufRead>(
    reader: R,
) -> (Vec<TranscriptBlock>, Option<TelemetrySnapshot>) {
    parse_transcript_inner(reader, true, true)
}

pub fn parse_transcript_reader<R: BufRead>(reader: R) -> Vec<TranscriptBlock> {
    parse_transcript_inner(reader, true, false).0
}

pub fn parse_transcript_telemetry<R: BufRead>(reader: R) -> Option<TelemetrySnapshot> {
    parse_transcript_inner(reader, false, true).1
}

pub fn load_usage_at(path: &Path) -> Option<TelemetrySnapshot> {
    let file = File::open(path).ok()?;
    parse_transcript_telemetry(BufReader::new(file))
}

pub fn load_usage(conv_id: &str) -> Option<TelemetrySnapshot> {
    let path = transcript_path_for_conversation(conv_id)?;
    load_usage_at(&path)
}
