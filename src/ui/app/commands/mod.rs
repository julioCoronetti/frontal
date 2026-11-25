mod modals;
mod resume;

use crate::ui::app::{App, AppAction};
use crate::ui::modal::{Modal, ModelEffortModal};

impl App {
    pub fn is_turn_active(&self) -> bool {
        let tab = self.active_tab();
        tab.is_waiting_for_response()
            || tab.cache.has_unconsolidated_blocks()
            || self
                .session_manager
                .get_metadata(self.active_session_id())
                .map(|m| m.status == crate::session::SessionStatus::RunningTurn)
                .unwrap_or(false)
    }

    pub(crate) fn clear_active_tab(&mut self) {
        let tab = self.active_tab_mut();
        tab.cache.clear();
        tab.viewport.clear_selection();
        tab.viewport.scroll_to_bottom();
        tab.last_turn_usage = None;
        tab.accumulated_usage = crate::protocol::TokenUsage::default();
        tab.served_model = None;
        tab.context_window_hint = None;
        tab.steps_seen = 0;
        tab.turns_completed = 0;
        tab.last_turn_duration_seconds = None;
        tab.transition_turn(crate::ui::tab::TurnEvent::RespawnReset);
        tab.status_text = String::from("Cleared");
        self.dirty = true;
    }

    pub(crate) fn request_new_tab(&mut self) -> Option<Option<AppAction>> {
        let sid = self.active_session_id().clone();
        self.sync_session_to_history(&sid);
        self.active_tab_mut().status_text = String::from("New tab");
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
            let backend = self.active_tab().backend;
            if let Some(canonical) = crate::config::BackendKind::normalize_mode(arg)
                .filter(|mode| backend.supports_mode(mode))
            {
                self.active_tab_mut().status_text =
                    format!("Mode: {} · {}", canonical, backend.as_str());
                self.dirty = true;
                Some(Some(AppAction::RespawnSession {
                    conversation_id: None,
                    model: None,
                    effort: None,
                    mode: Some(canonical.to_string()),
                }))
            } else {
                let supported: Vec<&str> = backend
                    .supported_modes()
                    .iter()
                    .map(|(id, _)| *id)
                    .collect();
                let msg = format!(
                    "Unsupported mode '{}' for {} (supported: {})",
                    arg,
                    backend.as_str(),
                    supported.join(", ")
                );
                let tab = self.active_tab_mut();
                tab.cache.push_user_message(trimmed);
                tab.cache.push_error(&msg);
                tab.status_text = msg;
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
                self.active_tab_mut().status_text = String::from("Usage: /rename <title>");
                self.dirty = true;
                return Some(None);
            }
            let sid = self.active_session_id().clone();
            {
                let tab = self.active_tab_mut();
                tab.title = new_title.to_string();
                tab.custom_title = true;
                tab.title_source = crate::infra::history::TitleSource::Custom;
            }
            self.sync_session_to_history(&sid);
            self.history.rename_session_with_source(
                sid.as_str(),
                new_title,
                crate::infra::history::TitleSource::Custom,
            );
            if let Some(conv_id) = self
                .session_manager
                .get_metadata(&sid)
                .and_then(|m| m.conversation_id.clone())
            {
                self.history.rename_session_with_source(
                    &conv_id,
                    new_title,
                    crate::infra::history::TitleSource::Custom,
                );
                self.history.rename_agent_title(&conv_id, new_title);
            }
            self.active_tab_mut().status_text = format!("Session renamed to: {}", new_title);
            self.dirty = true;
            Some(None)
        } else if trimmed == "/help" {
            let tab = self.active_tab_mut();
            tab.cache.push_user_message("/help");
            tab.cache.push_error(&format!(
                "Frontal v{} - Available local commands: /clear, /new, /quota, /quit, /exit, /help, /model, /mode, /resume, /rename",
                env!("CARGO_PKG_VERSION")
            ));
            tab.status_text = String::from("Ready");
            self.dirty = true;
            Some(None)
        } else if trimmed.starts_with('/') {
            let cmd_name = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let tab = self.active_tab_mut();
            tab.cache.push_user_message(trimmed);
            tab.cache
                .push_error(&format!("Unknown command: {}", cmd_name));
            tab.status_text = format!("Unknown command: {}", cmd_name);
            self.dirty = true;
            Some(None)
        } else {
            None
        }
    }

    pub(crate) fn open_mode_modal(&mut self, query: Option<&str>) {
        let current_mode = self.active_mode_name();
        let supported = self.active_tab().backend.supported_modes();
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
