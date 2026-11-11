use std::io::BufRead;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

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

pub fn parse_transcript_reader<R: BufRead>(reader: R) -> Vec<TranscriptBlock> {
    let mut blocks = Vec::new();
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
            Some("PLANNER_RESPONSE") => {
                if let Some(thought) = entry.thinking {
                    let t_trimmed = thought.trim();
                    if !t_trimmed.is_empty() {
                        blocks.push(TranscriptBlock::Thought(t_trimmed.to_string()));
                    }
                }
                if let Some(calls) = entry.tool_calls {
                    for call in calls {
                        let formatted = format_tool_args(&call.args);
                        pending_tools.push(PendingHistoricalTool {
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
            Some("GENERIC") if !pending_tools.is_empty() => {
                let p = pending_tools.remove(0);
                blocks.push(TranscriptBlock::ToolCall {
                    step_index: p.step_index,
                    name: p.name,
                    args: p.args,
                    output: entry.content,
                });
            }
            _ => {}
        }
    }

    for p in pending_tools.drain(..) {
        blocks.push(TranscriptBlock::ToolCall {
            step_index: p.step_index,
            name: p.name,
            args: p.args,
            output: None,
        });
    }

    blocks
}
