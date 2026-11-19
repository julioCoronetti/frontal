use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::BackendKind;
use crate::protocol::{fnv1a_64, TokenUsage};

use super::{SessionRecord, StoredMessage, TitleSource};

#[derive(Deserialize, Default)]
struct AssistantPayload {
    #[serde(default)]
    tokens: Option<MessageTokens>,
    #[serde(default)]
    model: Option<MessageModel>,
    #[serde(default)]
    time: MessageTime,
    #[serde(default)]
    content: Vec<Value>,
}

#[derive(Deserialize, Default)]
struct UserPayload {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Vec<Value>,
}

#[derive(Deserialize, Default)]
struct MessageTokens {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    reasoning: u64,
    #[serde(default)]
    cache: CacheTokens,
}

#[derive(Deserialize, Default)]
struct CacheTokens {
    #[serde(default)]
    read: u64,
}

#[derive(Deserialize, Default)]
struct MessageTime {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    streamed: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
}

#[derive(Deserialize, Default)]
struct MessageModel {
    #[serde(default, rename = "providerID")]
    provider_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    variant: Option<String>,
}

impl MessageModel {
    fn full_id(&self) -> Option<String> {
        let provider = sanitize_part(self.provider_id.as_deref()?)?;
        let id = sanitize_part(self.id.as_deref()?)?;
        Some(format!("{}/{}", provider, id))
    }
}

pub fn default_path() -> Option<PathBuf> {
    crate::model_catalog::ModelCatalog::default_path()
}

pub fn normalize_timestamp_secs(raw: i64) -> i64 {
    if raw <= 0 {
        0
    } else if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    }
}

pub fn parse_opencode_model_json(raw: &str) -> (Option<String>, Option<String>) {
    let Ok(parsed) = serde_json::from_str::<MessageModel>(raw) else {
        return (None, None);
    };
    let model = parsed.full_id();
    let effort = parsed
        .variant
        .as_deref()
        .and_then(sanitize_part)
        .map(str::to_string);
    (model, effort)
}

pub fn load_usage(session_id: &str) -> Option<(TokenUsage, Option<String>, Option<u64>)> {
    default_path().and_then(|path| load_usage_at(&path, session_id))
}

pub fn load_usage_at(
    path: &Path,
    session_id: &str,
) -> Option<(TokenUsage, Option<String>, Option<u64>)> {
    if !path.is_file() || !session_id.starts_with("ses_") {
        return None;
    }
    let connection = open_readonly(path)?;
    let payload: String = connection
        .query_row(
            "SELECT data FROM session_message
             WHERE session_id = ?1 AND type = 'assistant'
             ORDER BY seq DESC LIMIT 1",
            [session_id],
            |row| row.get(0),
        )
        .ok()?;
    let payload: AssistantPayload = serde_json::from_str(&payload).ok()?;
    let tokens = payload.tokens?;
    let input = tokens.input;
    let output = tokens.output;
    let reasoning = tokens.reasoning;
    let cache_read = tokens.cache.read;
    if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 {
        return None;
    }
    let model = payload.model.and_then(|model| model.full_id());
    let end = payload.time.completed.or(payload.time.streamed);
    let duration_millis = end
        .zip(payload.time.created)
        .map(|(end, start)| end.saturating_sub(start))
        .filter(|millis| *millis > 0);
    Some((
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            thinking_tokens: reasoning,
            cache_read_tokens: cache_read,
            total_tokens: 0,
        },
        model,
        duration_millis,
    ))
}

pub fn load_opencode_conversations() -> Vec<SessionRecord> {
    default_path()
        .map(|path| load_opencode_conversations_at(&path))
        .unwrap_or_default()
}

pub fn load_opencode_conversations_at(path: &Path) -> Vec<SessionRecord> {
    let Some(conn) = open_readonly(path) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare(
        "SELECT id, title, model, COALESCE(time_updated, time_created, 0)
         FROM session_v2
         WHERE parent_id IS NULL
           AND (time_archived IS NULL OR time_archived = 0)
           AND id IS NOT NULL AND id != ''
         ORDER BY COALESCE(time_updated, time_created, 0) DESC
         LIMIT 200",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let mut records = Vec::new();
    if let Ok(mut rows) = stmt.query([]) {
        while let Ok(Some(row)) = rows.next() {
            let Ok(session_id) = row.get::<_, String>(0) else {
                continue;
            };
            let title: Option<String> = row.get(1).ok().flatten();
            let model_raw: Option<String> = row.get(2).ok().flatten();
            let raw_ts: i64 = row.get(3).unwrap_or(0);
            let final_title = clean_opencode_title(title);
            let (model, effort) = model_raw
                .as_deref()
                .map(parse_opencode_model_json)
                .unwrap_or((None, None));
            let title_source = if final_title.is_some() {
                TitleSource::Semantic
            } else {
                TitleSource::Default
            };
            records.push(SessionRecord {
                id: format!("ext-{}", session_id),
                conversation_id: Some(session_id),
                backend: Some(BackendKind::OpenCode),
                model,
                mode: None,
                effort,
                title: final_title,
                title_source,
                created_at: normalize_timestamp_secs(raw_ts),
            });
        }
    }
    records
}

fn query_session_col<T: rusqlite::types::FromSql>(
    path: &Path,
    session_id: &str,
    sql: &str,
) -> Option<T> {
    if !session_id.starts_with("ses_") {
        return None;
    }
    open_readonly(path)?
        .query_row(sql, params![session_id], |row| row.get(0))
        .ok()
}

pub fn query_opencode_title(session_id: &str) -> Option<String> {
    default_path().and_then(|path| query_opencode_title_at(&path, session_id))
}

pub fn query_opencode_title_at(path: &Path, session_id: &str) -> Option<String> {
    let title: Option<String> = query_session_col(
        path,
        session_id,
        "SELECT title FROM session_v2 WHERE id = ?1",
    )?;
    clean_opencode_title(title)
}

fn clean_opencode_title(raw: Option<String>) -> Option<String> {
    raw.as_deref()
        .map(str::trim)
        .filter(|t| {
            !t.is_empty()
                && !t.eq_ignore_ascii_case("new session")
                && !t.starts_with("New session - ")
                && !t.eq_ignore_ascii_case("untitled")
        })
        .map(str::to_string)
}

pub fn query_opencode_timestamp(session_id: &str) -> Option<i64> {
    default_path().and_then(|path| query_opencode_timestamp_at(&path, session_id))
}

pub fn query_opencode_timestamp_at(path: &Path, session_id: &str) -> Option<i64> {
    let raw_ts: i64 = query_session_col(
        path,
        session_id,
        "SELECT COALESCE(time_updated, time_created, 0) FROM session_v2 WHERE id = ?1",
    )?;
    let normalized = normalize_timestamp_secs(raw_ts);
    (normalized > 0).then_some(normalized)
}

pub fn query_opencode_model(session_id: &str) -> (Option<String>, Option<String>) {
    default_path()
        .map(|path| query_opencode_model_at(&path, session_id))
        .unwrap_or((None, None))
}

pub fn query_opencode_model_at(path: &Path, session_id: &str) -> (Option<String>, Option<String>) {
    let raw: Option<String> = query_session_col(
        path,
        session_id,
        "SELECT model FROM session_v2 WHERE id = ?1",
    )
    .flatten();
    raw.as_deref()
        .map(parse_opencode_model_json)
        .unwrap_or((None, None))
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

fn open_readonly(path: &Path) -> Option<Connection> {
    if !path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(300));
    Some(conn)
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
    let duration_seconds = payload
        .time
        .completed
        .or(payload.time.streamed)
        .zip(payload.time.created)
        .map(|(end, start)| end.saturating_sub(start))
        .filter(|ms| *ms > 0)
        .map(|ms| ms as f64 / 1000.0);
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

fn sanitize_part(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed)
    }
}
