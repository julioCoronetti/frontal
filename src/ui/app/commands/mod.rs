mod modals;
mod resume;

use crate::ui::app::{App, AppAction};
use crate::ui::modal::{Modal, ModelEffortModal};

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

    pub(crate) fn clear_active_tab(&mut self) {
        self.cache.clear();
        self.viewport.clear_selection();
        self.viewport.scroll_to_bottom();
        self.last_turn_usage = None;
        self.in_flight_usage = None;
        self.accumulated_usage = crate::protocol::TokenUsage::default();
        self.context_window_hint = None;
        self.steps_seen = 0;
        self.last_step_index = None;
        self.turns_completed = 0;
        self.last_turn_duration_seconds = None;
        self.queued_prompts.clear();
        self.esc_armed_at = None;
        self.status_text = String::from("Cleared");
        self.dirty = true;
    }

    pub(crate) fn request_new_tab(&mut self) -> Option<Option<AppAction>> {
        self.sync_session_to_history(&self.active_session.clone());
        self.status_text = String::from("New tab");
        self.dirty = true;
        Some(Some(AppAction::NewTab))
    }

    pub(crate) fn execute_local_command(&mut self, trimmed: &str) -> Option<Option<AppAction>> {
        if trimmed == "/quit" || trimmed == "/exit" {
            self.should_quit = true;
            Some(Some(AppAction::Quit))
        } else if trimmed == "/clear" {
            self.clear_active_tab();
            Some(None)
        } else if trimmed == "/new" {
            self.request_new_tab()
        } else if trimmed == "/quota" {
            let modal = if self.backend == crate::config::BackendKind::OpenCode {
                crate::ui::modal::UsageModal::load_opencode(
                    &self.active_model_name(),
                    self.last_turn_usage,
                    self.accumulated_usage,
                )
                .with_stats_cli(
                    !self.history.is_in_memory()
                        || std::env::var_os("FRONTAL_OPENCODE_BIN").is_some(),
                )
            } else {
                crate::ui::modal::UsageModal::load(!self.history.is_in_memory())
            };
            self.modals.push(Modal::Usage(modal));
            self.dirty = true;
            Some(None)
        } else if trimmed == "/model" || trimmed.starts_with("/model ") {
            let mut modal = ModelEffortModal::with_models(self.available_models.clone());
            modal.set_opencode_models(self.opencode_models.clone());
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
        } else if trimmed == "/mode" || trimmed.starts_with("/mode ") {
            let arg = trimmed.strip_prefix("/mode").unwrap_or("").trim();
            if arg.is_empty() {
                self.open_mode_modal(None);
                return Some(None);
            }
            if let Some(canonical) = crate::config::BackendKind::normalize_mode(arg)
                .filter(|mode| self.backend.supports_mode(mode))
            {
                self.status_text = format!("Mode: {} · {}", canonical, self.backend.as_str());
                self.dirty = true;
                Some(Some(AppAction::RespawnSession {
                    conversation_id: None,
                    model: None,
                    effort: None,
                    mode: Some(canonical.to_string()),
                }))
            } else {
                let supported: Vec<&str> = self
                    .backend
                    .supported_modes()
                    .iter()
                    .map(|(id, _)| *id)
                    .collect();
                let msg = format!(
                    "Unsupported mode '{}' for {} (supported: {})",
                    arg,
                    self.backend.as_str(),
                    supported.join(", ")
                );
                self.cache.push_user_message(trimmed);
                self.cache.push_error(&msg);
                self.status_text = msg;
                self.dirty = true;
                Some(None)
            }
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
            self.custom_title = true;
            self.title_source = crate::infra::history::TitleSource::Custom;
            self.sync_session_to_history(&self.active_session.clone());
            self.history.rename_session_with_source(
                self.active_session.as_str(),
                new_title,
                crate::infra::history::TitleSource::Custom,
            );
            if let Some(conv_id) = self
                .session_manager
                .get_metadata(&self.active_session)
                .and_then(|m| m.conversation_id.clone())
            {
                self.history.rename_session_with_source(
                    &conv_id,
                    new_title,
                    crate::infra::history::TitleSource::Custom,
                );
                self.history.rename_agent_title(&conv_id, new_title);
            }
            self.status_text = format!("Session renamed to: {}", new_title);
            self.dirty = true;
            Some(None)
        } else if trimmed == "/help" {
            self.cache.push_user_message("/help");
            self.cache.push_error(&format!(
                "Frontal v{} - Available local commands: /clear, /new, /quota, /quit, /exit, /help, /model, /mode, /resume, /rename",
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

    pub(crate) fn open_mode_modal(&mut self, query: Option<&str>) {
        let current_mode = self.active_mode_name();
        let supported = self.backend.supported_modes();
        let items = supported
            .iter()
            .map(|(mode_id, desc)| {
                let mut item = crate::ui::modal::SelectItem::new(
                    *mode_id,
                    *mode_id,
                    crate::ui::modal::ModalAction::Respawn {
                        conversation_id: None,
                        model: None,
                        effort: None,
                        mode: Some((*mode_id).to_string()),
                    },
                )
                .with_description(*desc);
                if mode_id.eq_ignore_ascii_case(&current_mode) {
                    item = item.with_badge("active");
                }
                item
            })
            .collect();
        let mut select = crate::ui::modal::DialogSelect::new("Select Execution Mode", items);
        if let Some(pos) = supported
            .iter()
            .position(|(mode_id, _)| mode_id.eq_ignore_ascii_case(&current_mode))
        {
            select.selected_idx = pos;
        }
        if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
            select.query = q.to_string();
        }
        self.modals.push(Modal::Select(select));
        self.dirty = true;
    }
}
