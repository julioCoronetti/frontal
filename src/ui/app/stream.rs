use crate::infra::history::StoredMessage;
use crate::protocol::{EngineEvent, SessionEnvelope, StreamEvent};
use crate::ui::app::App;
use crate::ui::cache::BlockKind;
use crate::ui::modal::{DangerConfirmationModal, Modal};
use crate::ui::notify::emit_sensory_notification;
use crate::ui::tab::StreamOutcome;

impl App {
    pub fn handle_prompt_failure(
        &mut self,
        session_id: &crate::protocol::SessionId,
        message: &str,
    ) {
        let is_active = &self.active_session == session_id;
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| &tab.session_id == session_id)
        {
            tab.waiting_for_response = false;
            tab.turn_started_at = None;
            tab.queued_prompts.clear();
            tab.cache.consolidate_active_turn_with_meta(None, None);
            tab.cache.push_error(message);
            tab.status_text = String::from("Error received");
            if !is_active {
                tab.has_unread = true;
            }
        }
        self.dirty = true;
    }

    pub fn handle_engine_envelope(&mut self, envelope: SessionEnvelope) {
        let tab_idx = match self
            .tabs
            .iter()
            .position(|t| t.session_id == envelope.session_id)
        {
            Some(idx) => idx,
            None => return,
        };
        let is_active = tab_idx == self.active_tab_index;

        if let EngineEvent::Stream(ref boxed_ev) = envelope.event {
            if let StreamEvent::Init(ref ev) = **boxed_ev {
                if let Some(ref conv_id) = ev.conversation_id {
                    self.session_manager.set_session_metadata(
                        &envelope.session_id,
                        Some(conv_id.clone()),
                        None,
                        None,
                        None,
                    );
                    self.sync_session_to_history(&envelope.session_id);
                }
            } else if let StreamEvent::Result(ref res) = **boxed_ev {
                let conv_id = res
                    .conversation_id
                    .clone()
                    .or_else(|| res.result.as_ref().and_then(|r| r.conversation_id.clone()));
                if let Some(cid) = conv_id {
                    self.session_manager.set_session_metadata(
                        &envelope.session_id,
                        Some(cid.clone()),
                        None,
                        None,
                        None,
                    );
                    if let Some(agent_title) =
                        crate::ui::history::query_agent_conversation_title(&cid)
                    {
                        self.tabs[tab_idx].title = agent_title.clone();
                        self.history.rename_session(&cid, &agent_title);
                    }
                    self.sync_session_to_history(&envelope.session_id);
                }
            }
        }

        match envelope.event {
            EngineEvent::Stream(boxed_ev) => {
                let (mode, model) = {
                    let meta = self.session_manager.get_metadata(&envelope.session_id);
                    (
                        meta.as_ref()
                            .and_then(|m| m.mode.clone())
                            .unwrap_or_else(|| self.user_config.mode.clone()),
                        meta.as_ref()
                            .and_then(|m| m.model.clone())
                            .unwrap_or_else(|| self.user_config.model.clone()),
                    )
                };
                let outcome =
                    self.tabs[tab_idx].apply_stream_event(&boxed_ev, &mode, &model, is_active);
                let completed = matches!(&outcome, Some(StreamOutcome::TurnCompleted { .. }));
                match outcome {
                    Some(StreamOutcome::DangerRequired(danger)) => {
                        if is_active {
                            self.modals.push(Modal::DangerConfirmation(
                                DangerConfirmationModal::new(danger),
                            ));
                        }
                    }
                    Some(StreamOutcome::TurnCompleted { is_result }) => {
                        if let Some(next_prompt) = self.tabs[tab_idx].queued_prompts.pop_front() {
                            self.tabs[tab_idx].cache.push_user_message(&next_prompt);
                            self.tabs[tab_idx].waiting_for_response = true;
                            self.tabs[tab_idx].turn_started_at = Some(std::time::Instant::now());
                            self.tabs[tab_idx].status_text = format!(
                                "Processing queued prompt ({} left)",
                                self.tabs[tab_idx].queued_prompts.len()
                            );
                            self.pending_engine_prompts
                                .push_back((self.tabs[tab_idx].session_id.clone(), next_prompt));
                        }
                        if (is_result || !is_active) && !self.clipboard.is_isolated() {
                            emit_sensory_notification(
                                "Frontal Session Completed",
                                &format!("Turn completed in {}", self.tabs[tab_idx].title),
                            );
                        }
                    }
                    None => {}
                }
                if completed {
                    self.persist_completed_turn(tab_idx);
                }
                self.dirty = true;
            }
            EngineEvent::Stderr(line) => {
                self.tabs[tab_idx].status_text = format!("stderr: {}", line);
                self.dirty = true;
            }
            EngineEvent::ProtocolError { message, .. } => {
                self.tabs[tab_idx].waiting_for_response = false;
                self.tabs[tab_idx].pending_danger = None;
                self.tabs[tab_idx].suspended_events.clear();
                self.tabs[tab_idx].queued_prompts.clear();
                if is_active {
                    self.modals.remove_danger_modal();
                } else {
                    self.tabs[tab_idx].has_unread = true;
                }
                self.tabs[tab_idx]
                    .cache
                    .push_error(&format!("Protocol Error: {}", message));
                self.tabs[tab_idx].status_text = String::from("Protocol Error");
                self.dirty = true;
            }
            EngineEvent::ProcessExited { code, .. } => {
                self.tabs[tab_idx].waiting_for_response = false;
                self.tabs[tab_idx].pending_danger = None;
                self.tabs[tab_idx].suspended_events.clear();
                self.tabs[tab_idx].queued_prompts.clear();
                if is_active {
                    self.modals.remove_danger_modal();
                }
                self.tabs[tab_idx].status_text = format!("Process exited: {:?}", code);
                self.dirty = true;
            }
        }
    }

    fn persist_completed_turn(&mut self, tab_idx: usize) {
        let Some(conversation_id) = self
            .session_manager
            .get_metadata(&self.tabs[tab_idx].session_id)
            .and_then(|meta| meta.conversation_id.clone())
        else {
            return;
        };
        let last_persisted = self.tabs[tab_idx].persisted_block_id;
        let mut messages = Vec::new();
        let mut max_id = last_persisted;
        for block in self.tabs[tab_idx].cache.blocks() {
            if !block.consolidated || block.id <= last_persisted {
                continue;
            }
            max_id = max_id.max(block.id);
            let role = match &block.kind {
                BlockKind::User => "user",
                BlockKind::Assistant => "assistant",
                BlockKind::Thought => "thought",
                BlockKind::Tool { .. } => "tool",
                BlockKind::Error => continue,
            };
            let (tool_name, tool_args) = match &block.kind {
                BlockKind::Tool { name, .. } => (Some(name.clone()), block.tool_args.clone()),
                _ => (None, None),
            };
            let turn_meta = block.turn_meta.as_ref().and_then(|meta| {
                let mut meta = meta.clone();
                meta.model = crate::config::sanitize_model_id(&meta.model);
                serde_json::to_string(&meta).ok()
            });
            messages.push(StoredMessage {
                role: role.to_string(),
                content: block.content.clone(),
                tool_name,
                tool_args,
                step_index: block.step_index,
                turn_meta,
            });
        }
        let last_usage = self.tabs[tab_idx].last_turn_usage;
        if last_usage.is_some() || self.tabs[tab_idx].turns_completed > 0 {
            let model = self.tabs[tab_idx].served_model.clone().or_else(|| {
                self.session_manager
                    .get_metadata(&self.tabs[tab_idx].session_id)
                    .and_then(|meta| meta.model.clone())
            });
            let snapshot = crate::infra::history::TelemetrySnapshot {
                last: last_usage.unwrap_or_default(),
                accumulated: self.tabs[tab_idx].accumulated_usage,
                steps: self.tabs[tab_idx].steps_seen,
                turns: self.tabs[tab_idx].turns_completed,
                duration_millis: self.tabs[tab_idx]
                    .last_turn_duration_seconds
                    .map(|seconds| (seconds * 1000.0).round() as u64),
                context_window: self.tabs[tab_idx].context_window_hint,
                model: model.filter(|model| !model.trim().is_empty()),
            };
            self.history.record_telemetry(&conversation_id, snapshot);
        }
        if !messages.is_empty() {
            self.history.append_messages(conversation_id, messages);
        }
        self.tabs[tab_idx].persisted_block_id = max_id;
    }
}
