use crate::protocol::{EngineEvent, SessionEnvelope, StepState, StreamEvent};
use crate::ui::app::App;
use crate::ui::cache::BlockKind;
use crate::ui::notify::emit_sensory_notification;

impl App {
    pub fn handle_engine_envelope(&mut self, envelope: SessionEnvelope) {
        if envelope.session_id == self.active_session {
            match envelope.event {
                EngineEvent::Stream(boxed_ev) => match *boxed_ev {
                    StreamEvent::Init(ev) => {
                        if let Some(conv_id) = ev.conversation_id {
                            self.status_text = format!("Conversation: {}", conv_id);
                        }
                        self.dirty = true;
                    }
                    StreamEvent::StepUpdate(update) => {
                        if let Some(data) = update.step_update {
                            if let Some(usage) = data.usage {
                                self.last_turn_usage = Some(usage);
                            }
                            if let Some(ref delta) = data.text_delta {
                                self.waiting_for_response = false;
                                self.cache
                                    .append_streaming_delta(BlockKind::Assistant, delta);
                                self.status_text = String::from("Streaming...");
                                self.dirty = true;
                            }
                            if let Some(ref thought) = data.thought {
                                self.waiting_for_response = false;
                                self.cache
                                    .append_streaming_delta(BlockKind::Thought, thought);
                                self.status_text = String::from("Thinking...");
                                self.dirty = true;
                            }
                            if let Some(tool_name) = data.resolved_tool_name() {
                                self.waiting_for_response = false;
                                let state = data.state.unwrap_or(StepState::Active);
                                let args = data
                                    .tool_info
                                    .as_ref()
                                    .and_then(|info| info.parameters_text());
                                let output =
                                    data.tool_info.as_ref().and_then(|info| info.output_text());
                                self.cache.upsert_tool_call(
                                    data.step_index,
                                    tool_name,
                                    state,
                                    args.as_deref(),
                                    output.as_deref(),
                                );
                                self.status_text = format!("Tool: {}", tool_name);
                                self.dirty = true;
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
                        self.last_turn_usage = usage;
                        let meta = crate::ui::cache::TurnMeta {
                            mode: self.active_mode_name(),
                            model: self.active_model_name(),
                            duration_seconds: duration,
                            output_tokens: tokens,
                        };
                        self.cache
                            .consolidate_active_turn_with_meta(fallback, Some(meta));
                        self.status_text = String::from("Ready");
                        self.dirty = true;
                    }
                    StreamEvent::Error(err) => {
                        self.waiting_for_response = false;
                        self.last_turn_usage = None;
                        let msg = err
                            .error
                            .or(err.message)
                            .unwrap_or_else(|| String::from("Unknown error"));
                        self.cache.push_error(&msg);
                        self.status_text = String::from("Error received");
                        self.dirty = true;
                    }
                    _ => {}
                },
                EngineEvent::Stderr(line) => {
                    self.status_text = format!("stderr: {}", line);
                    self.dirty = true;
                }
                EngineEvent::ProtocolError { message, .. } => {
                    self.waiting_for_response = false;
                    self.cache
                        .push_error(&format!("Protocol Error: {}", message));
                    self.status_text = String::from("Protocol Error");
                    self.dirty = true;
                }
                EngineEvent::ProcessExited { code, .. } => {
                    self.waiting_for_response = false;
                    self.status_text = format!("Process exited: {:?}", code);
                    self.dirty = true;
                }
            }
            if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                tab.waiting_for_response = self.waiting_for_response;
                tab.status_text = self.status_text.clone();
            }
        } else if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|t| t.session_id == envelope.session_id)
        {
            let (bg_mode, bg_model) = {
                let meta = self.session_manager.get_metadata(&envelope.session_id);
                (
                    meta.and_then(|m| m.mode.clone())
                        .unwrap_or_else(|| crate::config::DEFAULT_MODE.to_string()),
                    meta.and_then(|m| m.model.clone())
                        .unwrap_or_else(|| crate::config::DEFAULT_MODEL.to_string()),
                )
            };
            match envelope.event {
                EngineEvent::Stream(boxed_ev) => {
                    let turn_completed =
                        tab.handle_background_stream_event(&boxed_ev, &bg_mode, &bg_model);
                    if turn_completed && !self.clipboard.is_isolated() {
                        emit_sensory_notification(
                            "Frontal Session Completed",
                            &format!("Turn completed in {}", tab.title),
                        );
                    }
                    self.dirty = true;
                }
                EngineEvent::ProtocolError { message, .. } => {
                    tab.waiting_for_response = false;
                    tab.cache
                        .push_error(&format!("Protocol Error: {}", message));
                    tab.status_text = String::from("Protocol Error");
                    tab.has_unread = true;
                    self.dirty = true;
                }
                EngineEvent::Stderr(line) => {
                    tab.status_text = format!("stderr: {}", line);
                    self.dirty = true;
                }
                EngineEvent::ProcessExited { .. } => {
                    tab.waiting_for_response = false;
                    tab.status_text = String::from("Process exited");
                    self.dirty = true;
                }
            }
        }
    }
}
