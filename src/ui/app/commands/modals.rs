use crate::ui::app::{App, AppAction};
use crate::ui::modal::{Modal, ModalAction, ModelEffortModal};

impl App {
    pub fn dispatch_modal_action(&mut self, action: ModalAction) -> Option<Option<AppAction>> {
        match action {
            ModalAction::Close => Some(None),
            ModalAction::ApproveDanger => {
                self.modals.pop();
                let session_id = self.active_session_id().clone();
                let queued = {
                    let tab = self.active_tab_mut();
                    tab.pending_danger = None;
                    tab.status_text = String::from("Command approved");
                    std::mem::take(&mut tab.suspended_events)
                };
                self.dirty = true;
                for ev in queued {
                    let envelope = crate::protocol::SessionEnvelope {
                        session_id: session_id.clone(),
                        event: crate::protocol::EngineEvent::Stream(Box::new(ev)),
                    };
                    self.handle_engine_envelope(envelope);
                }
                Some(None)
            }
            ModalAction::RejectDanger => {
                self.modals.pop();
                let tab = self.active_tab_mut();
                let danger = tab.pending_danger.take();
                if let Some(d) = danger {
                    tab.cache.upsert_tool_call(
                        d.step_index,
                        &d.tool_name,
                        crate::protocol::StepState::Error,
                        None,
                        Some("Execution rejected by operator"),
                    );
                    tab.cache.push_error(&format!(
                        "Security Guardian blocked destructive command: {}",
                        d.command
                    ));
                }
                tab.cache.consolidate_active_turn_with_meta(None, None);
                tab.transition_turn(crate::ui::tab::TurnEvent::InterruptRequested);
                tab.status_text = String::from("Blocked destructive command");
                self.dirty = true;
                Some(Some(AppAction::Interrupt))
            }
            ModalAction::SelectModelEffort { model, effort } => {
                let backend = match self.modals.top() {
                    Some(Modal::ModelEffort(modal)) => modal.backend,
                    _ => self.user_config.backend,
                };
                self.modals.pop();
                self.active_tab_mut().status_text = format!(
                    "Model: {} ({}) · {}",
                    crate::ui::labels::model_display_name(&model),
                    effort,
                    backend.as_str()
                );
                self.user_config.backend = backend;
                self.user_config.model = model.clone();
                self.user_config.effort = effort.clone();
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
                if conversation_id.is_none() {
                    if let Some(ref m) = mode {
                        let tab = self.active_tab_mut();
                        tab.status_text = format!("Mode: {} · {}", m, tab.backend.as_str());
                        self.dirty = true;
                    }
                }
                Some(Some(AppAction::RespawnSession {
                    conversation_id,
                    model,
                    effort,
                    mode,
                }))
            }
            ModalAction::SwitchTab(idx) => {
                self.modals.pop();
                Some(Some(AppAction::SwitchTab(idx)))
            }
            ModalAction::DeleteSession { conversation_id } => {
                if !self.open_tab_conversation_ids().contains(&conversation_id) {
                    self.history.delete_session(&conversation_id);
                    self.active_tab_mut().status_text =
                        format!("Deleted session {}", conversation_id);
                } else {
                    let preserved_query = match self.modals.top() {
                        Some(Modal::Select(select)) => {
                            let q = select.query.clone();
                            self.modals.pop();
                            Some(q)
                        }
                        _ => None,
                    };
                    if let Some(q) = preserved_query {
                        self.open_resume_modal(if q.is_empty() { None } else { Some(q.as_str()) });
                    }
                    self.active_tab_mut().status_text =
                        format!("Cannot delete open session {}", conversation_id);
                }
                self.dirty = true;
                Some(None)
            }
            ModalAction::ExecuteCommand(cmd) => {
                self.modals.pop();
                match cmd.as_str() {
                    "new_tab" => self.request_new_tab(),
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
                    "select_mode" => {
                        self.open_mode_modal(None);
                        Some(None)
                    }
                    "resume_session" => {
                        self.open_resume_modal(None);
                        Some(None)
                    }
                    "rename_session" => {
                        self.active_tab_mut().editor.insert_str("/rename ");
                        self.dirty = true;
                        Some(None)
                    }
                    "view_usage" => {
                        let tab = self.active_tab();
                        let modal = if tab.backend == crate::config::BackendKind::OpenCode {
                            crate::ui::modal::UsageModal::load_opencode_with_service(
                                self.usage_service.clone(),
                                &self.active_model_name(),
                                tab.last_turn_usage,
                                tab.accumulated_usage,
                            )
                            .with_stats_cli(
                                !self.history.is_in_memory()
                                    || std::env::var_os("FRONTAL_OPENCODE_BIN").is_some(),
                            )
                        } else {
                            crate::ui::modal::UsageModal::load_with_service(
                                self.usage_service.clone(),
                                !self.history.is_in_memory(),
                            )
                        };
                        self.modals.push(Modal::Usage(Box::new(modal)));
                        Some(None)
                    }
                    "toggle_sidebar" => {
                        self.sidebar_visible = !self.sidebar_visible;
                        self.dirty = true;
                        Some(None)
                    }
                    "clear" => {
                        self.clear_active_tab();
                        Some(None)
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
