use crate::protocol::SessionId;
use crate::ui::app::App;
use crate::ui::cache::RenderCache;
use crate::ui::editor::PromptEditor;
use crate::ui::tab::TabState;
use crate::ui::viewport::ConversationViewport;

impl App {
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn switch_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab_index {
            return;
        }
        self.popover.close();
        self.save_active_tab_state();
        self.active_tab_index = index;
        self.load_active_tab_state();
        self.dirty = true;
    }

    pub fn save_active_tab_state(&mut self) {
        self.modals.remove_danger_modal();
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            std::mem::swap(&mut self.cache, &mut tab.cache);
            std::mem::swap(&mut self.viewport, &mut tab.viewport);
            std::mem::swap(&mut self.editor, &mut tab.editor);
            tab.waiting_for_response = self.waiting_for_response;
            tab.turn_started_at = self.turn_started_at;
            std::mem::swap(&mut self.status_text, &mut tab.status_text);
            std::mem::swap(&mut self.draft_snapshot, &mut tab.draft_snapshot);
            std::mem::swap(&mut self.last_turn_usage, &mut tab.last_turn_usage);
            std::mem::swap(&mut self.pending_danger, &mut tab.pending_danger);
            std::mem::swap(&mut self.suspended_events, &mut tab.suspended_events);
            tab.session_id = self.active_session.clone();
        }
    }

    pub fn load_active_tab_state(&mut self) {
        self.modals.remove_danger_modal();
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            std::mem::swap(&mut self.cache, &mut tab.cache);
            std::mem::swap(&mut self.viewport, &mut tab.viewport);
            std::mem::swap(&mut self.editor, &mut tab.editor);
            self.waiting_for_response = tab.waiting_for_response;
            self.turn_started_at = tab.turn_started_at;
            std::mem::swap(&mut self.status_text, &mut tab.status_text);
            std::mem::swap(&mut self.draft_snapshot, &mut tab.draft_snapshot);
            std::mem::swap(&mut self.last_turn_usage, &mut tab.last_turn_usage);
            std::mem::swap(&mut self.pending_danger, &mut tab.pending_danger);
            std::mem::swap(&mut self.suspended_events, &mut tab.suspended_events);
            if let Some(danger) = &self.pending_danger {
                self.modals
                    .push(crate::ui::modal::Modal::DangerConfirmation(
                        crate::ui::modal::DangerConfirmationModal::new(danger.clone()),
                    ));
            }
            self.active_session = tab.session_id.clone();
            tab.has_unread = false;
            self.session_manager
                .set_active_session(self.active_session.clone());
        }
    }

    pub fn add_tab(&mut self, session_id: SessionId, title: Option<String>) -> usize {
        self.popover.close();
        self.save_active_tab_state();
        let title = title.unwrap_or_else(|| format!("Session {}", self.tabs.len() + 1));
        let new_tab = TabState::new(session_id, title);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.load_active_tab_state();
        self.dirty = true;
        self.active_tab_index
    }

    pub async fn close_tab(&mut self, index: usize) -> Option<SessionId> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None;
        }
        if index == self.active_tab_index {
            let removed_tab = self.tabs.remove(index);
            let _ = self
                .session_manager
                .shutdown_session(
                    &removed_tab.session_id,
                    std::time::Duration::from_millis(500),
                )
                .await;
            self.cache = RenderCache::new();
            self.viewport = ConversationViewport::new();
            self.editor = PromptEditor::new();
            self.draft_snapshot = None;
            self.waiting_for_response = false;
            self.turn_started_at = None;
            self.last_turn_usage = None;
            self.pending_danger = None;
            self.suspended_events.clear();
            self.modals.remove_danger_modal();
            self.status_text = String::from("Ready");
            self.popover.close();
            self.active_tab_index = self.active_tab_index.min(self.tabs.len() - 1);
            self.load_active_tab_state();
            self.dirty = true;
            Some(removed_tab.session_id)
        } else {
            let removed_tab = self.tabs.remove(index);
            let _ = self
                .session_manager
                .shutdown_session(
                    &removed_tab.session_id,
                    std::time::Duration::from_millis(500),
                )
                .await;
            if index < self.active_tab_index {
                self.active_tab_index -= 1;
            }
            self.popover.close();
            self.dirty = true;
            Some(removed_tab.session_id)
        }
    }

    pub async fn respawn_active_session(
        &mut self,
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> Result<SessionId, crate::transport::EngineError> {
        let old_id = self.active_session.clone();
        let was_replay = self.session_manager.is_replay_session(&old_id);
        let base_engine_config = self.session_manager.get_engine_config(&old_id);
        let (prev_conv_id, prev_model, prev_effort, prev_mode) = {
            let meta = self.session_manager.get_metadata(&old_id);
            (
                meta.and_then(|m| m.conversation_id.clone()),
                meta.and_then(|m| m.model.clone()),
                meta.and_then(|m| m.effort.clone()),
                meta.and_then(|m| m.mode.clone()),
            )
        };
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;
        let final_conv_id = conversation_id.or(prev_conv_id);
        let final_model = model.or(prev_model);
        let final_effort = effort.or(prev_effort);
        let final_mode = mode.or(prev_mode);

        if let Some(m) = &final_model {
            self.user_config.model = m.clone();
        }
        if let Some(e) = &final_effort {
            self.user_config.effort = e.clone();
        }
        if let Some(md) = &final_mode {
            self.user_config.mode = md.clone();
        }
        let _ = self.user_config.save();

        let new_id = if was_replay {
            let id = SessionId::new();
            self.session_manager.register_mock_session(id.clone());
            self.session_manager.set_session_metadata(
                &id,
                final_conv_id,
                final_model,
                final_effort,
                final_mode,
            );
            id
        } else {
            let mut config = base_engine_config.unwrap_or_default();
            config.conversation_id = final_conv_id;
            config.model = final_model;
            config.effort = final_effort;
            config.mode = final_mode;
            self.session_manager
                .create_subprocess_session(config)
                .await?
        };
        self.active_session = new_id.clone();
        self.session_manager.set_active_session(new_id.clone());
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            tab.session_id = new_id.clone();
            tab.waiting_for_response = false;
        }
        self.waiting_for_response = false;
        self.pending_danger = None;
        self.suspended_events.clear();
        self.modals.remove_danger_modal();
        self.sync_session_to_history(&new_id);
        self.status_text = format!("Respawned session: {}", new_id);
        self.dirty = true;
        Ok(new_id)
    }
}
