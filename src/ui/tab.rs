use crate::protocol::{SessionId, StepState, StreamEvent, TokenUsage};
use crate::ui::cache::{BlockKind, RenderCache};
use crate::ui::editor::{EditorSnapshot, PromptEditor};
use crate::ui::viewport::ConversationViewport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamOutcome {
    DangerRequired(crate::security::DangerAssessment),
    TurnCompleted { is_result: bool },
}

pub struct TabState {
    pub session_id: SessionId,
    pub title: String,
    pub cache: RenderCache,
    pub viewport: ConversationViewport,
    pub editor: PromptEditor,
    pub draft_snapshot: Option<EditorSnapshot>,
    pub waiting_for_response: bool,
    pub status_text: String,
    pub has_unread: bool,
    pub turn_started_at: Option<std::time::Instant>,
    pub last_turn_usage: Option<TokenUsage>,
    pub accumulated_usage: TokenUsage,
    pub pending_danger: Option<crate::security::DangerAssessment>,
    pub suspended_events: Vec<StreamEvent>,
    pub queued_prompts: std::collections::VecDeque<String>,
}

impl TabState {
    pub fn new(session_id: SessionId, title: String) -> Self {
        Self {
            session_id,
            title,
            cache: RenderCache::new(),
            viewport: ConversationViewport::new(),
            editor: PromptEditor::new(),
            draft_snapshot: None,
            waiting_for_response: false,
            status_text: String::from("Ready"),
            has_unread: false,
            turn_started_at: None,
            last_turn_usage: None,
            accumulated_usage: TokenUsage::default(),
            pending_danger: None,
            suspended_events: Vec::new(),
            queued_prompts: std::collections::VecDeque::new(),
        }
    }

    pub fn apply_stream_event(
        &mut self,
        event: &StreamEvent,
        mode: &str,
        model: &str,
        is_active: bool,
    ) -> Option<StreamOutcome> {
        if self.pending_danger.is_some() {
            self.suspended_events.push(event.clone());
            return None;
        }
        let mut outcome = None;
        match event {
            StreamEvent::Init(ev) => {
                if let Some(conv_id) = &ev.conversation_id {
                    self.status_text = format!("Conversation: {}", conv_id);
                }
                if !is_active {
                    self.has_unread = true;
                }
            }
            StreamEvent::StepUpdate(update) => {
                if let Some(data) = &update.step_update {
                    if let Some(usage) = &data.usage {
                        self.last_turn_usage = Some(*usage);
                    }
                    if let Some(delta) = &data.text_delta {
                        self.waiting_for_response = false;
                        self.cache
                            .append_streaming_delta(BlockKind::Assistant, delta);
                        self.status_text = String::from("Streaming...");
                        if !is_active {
                            self.has_unread = true;
                        }
                    }
                    if let Some(thought) = &data.thought {
                        self.waiting_for_response = false;
                        self.cache
                            .append_streaming_delta(BlockKind::Thought, thought);
                        self.status_text = String::from("Thinking...");
                        if !is_active {
                            self.has_unread = true;
                        }
                    }
                    if let Some(tool_name) = data.resolved_tool_name() {
                        self.waiting_for_response = false;
                        let state = data.state.unwrap_or(StepState::Active);
                        let args = data
                            .tool_info
                            .as_ref()
                            .and_then(|info| info.parameters_text());
                        let output = data.tool_info.as_ref().and_then(|info| info.output_text());
                        self.cache.upsert_tool_call(
                            data.step_index,
                            tool_name,
                            state,
                            args.as_deref(),
                            output.as_deref(),
                        );
                        if state == StepState::Active && output.is_none() {
                            let params_val =
                                data.tool_info.as_ref().and_then(|i| i.parameters.as_ref());
                            if let Some(danger) = crate::security::evaluate_tool_call(
                                tool_name,
                                data.step_index,
                                params_val,
                                args.as_deref(),
                            ) {
                                self.pending_danger = Some(danger.clone());
                                self.status_text = String::from("Security confirmation required");
                                if !is_active {
                                    self.has_unread = true;
                                }
                                return Some(StreamOutcome::DangerRequired(danger));
                            }
                        }
                        self.status_text = format!("Tool: {}", tool_name);
                        if !is_active {
                            self.has_unread = true;
                        }
                    }
                }
            }
            StreamEvent::Result(res) => {
                self.waiting_for_response = false;
                let fallback = res.result.as_ref().and_then(|r| r.response.as_deref());
                let duration = res
                    .result
                    .as_ref()
                    .and_then(|r| r.duration_seconds)
                    .or_else(|| {
                        self.turn_started_at
                            .take()
                            .map(|t| t.elapsed().as_secs_f64())
                    });
                let usage = res
                    .result
                    .as_ref()
                    .and_then(|r| r.usage)
                    .or(self.last_turn_usage);
                let tokens = usage.as_ref().map(|u| u.output_tokens);
                if let Some(ref u) = usage {
                    self.accumulated_usage.accumulate(u);
                }
                self.last_turn_usage = usage;
                let meta = crate::ui::cache::TurnMeta {
                    mode: mode.to_string(),
                    model: model.to_string(),
                    duration_seconds: duration,
                    output_tokens: tokens,
                };
                self.cache
                    .consolidate_active_turn_with_meta(fallback, Some(meta));
                self.status_text = String::from("Ready");
                if !is_active {
                    self.has_unread = true;
                }
                outcome = Some(StreamOutcome::TurnCompleted { is_result: true });
            }
            StreamEvent::Error(err) => {
                self.waiting_for_response = false;
                self.last_turn_usage = None;
                let msg = err
                    .error
                    .as_deref()
                    .or(err.message.as_deref())
                    .unwrap_or("Unknown error");
                self.cache.push_error(msg);
                self.status_text = String::from("Error received");
                if !is_active {
                    self.has_unread = true;
                }
                outcome = Some(StreamOutcome::TurnCompleted { is_result: false });
            }
            _ => {}
        }
        outcome
    }

    pub fn handle_background_stream_event(
        &mut self,
        event: &StreamEvent,
        mode: &str,
        model: &str,
    ) -> bool {
        matches!(
            self.apply_stream_event(event, mode, model, false),
            Some(StreamOutcome::TurnCompleted { .. })
        )
    }
}
