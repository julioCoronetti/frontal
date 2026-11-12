use crate::ui::app::{App, AppAction};
use crate::ui::modal::{DiffModal, HotRespawnModal, Modal, ModalAction, ModelEffortModal};

impl App {
    pub fn dispatch_modal_action(&mut self, action: ModalAction) -> Option<Option<AppAction>> {
        match action {
            ModalAction::Close => Some(None),
            ModalAction::ApproveDanger => {
                self.modals.pop();
                self.pending_danger = None;
                self.status_text = String::from("Command approved");
                self.dirty = true;
                let queued = std::mem::take(&mut self.suspended_events);
                for ev in queued {
                    let envelope = crate::protocol::SessionEnvelope {
                        session_id: self.active_session.clone(),
                        event: crate::protocol::EngineEvent::Stream(Box::new(ev)),
                    };
                    self.handle_engine_envelope(envelope);
                }
                Some(None)
            }
            ModalAction::RejectDanger => {
                self.modals.pop();
                let danger = self.pending_danger.take();
                self.suspended_events.clear();
                let _ = self.session_manager.interrupt(&self.active_session);
                if let Some(d) = danger {
                    self.cache.upsert_tool_call(
                        d.step_index,
                        &d.tool_name,
                        crate::protocol::StepState::Error,
                        None,
                        Some("Execution rejected by operator"),
                    );
                    self.cache.push_error(&format!(
                        "Security Guardian blocked destructive command: {}",
                        d.command
                    ));
                }
                self.waiting_for_response = false;
                self.status_text = String::from("Blocked destructive command");
                self.dirty = true;
                Some(Some(AppAction::Interrupt))
            }
            ModalAction::SelectModelEffort { model, effort } => {
                let backend = match self.modals.top() {
                    Some(Modal::ModelEffort(modal)) => modal.backend,
                    _ => self.user_config.backend,
                };
                self.modals.pop();
                self.status_text = format!("Model: {} ({}) · {}", model, effort, backend.as_str());
                self.user_config.backend = backend;
                self.user_config.model = model.clone();
                self.user_config.effort = effort.clone();
                let _ = self.user_config.save();
                Some(Some(AppAction::RespawnSession {
                    conversation_id: None,
                    model: Some(model),
                    effort: Some(effort),
                    mode: None,
                }))
            }
            ModalAction::Respawn {
                conversation_id,
                model,
                effort,
                mode,
            } => {
                self.modals.pop();
                Some(Some(AppAction::RespawnSession {
                    conversation_id,
                    model,
                    effort,
                    mode,
                }))
            }
            ModalAction::SwitchTab(idx) => {
                self.modals.pop();
                if self.active_tab_index != idx {
                    self.switch_tab(idx);
                }
                Some(Some(AppAction::SwitchTab(idx)))
            }
            ModalAction::ExecuteCommand(cmd) => {
                self.modals.pop();
                match cmd.as_str() {
                    "new_tab" => Some(Some(AppAction::NewTab)),
                    "close_tab" => Some(Some(AppAction::CloseTab(self.active_tab_index))),
                    "select_model" => {
                        let mut modal =
                            ModelEffortModal::with_models(self.available_models.clone());
                        modal.set_opencode_models(self.opencode_models.clone());
                        let modal = modal.with_initial_selection(
                            Some(self.active_model_name()),
                            Some(self.active_effort_name()),
                        );
                        self.modals.push(Modal::ModelEffort(modal));
                        Some(None)
                    }
                    "hot_respawn" => {
                        let conv_id = self
                            .session_manager
                            .get_metadata(&self.active_session)
                            .and_then(|m| m.conversation_id.clone());
                        let model = Some(self.active_model_name());
                        let effort = Some(self.active_effort_name());
                        let mode = Some(self.active_mode_name());
                        self.modals.push(Modal::HotRespawn(HotRespawnModal::new(
                            conv_id, model, effort, mode,
                        )));
                        Some(None)
                    }
                    "resume_session" => {
                        self.open_resume_modal(None);
                        Some(None)
                    }
                    "rename_session" => {
                        self.editor.insert_str("/rename ");
                        self.dirty = true;
                        Some(None)
                    }
                    "view_diff" => {
                        self.modals
                            .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
                        Some(None)
                    }
                    "view_usage" => {
                        let modal = if self.backend == crate::config::BackendKind::OpenCode {
                            crate::ui::modal::UsageModal::load_opencode(
                                &self.active_model_name(),
                                self.last_turn_usage,
                                self.accumulated_usage,
                            )
                        } else {
                            crate::ui::modal::UsageModal::load(!self.history.is_in_memory())
                        };
                        self.modals.push(Modal::Usage(modal));
                        Some(None)
                    }
                    "clear" => {
                        self.cache.clear();
                        self.viewport.clear_selection();
                        self.viewport.scroll_to_bottom();
                        self.last_turn_usage = None;
                        self.accumulated_usage = crate::protocol::TokenUsage::default();
                        self.status_text = String::from("Cleared");
                        Some(None)
                    }
                    "interrupt" => {
                        let _ = self.session_manager.interrupt_active();
                        self.status_text = String::from("Interrupted");
                        self.waiting_for_response = false;
                        Some(Some(AppAction::Interrupt))
                    }
                    "quit" => {
                        self.should_quit = true;
                        Some(Some(AppAction::Quit))
                    }
                    _ => Some(None),
                }
            }
            ModalAction::None => None,
        }
    }
}
