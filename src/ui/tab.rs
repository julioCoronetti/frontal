use crate::config::BackendKind;
use crate::protocol::{ResultStatus, SessionId, StepState, StreamEvent, TokenUsage};
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
    pub backend: BackendKind,
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
    pub in_flight_usage: Option<TokenUsage>,
    pub accumulated_usage: TokenUsage,
    pub served_model: Option<String>,
    pub context_window_hint: Option<u64>,
    pub steps_seen: u64,
    pub last_step_index: Option<u64>,
    pub turns_completed: u64,
    pub last_turn_duration_seconds: Option<f64>,
    pub pending_danger: Option<crate::security::DangerAssessment>,
    pub suspended_events: Vec<StreamEvent>,
    pub queued_prompts: std::collections::VecDeque<String>,
    pub persisted_block_id: u64,
}

impl TabState {
    pub fn new(session_id: SessionId, title: String) -> Self {
        Self {
            session_id,
            backend: BackendKind::Agy,
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
            in_flight_usage: None,
            accumulated_usage: TokenUsage::default(),
            served_model: None,
            context_window_hint: None,
            steps_seen: 0,
            last_step_index: None,
            turns_completed: 0,
            last_turn_duration_seconds: None,
            pending_danger: None,
            suspended_events: Vec::new(),
            queued_prompts: std::collections::VecDeque::new(),
            persisted_block_id: 0,
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
                self.last_step_index = None;
                if let Some(init) = &ev.init {
                    self.update_context_hint(init.context_window);
                    if let Some(model) = &init.model {
                        self.served_model = Some(model.clone());
                    }
                }
                if let Some(conv_id) = &ev.conversation_id {
                    self.status_text = format!("Conversation: {}", conv_id);
                }
                if !is_active {
                    self.has_unread = true;
                }
            }
            StreamEvent::StepUpdate(update) => {
                if let Some(data) = &update.step_update {
                    self.update_context_hint(data.context_window);
                    if let Some(model) = &data.model {
                        self.served_model = Some(model.clone());
                    }
                    if let Some(index) = data.step_index {
                        let advanced = self
                            .last_step_index
                            .map(|previous| index > previous)
                            .unwrap_or(true);
                        if advanced {
                            self.last_step_index = Some(index);
                            self.steps_seen = self.steps_seen.saturating_add(1);
                        }
                    }
                    if let Some(usage) = &data.usage {
                        let in_flight =
                            self.in_flight_usage.get_or_insert_with(TokenUsage::default);
                        in_flight.accumulate(usage);
                        self.last_turn_usage = self.in_flight_usage;
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
                if let Some(result) = &res.result {
                    self.update_context_hint(result.context_window);
                    if let Some(model) = &result.model {
                        self.served_model = Some(model.clone());
                    }
                }
                let turn_usage = res.result.as_ref().and_then(|r| r.usage);
                let consolidated = turn_usage.or(self.in_flight_usage.take());
                if let Some(usage) = consolidated {
                    self.accumulated_usage.accumulate(&usage);
                    self.last_turn_usage = Some(usage);
                }
                let tokens = self.last_turn_usage.map(|u| u.output_tokens);
                self.last_step_index = None;
                self.turns_completed = self.turns_completed.saturating_add(1);
                if let Some(elapsed) = duration {
                    self.last_turn_duration_seconds = Some(elapsed);
                }
                let meta = crate::ui::cache::TurnMeta {
                    mode: mode.to_string(),
                    model: model.to_string(),
                    duration_seconds: duration,
                    output_tokens: tokens,
                };
                self.cache
                    .consolidate_active_turn_with_meta(fallback, Some(meta));
                if res.result.as_ref().and_then(|r| r.status.as_ref()) == Some(&ResultStatus::Error)
                {
                    if let Some(err) = res.result.as_ref().and_then(|r| r.error.as_deref()) {
                        self.cache.push_error(err);
                    }
                    self.status_text = String::from("Error received");
                } else {
                    self.status_text = String::from("Ready");
                }
                if !is_active {
                    self.has_unread = true;
                }
                outcome = Some(StreamOutcome::TurnCompleted { is_result: true });
            }
            StreamEvent::Error(err) => {
                self.waiting_for_response = false;
                self.in_flight_usage = None;
                self.last_step_index = None;
                let msg = err
                    .error
                    .as_deref()
                    .or(err.message.as_deref())
                    .unwrap_or("Unknown error");
                self.cache.push_error(msg);
                self.cache.consolidate_active_turn_with_meta(None, None);
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

    fn update_context_hint(&mut self, hint: Option<u64>) {
        if let Some(tokens) = hint.filter(|tokens| *tokens > 0) {
            self.context_window_hint = Some(tokens);
        }
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
