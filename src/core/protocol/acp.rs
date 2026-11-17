use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    fnv1a_64, ProtocolError, ResultData, ResultEvent, ResultStatus, StepState, StepType,
    StepUpdateData, StepUpdateEvent, StreamEvent, TokenUsage, ToolInfo,
};

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(input: &[u8]) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i];
        let b1 = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] } else { 0 };
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if i + 1 < input.len() {
            out.push(BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(BASE64_ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AcpRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'static str,
    pub params: Value,
}

impl AcpRequest {
    pub fn new(id: u64, method: &'static str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }

    pub fn to_ndjson(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|e| ProtocolError::Serialization(e.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AcpNotification {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: Value,
}

impl AcpNotification {
    pub fn new(method: &'static str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
        }
    }

    pub fn to_ndjson(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self)
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|e| ProtocolError::Serialization(e.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AcpContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image", rename_all = "camelCase")]
    Image {
        data: String,
        mime_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        uri: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AcpRpcError {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPermissionOption {
    pub option_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AcpIncoming {
    Response {
        id: u64,
        result: Option<Value>,
        error: Option<AcpRpcError>,
    },
    PermissionRequest {
        id: Value,
        session_id: Option<String>,
        tool_call: Value,
        options: Vec<AcpPermissionOption>,
    },
    Notification {
        method: String,
        params: Value,
    },
    UnsupportedRequest {
        id: Value,
        method: String,
    },
    Unknown,
}

#[derive(Clone, Debug, Default)]
pub struct AcpStreamState {
    pub context_window_hint: Option<u64>,
    pub tool_names: HashMap<String, String>,
}

pub fn parse_acp_line(line: &str) -> Result<Option<AcpIncoming>, ProtocolError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let val: Value =
        serde_json::from_str(trimmed).map_err(|e| ProtocolError::Deserialization(e.to_string()))?;
    if let Some(method) = val.get("method").and_then(Value::as_str) {
        let params = val.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = val.get("id").cloned() {
            if !id.is_null() && method == "session/request_permission" {
                let session_id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
                let options = params
                    .get("options")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Vec<AcpPermissionOption>>(v).ok())
                    .unwrap_or_default();
                return Ok(Some(AcpIncoming::PermissionRequest {
                    id,
                    session_id,
                    tool_call,
                    options,
                }));
            }
            return Ok(Some(AcpIncoming::UnsupportedRequest {
                id,
                method: method.to_string(),
            }));
        }
        return Ok(Some(AcpIncoming::Notification {
            method: method.to_string(),
            params,
        }));
    }
    if let Some(id) = val.get("id").and_then(Value::as_u64) {
        let error = val
            .get("error")
            .cloned()
            .and_then(|v| serde_json::from_value::<AcpRpcError>(v).ok());
        let result = val.get("result").cloned();
        return Ok(Some(AcpIncoming::Response { id, result, error }));
    }
    Ok(Some(AcpIncoming::Unknown))
}

pub fn select_permission_option_id(options: &[AcpPermissionOption]) -> Option<String> {
    for preferred_kind in ["allow_always", "allow_once", "allow"] {
        if let Some(opt) = options
            .iter()
            .find(|o| o.kind.as_deref() == Some(preferred_kind))
        {
            return Some(opt.option_id.clone());
        }
    }
    options.first().map(|o| o.option_id.clone())
}

pub fn format_permission_response(
    id: Value,
    option_id: Option<&str>,
) -> Result<String, ProtocolError> {
    let outcome = match option_id {
        Some(opt) => json!({ "outcome": "selected", "optionId": opt }),
        None => json!({ "outcome": "cancelled" }),
    };
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "outcome": outcome }
    });
    serde_json::to_string(&payload)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| ProtocolError::Serialization(e.to_string()))
}

pub fn format_method_not_found(id: Value, method: &str) -> Result<String, ProtocolError> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32601,
            "message": format!("Method not found: {}", method)
        }
    });
    serde_json::to_string(&payload)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| ProtocolError::Serialization(e.to_string()))
}

pub fn parse_session_update(params: &Value, state: &mut AcpStreamState) -> Option<StreamEvent> {
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let update = params.get("update")?;
    let kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    match kind {
        "agent_message_chunk" => {
            let text = extract_chunk_text(update)?;
            Some(StreamEvent::StepUpdate(StepUpdateEvent {
                conversation_id: session_id.clone(),
                step_update: Some(StepUpdateData {
                    conversation_id: session_id,
                    step_type: Some(StepType::AgentResponse),
                    text_delta: Some(text),
                    ..Default::default()
                }),
            }))
        }
        "agent_thought_chunk" => {
            let thought = extract_chunk_text(update)?;
            Some(StreamEvent::StepUpdate(StepUpdateEvent {
                conversation_id: session_id.clone(),
                step_update: Some(StepUpdateData {
                    conversation_id: session_id,
                    step_type: Some(StepType::Thought),
                    thought: Some(thought),
                    ..Default::default()
                }),
            }))
        }
        "tool_call" | "tool_call_update" => {
            let raw_id = update
                .get("toolCallId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty());
            let explicit_name = update
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| update.get("title").and_then(Value::as_str))
                .or_else(|| update.get("kind").and_then(Value::as_str))
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let status = update.get("status").and_then(Value::as_str).unwrap_or("");
            let step_state = match status {
                "completed" | "done" => StepState::Done,
                "failed" | "error" | "cancelled" => StepState::Error,
                _ => StepState::Active,
            };
            let tool_name = match (explicit_name, raw_id) {
                (Some(name), Some(id)) => {
                    if step_state == StepState::Active {
                        state.tool_names.insert(id.to_string(), name.clone());
                    } else {
                        state.tool_names.remove(id);
                    }
                    name
                }
                (Some(name), None) => name,
                (None, Some(id)) => {
                    if step_state == StepState::Active {
                        state
                            .tool_names
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| "tool".to_string())
                    } else {
                        state
                            .tool_names
                            .remove(id)
                            .unwrap_or_else(|| "tool".to_string())
                    }
                }
                (None, None) => "tool".to_string(),
            };
            let step_index = raw_id.map(fnv1a_64);
            let parameters = update
                .get("rawInput")
                .filter(|v| !v.is_null())
                .cloned()
                .or_else(|| update.get("args").filter(|v| !v.is_null()).cloned());
            let output = extract_tool_output(update);
            Some(StreamEvent::StepUpdate(StepUpdateEvent {
                conversation_id: session_id.clone(),
                step_update: Some(StepUpdateData {
                    conversation_id: session_id,
                    step_index,
                    state: Some(step_state),
                    step_type: Some(StepType::Tool),
                    tool_name: Some(tool_name.clone()),
                    tool_info: Some(ToolInfo {
                        name: Some(tool_name),
                        parameters,
                        output,
                    }),
                    ..Default::default()
                }),
            }))
        }
        "usage_update" => {
            let size = update.get("size").and_then(Value::as_u64)?;
            state.context_window_hint = Some(size);
            Some(StreamEvent::StepUpdate(StepUpdateEvent {
                conversation_id: session_id.clone(),
                step_update: Some(StepUpdateData {
                    conversation_id: session_id,
                    context_window: Some(size),
                    ..Default::default()
                }),
            }))
        }
        _ => None,
    }
}

pub fn parse_prompt_result(
    conversation_id: Option<String>,
    model: Option<String>,
    context_window: Option<u64>,
    result: Option<&Value>,
    error: Option<&AcpRpcError>,
) -> StreamEvent {
    if let Some(err) = error {
        return StreamEvent::Result(ResultEvent {
            conversation_id: conversation_id.clone(),
            result: Some(ResultData {
                conversation_id,
                status: Some(ResultStatus::Error),
                error: Some(err.message.clone()),
                model,
                context_window,
                ..Default::default()
            }),
        });
    }
    let usage = result.and_then(|r| r.get("usage")).map(parse_acp_usage);
    let stop_reason = result
        .and_then(|r| r.get("stopReason"))
        .and_then(Value::as_str);
    let status = match stop_reason {
        Some("error") => ResultStatus::Error,
        Some("cancelled") => ResultStatus::Other,
        _ => ResultStatus::Success,
    };
    StreamEvent::Result(ResultEvent {
        conversation_id: conversation_id.clone(),
        result: Some(ResultData {
            conversation_id,
            status: Some(status),
            usage,
            model,
            context_window,
            ..Default::default()
        }),
    })
}

fn parse_acp_usage(u: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: u.get("inputTokens").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: u.get("outputTokens").and_then(Value::as_u64).unwrap_or(0),
        thinking_tokens: u.get("thoughtTokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: u
            .get("cachedReadTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: u.get("totalTokens").and_then(Value::as_u64).unwrap_or(0),
    }
}

fn extract_chunk_text(update: &Value) -> Option<String> {
    update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_tool_output(update: &Value) -> Option<Value> {
    if let Some(raw) = update.get("rawOutput").filter(|v| !v.is_null()) {
        return Some(raw.clone());
    }
    let arr = update.get("content")?.as_array()?;
    let mut texts = Vec::new();
    for entry in arr {
        if let Some(text) = entry
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
        {
            if !text.is_empty() {
                texts.push(text);
            }
        } else if let Some(text) = entry.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(Value::String(texts.join("\n")))
    }
}
