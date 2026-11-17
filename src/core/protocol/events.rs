use serde::{Deserialize, Serialize};

use super::SessionId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageContent {
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInputEvent {
    pub event: String,
    pub message: UserMessageContent,
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

impl TokenUsage {
    pub fn effective_total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                .saturating_add(self.output_tokens)
                .saturating_add(self.thinking_tokens)
                .saturating_add(self.cache_read_tokens)
        }
    }

    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.thinking_tokens = self.thinking_tokens.saturating_add(other.thinking_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.effective_total());
    }

    pub fn cache_hit_percentage(&self) -> f64 {
        let total_input = self.input_tokens.saturating_add(self.cache_read_tokens);
        if total_input == 0 {
            0.0
        } else {
            ((self.cache_read_tokens as f64 / total_input as f64) * 100.0).clamp(0.0, 100.0)
        }
    }
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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "max_context_tokens", alias = "context_length")]
    pub context_window: Option<u64>,
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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "max_context_tokens", alias = "context_length")]
    pub context_window: Option<u64>,
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
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "max_context_tokens", alias = "context_length")]
    pub context_window: Option<u64>,
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
