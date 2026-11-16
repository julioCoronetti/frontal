use super::{
    media_uri_to_path, ErrorEvent, PromptPayload, ProtocolError, StreamEvent, UserInputEvent,
    UserMessageContent,
};

pub struct AgyStreamParser;

impl AgyStreamParser {
    pub fn parse_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed.starts_with("error:") {
            return Ok(Some(StreamEvent::Error(ErrorEvent {
                error: Some(trimmed.to_string()),
                message: None,
            })));
        }
        let val: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;
        match serde_json::from_value::<StreamEvent>(val) {
            Ok(event) => Ok(Some(event)),
            Err(_) => Ok(Some(StreamEvent::Unknown)),
        }
    }
}

pub fn parse_stream_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
    AgyStreamParser::parse_line(line)
}

pub fn format_user_message(content: &str) -> Result<String, ProtocolError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(ProtocolError::EmptyPrompt);
    }
    let event = UserInputEvent {
        event: "user".to_string(),
        message: UserMessageContent {
            content: content.to_string(),
        },
    };
    serde_json::to_string(&event)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| ProtocolError::Serialization(e.to_string()))
}

pub fn format_user_payload(payload: &PromptPayload) -> Result<String, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::EmptyPrompt);
    }
    if payload.media.is_empty() {
        return format_user_message(&payload.text);
    }
    let mut parts = Vec::new();
    let trimmed = payload.text.trim();
    if !trimmed.is_empty() {
        parts.push(payload.text.trim_end().to_string());
    }
    for item in &payload.media {
        let path = media_uri_to_path(&item.uri);
        parts.push(format!("[Attached Media: {} ({})]", path, item.mime_type));
    }
    format_user_message(&parts.join("\n"))
}
