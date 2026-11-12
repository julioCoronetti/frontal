mod git_diff;
mod modals;
mod resume;

use crate::ui::app::{App, AppAction};
use crate::ui::modal::{DiffModal, HotRespawnModal, Modal, ModelEffortModal};

impl App {
    pub fn is_turn_active(&self) -> bool {
        self.waiting_for_response
            || self.cache.has_unconsolidated_blocks()
            || self
                .session_manager
                .get_metadata(&self.active_session)
                .map(|m| m.status == crate::session::SessionStatus::RunningTurn)
                .unwrap_or(false)
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
            self.accumulated_usage = crate::protocol::TokenUsage::default();
            self.queued_prompts.clear();
            self.status_text = String::from("Cleared");
            self.dirty = true;
            Some(None)
        } else if trimmed == "/new" {
            self.sync_session_to_history(&self.active_session.clone());
            self.status_text = String::from("New tab");
            self.dirty = true;
            Some(Some(AppAction::NewTab))
        } else if trimmed == "/quota" {
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
            self.dirty = true;
            Some(None)
        } else if trimmed == "/interrupt" {
            let _ = self.session_manager.interrupt_active();
            self.status_text = String::from("Interrupted");
            self.waiting_for_response = false;
            self.queued_prompts.clear();
            self.dirty = true;
            Some(Some(AppAction::Interrupt))
        } else if trimmed == "/model" || trimmed.starts_with("/model ") {
            let mut modal = ModelEffortModal::with_models(self.available_models.clone());
            modal.set_opencode_models(self.opencode_models.clone());
            modal.set_backend(self.backend);
            let mut modal = modal.with_initial_selection(
                Some(self.active_model_name()),
                Some(self.active_effort_name()),
            );
            if let Some(query) = trimmed.strip_prefix("/model ") {
                modal.set_query(query.trim());
            }
            self.modals.push(Modal::ModelEffort(modal));
            self.dirty = true;
            Some(None)
        } else if trimmed == "/diff" {
            self.modals
                .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
            self.dirty = true;
            Some(None)
        } else if trimmed == "/respawn" {
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
        } else if trimmed == "/resume" || trimmed.starts_with("/resume ") {
            let query = trimmed.strip_prefix("/resume ").map(|q| q.trim());
            self.open_resume_modal(query);
            Some(None)
        } else if trimmed == "/rename" || trimmed.starts_with("/rename ") {
            let new_title = trimmed.strip_prefix("/rename").unwrap_or("").trim();
            if new_title.is_empty() {
                self.status_text = String::from("Usage: /rename <title>");
                self.dirty = true;
                return Some(None);
            }
            self.title = new_title.to_string();
            self.sync_session_to_history(&self.active_session.clone());
            self.history
                .rename_session(self.active_session.as_str(), new_title);
            if let Some(conv_id) = self
                .session_manager
                .get_metadata(&self.active_session)
                .and_then(|m| m.conversation_id.clone())
            {
                self.history.rename_session(&conv_id, new_title);
                let _ = crate::ui::history::update_agent_conversation_title(&conv_id, new_title);
            }
            self.status_text = format!("Session renamed to: {}", new_title);
            self.dirty = true;
            Some(None)
        } else if trimmed == "/help" {
            self.cache.push_user_message("/help");
            self.cache.push_error(&format!(
                "Frontal v{} - Available local commands: /clear, /new, /quota, /interrupt, /quit, /exit, /help, /model, /diff, /respawn, /resume, /rename",
                env!("CARGO_PKG_VERSION")
            ));
            self.status_text = String::from("Ready");
            self.dirty = true;
            Some(None)
        } else if trimmed.starts_with('/') {
            let cmd_name = trimmed.split_whitespace().next().unwrap_or(trimmed);
            self.cache.push_user_message(trimmed);
            self.cache
                .push_error(&format!("Unknown command: {}", cmd_name));
            self.status_text = format!("Unknown command: {}", cmd_name);
            self.dirty = true;
            Some(None)
        } else {
            None
        }
    }
}
