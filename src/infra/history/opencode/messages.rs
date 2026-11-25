use std::path::Path;

use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::protocol::fnv1a_64;

use super::super::StoredMessage;
use super::{default_path, open_readonly, AssistantPayload, MessageModel};

#[derive(Deserialize, Default)]
struct UserPayload {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Vec<Value>,
}

pub fn load_opencode_messages(session_id: &str) -> Vec<StoredMessage> {
    default_path()
        .map(|path| load_opencode_messages_at(&path, session_id))
        .unwrap_or_default()
}

pub fn load_opencode_messages_at(path: &Path, session_id: &str) -> Vec<StoredMessage> {
    if !session_id.starts_with("ses_") {
        return Vec::new();
    }
    let Some(conn) = open_readonly(path) else {
        return Vec::new();
    };
    load_opencode_messages_from_conn(&conn, session_id)
}

pub(super) fn load_opencode_messages_from_conn(
    conn: &Connection,
    session_id: &str,
) -> Vec<StoredMessage> {
    if !session_id.starts_with("ses_") {
        return Vec::new();
    }
    let mut stmt = match conn.prepare(
        "SELECT type, data FROM session_message
         WHERE session_id = ?1
         ORDER BY seq ASC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let mut messages = Vec::new();
    if let Ok(mut rows) = stmt.query(params![session_id]) {
        while let Ok(Some(row)) = rows.next() {
            let Ok(kind) = row.get::<_, String>(0) else {
                continue;
            };
            let Ok(raw_data) = row.get::<_, String>(1) else {
                continue;
            };
            match kind.as_str() {
                "user" => {
                    if let Some(text) = extract_user_text(&raw_data) {
                        messages.push(text_message("user", text));
                    }
                }
                "assistant" => {
                    append_assistant_messages(&raw_data, &mut messages);
                }
                _ => {}
            }
        }
    }
    messages
}

fn extract_user_text(raw: &str) -> Option<String> {
    let payload: UserPayload = serde_json::from_str(raw).ok()?;
    if let Some(text) = payload
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        return Some(text.to_string());
    }
    let parts: Vec<&str> = payload
        .content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn append_assistant_messages(raw: &str, out: &mut Vec<StoredMessage>) {
    let Ok(payload) = serde_json::from_str::<AssistantPayload>(raw) else {
        return;
    };
    let model = payload
        .model
        .as_ref()
        .and_then(MessageModel::full_id)
        .unwrap_or_default();
    let duration_seconds = payload.turn_duration_millis().map(|ms| ms as f64 / 1000.0);
    let output_tokens = payload
        .tokens
        .as_ref()
        .map(|t| t.output)
        .filter(|count| *count > 0);
    let turn_meta_json = (!model.is_empty() || output_tokens.is_some()).then(|| {
        json!({
            "mode": "auto",
            "model": model,
            "duration_seconds": duration_seconds,
            "output_tokens": output_tokens,
        })
        .to_string()
    });

    let mut last_assistant_idx: Option<usize> = None;
    for part in &payload.content {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
        match part_type {
            "reasoning" | "thought" => {
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    out.push(text_message("thought", text.to_string()));
                }
            }
            "tool" | "tool_use" => {
                let tool_name = part
                    .get("name")
                    .or_else(|| part.get("tool"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let step_index = part
                    .get("id")
                    .or_else(|| part.get("callID"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(fnv1a_64);
                let state = part.get("state");
                let tool_args = state
                    .and_then(|s| s.get("input"))
                    .filter(|v| !v.is_null())
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    });
                let output = state.map(extract_tool_output).unwrap_or_default();
                out.push(StoredMessage {
                    role: "tool".to_string(),
                    content: output,
                    tool_name: Some(tool_name),
                    tool_args,
                    step_index,
                    turn_meta: None,
                });
            }
            "text" => {
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.trim().is_empty())
                {
                    out.push(text_message("assistant", text.to_string()));
                    last_assistant_idx = Some(out.len() - 1);
                }
            }
            _ => {}
        }
    }
    if let (Some(idx), Some(meta)) = (last_assistant_idx, turn_meta_json) {
        out[idx].turn_meta = Some(meta);
    }
}

fn extract_tool_output(state: &Value) -> String {
    if let Some(output) = state.get("output") {
        let s = match output {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        if !s.trim().is_empty() {
            return s;
        }
    }
    if let Some(items) = state.get("content").and_then(Value::as_array) {
        let texts: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect();
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    if let Some(err) = state.get("error").and_then(Value::as_str) {
        if !err.trim().is_empty() {
            return err.to_string();
        }
    }
    String::new()
}

fn text_message(role: &str, content: String) -> StoredMessage {
    StoredMessage {
        role: role.to_string(),
        content,
        tool_name: None,
        tool_args: None,
        step_index: None,
        turn_meta: None,
    }
}
