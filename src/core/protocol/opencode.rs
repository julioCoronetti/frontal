use serde_json::Value;

use super::{
    ErrorEvent, InitEvent, ProtocolError, StepState, StepType, StepUpdateData, StepUpdateEvent,
    StreamEvent, TokenUsage, ToolInfo,
};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub struct OpenCodeStreamParser;

impl OpenCodeStreamParser {
    pub fn matches(line: &str) -> bool {
        let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
            return false;
        };
        match val.get("type").and_then(Value::as_str) {
            Some("step_start") | Some("text") | Some("reasoning") | Some("thought")
            | Some("tool_use") | Some("step_finish") | Some("error") => true,
            _ => val.get("sessionID").is_some() || val.get("part").is_some_and(Value::is_object),
        }
    }

    pub fn parse_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let val: Value = serde_json::from_str(trimmed)
            .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;
        let Some(kind) = val.get("type").and_then(Value::as_str) else {
            return Ok(Some(StreamEvent::Unknown));
        };
        let conversation_id = extract_opencode_session_id(&val);
        let part = val.get("part").unwrap_or(&Value::Null);
        match kind {
            "step_start" => Ok(Some(StreamEvent::Init(InitEvent {
                conversation_id,
                init: None,
            }))),
            "text" => Ok(text_delta_event(
                part,
                &val,
                conversation_id,
                StepType::AgentResponse,
            )),
            "reasoning" | "thought" => Ok(thought_delta_event(part, &val, conversation_id)),
            "tool_use" => Ok(Some(tool_event(part, conversation_id))),
            "step_finish" => Ok(step_finish_event(part, &val, conversation_id)),
            "error" => Ok(Some(StreamEvent::Error(ErrorEvent {
                error: error_message(&val),
                message: None,
            }))),
            _ => Ok(Some(StreamEvent::Unknown)),
        }
    }
}

pub fn extract_opencode_session_id(val: &Value) -> Option<String> {
    val.get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            val.get("part")
                .and_then(|p| p.get("sessionID"))
                .and_then(Value::as_str)
        })
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

pub fn fnv1a_64(input: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn step_update(conversation_id: Option<String>, data: StepUpdateData) -> StreamEvent {
    StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id,
        step_update: Some(data),
    })
}

fn text_of(part: &Value, val: &Value) -> Option<String> {
    part.get("text")
        .and_then(Value::as_str)
        .or_else(|| val.get("text").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn text_delta_event(
    part: &Value,
    val: &Value,
    conversation_id: Option<String>,
    step_type: StepType,
) -> Option<StreamEvent> {
    let text = text_of(part, val)?;
    Some(step_update(
        conversation_id,
        StepUpdateData {
            step_type: Some(step_type),
            text_delta: Some(text),
            ..Default::default()
        },
    ))
}

fn thought_delta_event(
    part: &Value,
    val: &Value,
    conversation_id: Option<String>,
) -> Option<StreamEvent> {
    let text = text_of(part, val)?;
    Some(step_update(
        conversation_id,
        StepUpdateData {
            step_type: Some(StepType::Thought),
            thought: Some(text),
            ..Default::default()
        },
    ))
}

fn tool_event(part: &Value, conversation_id: Option<String>) -> StreamEvent {
    let name = part
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown_tool");
    let step_index = part
        .get("callID")
        .and_then(Value::as_str)
        .or_else(|| part.get("id").and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty())
        .map(fnv1a_64);
    let status = part
        .get("state")
        .and_then(|s| s.get("status"))
        .and_then(Value::as_str)
        .or_else(|| part.get("status").and_then(Value::as_str))
        .unwrap_or("running");
    let state = match status {
        "completed" => StepState::Done,
        "error" => StepState::Error,
        "pending" | "running" => StepState::Active,
        _ => StepState::Other,
    };
    let state_value = part.get("state").unwrap_or(&Value::Null);
    let tool_info = ToolInfo {
        name: Some(name.to_string()),
        parameters: state_value.get("input").cloned(),
        output: state_value
            .get("output")
            .cloned()
            .or_else(|| state_value.get("error").cloned()),
    };
    step_update(
        conversation_id,
        StepUpdateData {
            step_index,
            state: Some(state),
            step_type: Some(StepType::Tool),
            tool_name: Some(name.to_string()),
            tool_info: Some(tool_info),
            ..Default::default()
        },
    )
}

fn step_finish_event(
    part: &Value,
    val: &Value,
    conversation_id: Option<String>,
) -> Option<StreamEvent> {
    let tokens = part.get("tokens").or_else(|| val.get("tokens"))?;
    let usage = TokenUsage {
        input_tokens: tokens.get("input").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: tokens.get("output").and_then(Value::as_u64).unwrap_or(0),
        thinking_tokens: tokens.get("reasoning").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: tokens
            .get("cache")
            .and_then(|c| c.get("read"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: 0,
    };
    Some(step_update(
        conversation_id,
        StepUpdateData {
            step_type: Some(StepType::AgentResponse),
            usage: Some(usage),
            ..Default::default()
        },
    ))
}

fn error_message(val: &Value) -> Option<String> {
    let err = val.get("error");
    err.and_then(|e| e.get("data"))
        .and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        .or_else(|| err.and_then(|e| e.get("message")).and_then(Value::as_str))
        .or_else(|| err.and_then(|e| e.get("type")).and_then(Value::as_str))
        .or_else(|| err.and_then(|e| e.get("name")).and_then(Value::as_str))
        .or_else(|| err.and_then(Value::as_str))
        .or_else(|| val.get("message").and_then(Value::as_str))
        .map(str::to_string)
}
