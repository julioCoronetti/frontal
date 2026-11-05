use crate::ui::app::{App, AppAction};
use crate::ui::modal::{DiffModal, HotRespawnModal, Modal, ModalAction, ModelEffortModal};

impl App {
    pub fn load_git_diff() -> String {
        if let Ok(out) = std::process::Command::new("git").args(["diff"]).output() {
            if !out.stdout.is_empty() {
                return String::from_utf8_lossy(&out.stdout).to_string();
            }
        }
        if let Ok(out) = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .output()
        {
            if !out.stdout.is_empty() {
                return String::from_utf8_lossy(&out.stdout).to_string();
            }
        }
        String::from("diff --git (working tree clean)\nNo unstaged or uncommitted changes found.\n")
    }

    pub(crate) fn dispatch_modal_action(
        &mut self,
        action: ModalAction,
    ) -> Option<Option<AppAction>> {
        match action {
            ModalAction::Close => Some(None),
            ModalAction::ApproveDanger | ModalAction::RejectDanger => Some(None),
            ModalAction::SelectModelEffort { model, effort } => {
                self.modals.pop();
                self.status_text = format!("Model: {} ({})", model, effort);
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
            ModalAction::ExecuteCommand(cmd) => {
                self.modals.pop();
                match cmd.as_str() {
                    "new_tab" => Some(Some(AppAction::NewTab)),
                    "close_tab" => Some(Some(AppAction::CloseTab(self.active_tab_index))),
                    "select_model" => {
                        let modal = ModelEffortModal::with_models(self.available_models.clone())
                            .with_initial_selection(
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
                    "view_diff" => {
                        self.modals
                            .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
                        Some(None)
                    }
                    "clear" => {
                        self.cache.clear();
                        self.viewport.clear_selection();
                        self.viewport.scroll_to_bottom();
                        self.last_turn_usage = None;
                        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                            tab.last_turn_usage = None;
                        }
                        self.status_text = String::from("Cleared");
                        Some(None)
                    }
                    "interrupt" => {
                        let _ = self.session_manager.interrupt_active();
                        self.status_text = String::from("Interrupted");
                        self.waiting_for_response = false;
                        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                            tab.waiting_for_response = false;
                            tab.status_text = self.status_text.clone();
                        }
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

    pub(crate) fn execute_local_command(&mut self, trimmed: &str) -> Option<Option<AppAction>> {
        if trimmed == "/quit" || trimmed == "/exit" {
            self.should_quit = true;
            Some(Some(AppAction::Quit))
        } else if trimmed == "/clear" {
            self.cache.clear();
            self.viewport.clear_selection();
            self.viewport.scroll_to_bottom();
            self.last_turn_usage = None;
            if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                tab.last_turn_usage = None;
            }
            self.status_text = String::from("Cleared");
            self.dirty = true;
            Some(None)
        } else if trimmed == "/interrupt" {
            let _ = self.session_manager.interrupt_active();
            self.status_text = String::from("Interrupted");
            self.waiting_for_response = false;
            if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                tab.waiting_for_response = false;
                tab.status_text = self.status_text.clone();
            }
            self.dirty = true;
            Some(Some(AppAction::Interrupt))
        } else if trimmed == "/model" {
            let modal = ModelEffortModal::with_models(self.available_models.clone())
                .with_initial_selection(
                    Some(self.active_model_name()),
                    Some(self.active_effort_name()),
                );
            self.modals.push(Modal::ModelEffort(modal));
            self.dirty = true;
            Some(None)
        } else if trimmed.starts_with("/model ") {
            let model_name = trimmed.strip_prefix("/model ").unwrap().trim();
            let mut modal = ModelEffortModal::with_models(self.available_models.clone())
                .with_initial_selection(
                    Some(self.active_model_name()),
                    Some(self.active_effort_name()),
                );
            modal.query = model_name.to_string();
            self.modals.push(Modal::ModelEffort(modal));
            self.dirty = true;
            Some(None)
        } else if trimmed == "/diff" {
            self.modals
                .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
            self.dirty = true;
            Some(None)
        } else if trimmed == "/respawn" || trimmed == "/resume" {
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
            self.dirty = true;
            Some(None)
        } else if trimmed == "/help" {
            self.cache.push_user_message("/help");
            self.cache.push_error(&format!(
                "Frontal v{} - Available local commands: /clear, /interrupt, /quit, /exit, /help, /model, /diff, /respawn",
                env!("CARGO_PKG_VERSION")
            ));
            self.status_text = String::from("Ready");
            self.dirty = true;
            Some(None)
        } else {
            None
        }
    }
}
