use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        if let Ok(content) = fs::read_to_string("/proc/sys/kernel/random/uuid") {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Self(trimmed.to_string());
            }
        }
        let fallback_path = Path::new("/dev/urandom");
        if let Ok(mut file) = fs::File::open(fallback_path) {
            let mut b = [0u8; 16];
            if file.read_exact(&mut b).is_ok() {
                b[6] = (b[6] & 0x0f) | 0x40;
                b[8] = (b[8] & 0x3f) | 0x80;
                let formatted = format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
                );
                return Self(formatted);
            }
        }
        Self("00000000-0000-4000-8000-000000000000".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyPrompt,
    SlashCommandNotAllowed(String),
    Serialization(String),
    Deserialization(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPrompt => write!(f, "Prompt content cannot be empty"),
            Self::SlashCommandNotAllowed(cmd) => {
                write!(
                    f,
                    "Slash command '{}' must be handled locally by the harness",
                    cmd
                )
            }
            Self::Serialization(err) => write!(f, "Serialization error: {}", err),
            Self::Deserialization(err) => write!(f, "Deserialization error: {}", err),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageContent {
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInputEvent {
    pub event: String,
    pub message: UserMessageContent,
}

pub fn format_user_message(content: &str) -> Result<String, ProtocolError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(ProtocolError::EmptyPrompt);
    }
    if trimmed.starts_with('/') {
        return Err(ProtocolError::SlashCommandNotAllowed(trimmed.to_string()));
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepType {
    UserInput,
    AgentResponse,
    Thought,
    Tool,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StepState {
    Active,
    Done,
    #[serde(alias = "FAILED")]
    Error,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ResultStatus {
    Success,
    Error,
    #[serde(other)]
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub thinking_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
}

impl ToolInfo {
    pub fn output_text(&self) -> Option<String> {
        self.output.as_ref().map(|val| match val {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    pub fn parameters_text(&self) -> Option<String> {
        self.parameters.as_ref().and_then(|val| {
            if val.is_null() {
                None
            } else {
                match val {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Object(map) => {
                        let pairs: Vec<String> =
                            map.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                        Some(pairs.join(", "))
                    }
                    other => Some(other.to_string()),
                }
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitData {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitEvent {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub init: Option<InitData>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepUpdateData {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub step_index: Option<u64>,
    #[serde(default)]
    pub state: Option<StepState>,
    #[serde(default)]
    pub step_type: Option<StepType>,
    #[serde(default)]
    pub text_delta: Option<String>,
    #[serde(default)]
    pub thought: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_info: Option<ToolInfo>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

impl StepUpdateData {
    pub fn resolved_tool_name(&self) -> Option<&str> {
        self.tool_name
            .as_deref()
            .or_else(|| self.tool_info.as_ref().and_then(|t| t.name.as_deref()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepUpdateEvent {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub step_update: Option<StepUpdateData>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultData {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub status: Option<ResultStatus>,
    #[serde(default)]
    pub response: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultEvent {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub result: Option<ResultData>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEvent {
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum StreamEvent {
    #[serde(rename = "init")]
    Init(InitEvent),
    #[serde(rename = "step_update")]
    StepUpdate(StepUpdateEvent),
    #[serde(rename = "result")]
    Result(ResultEvent),
    #[serde(rename = "error")]
    Error(ErrorEvent),
    #[serde(other)]
    Unknown,
}

impl StreamEvent {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::Init(ev) => ev.conversation_id.as_deref(),
            Self::StepUpdate(ev) => ev.conversation_id.as_deref().or_else(|| {
                ev.step_update
                    .as_ref()
                    .and_then(|u| u.conversation_id.as_deref())
            }),
            Self::Result(ev) => ev.conversation_id.as_deref().or_else(|| {
                ev.result
                    .as_ref()
                    .and_then(|r| r.conversation_id.as_deref())
            }),
            _ => None,
        }
    }

    pub fn text_delta(&self) -> Option<&str> {
        match self {
            Self::StepUpdate(ev) => ev
                .step_update
                .as_ref()
                .and_then(|u| u.text_delta.as_deref()),
            _ => None,
        }
    }

    pub fn thought_delta(&self) -> Option<&str> {
        match self {
            Self::StepUpdate(ev) => ev.step_update.as_ref().and_then(|u| u.thought.as_deref()),
            _ => None,
        }
    }

    pub fn is_result(&self) -> bool {
        matches!(self, Self::Result(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    Stream(Box<StreamEvent>),
    Stderr(String),
    ProtocolError {
        message: String,
        raw_preview: Option<String>,
    },
    ProcessExited {
        code: Option<i32>,
        stderr_tail: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionEnvelope {
    pub session_id: SessionId,
    pub event: EngineEvent,
}

pub fn parse_stream_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
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
    let val: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| ProtocolError::Deserialization(e.to_string()))?;
    match serde_json::from_value::<StreamEvent>(val) {
        Ok(event) => Ok(Some(event)),
        Err(_) => Ok(Some(StreamEvent::Unknown)),
    }
}
