use crate::config::BackendKind;
use crate::protocol::SessionId;
use crate::ui::app::App;
use crate::ui::tab::TabState;

impl App {
    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn tab(&self, id: &SessionId) -> Option<&TabState> {
        self.tabs.iter().find(|t| &t.session_id == id)
    }

    pub fn tab_mut(&mut self, id: &SessionId) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|t| &t.session_id == id)
    }

    pub fn active_session_id(&self) -> &SessionId {
        &self.tabs[self.active_tab_index].session_id
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn switch_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab_index {
            return;
        }
        self.popover.close();
        self.modals.remove_danger_modal();
        self.active_tab_index = index;
        if let Some(danger) = &self.tabs[self.active_tab_index].pending_danger {
            self.modals
                .push(crate::ui::modal::Modal::DangerConfirmation(
                    crate::ui::modal::DangerConfirmationModal::new(danger.clone()),
                ));
        }
        self.tabs[self.active_tab_index].has_unread = false;
        let active_id = self.tabs[self.active_tab_index].session_id.clone();
        self.session_manager.set_active_session(active_id);
        self.hovered_tab_index = None;
        self.hovered_new_tab = false;
        self.dirty = true;
    }

    pub fn add_tab(&mut self, session_id: SessionId, title: Option<String>) -> usize {
        self.popover.close();
        self.modals.remove_danger_modal();
        let title = title.unwrap_or_else(|| String::from("New session"));
        let backend = self
            .session_manager
            .get_engine_config(&session_id)
            .map(|config| config.backend)
            .unwrap_or(self.user_config.backend);
        let mut new_tab = TabState::new(session_id, title);
        new_tab.backend = backend;
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.tabs[self.active_tab_index].has_unread = false;
        let active_id = self.tabs[self.active_tab_index].session_id.clone();
        self.session_manager.set_active_session(active_id);
        self.hit_regions.clear();
        self.hovered_tab_index = None;
        self.hovered_new_tab = false;
        self.dirty = true;
        self.active_tab_index
    }

    pub async fn close_tab(&mut self, index: usize) -> Option<SessionId> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None;
        }
        self.popover.close();
        self.modals.remove_danger_modal();
        let removed_tab = self.tabs.remove(index);
        self.pending_engine_prompts
            .retain(|(sid, _)| sid != &removed_tab.session_id);
        let _ = self
            .session_manager
            .shutdown_session(
                &removed_tab.session_id,
                std::time::Duration::from_millis(500),
            )
            .await;
        if index < self.active_tab_index {
            self.active_tab_index -= 1;
        } else if self.active_tab_index >= self.tabs.len() {
            self.active_tab_index = self.tabs.len() - 1;
        }
        if let Some(danger) = &self.tabs[self.active_tab_index].pending_danger {
            self.modals
                .push(crate::ui::modal::Modal::DangerConfirmation(
                    crate::ui::modal::DangerConfirmationModal::new(danger.clone()),
                ));
        }
        self.tabs[self.active_tab_index].has_unread = false;
        let active_id = self.tabs[self.active_tab_index].session_id.clone();
        self.session_manager.set_active_session(active_id);
        self.hit_regions.clear();
        self.hovered_tab_index = None;
        self.hovered_new_tab = false;
        self.dirty = true;
        Some(removed_tab.session_id)
    }

    pub async fn close_or_reset_tab(&mut self, index: usize) -> Option<SessionId> {
        if self.tabs.len() > 1 {
            return self.close_tab(index).await;
        }
        if index != 0 || self.tabs.is_empty() {
            return None;
        }
        self.popover.close();
        self.modals.remove_danger_modal();
        self.hit_regions.clear();
        self.hovered_tab_index = None;
        self.hovered_new_tab = false;

        if self.tabs[0].cache.blocks_count() == 0 && !self.tabs[0].waiting_for_response {
            self.tabs[0].title = String::from("New session");
            self.tabs[0].custom_title = false;
            self.tabs[0].title_source = crate::infra::history::TitleSource::Default;
            self.tabs[0].has_unread = false;
            self.tabs[0].queued_prompts.clear();
            self.tabs[0].pending_danger = None;
            self.tabs[0].suspended_events.clear();
            self.tabs[0].editor.clear();
            self.tabs[0].draft_snapshot = None;
            self.tabs[0].status_text = String::from("Ready");
            self.dirty = true;
            return Some(self.tabs[0].session_id.clone());
        }

        let old_id = self.tabs[0].session_id.clone();
        let was_replay = self.session_manager.is_replay_session(&old_id);
        let base_engine_config = self.session_manager.get_engine_config(&old_id);
        let prev_backend = base_engine_config
            .as_ref()
            .map(|config| config.backend)
            .unwrap_or(self.tabs[0].backend);
        let model = Some(self.active_model_name());
        let effort = Some(self.active_effort_name());
        let active_mode = self.active_mode_name();
        let mode = Some(
            BackendKind::normalize_mode(&active_mode)
                .filter(|m| prev_backend.supports_mode(m))
                .unwrap_or(crate::config::DEFAULT_MODE)
                .to_string(),
        );

        self.pending_engine_prompts
            .retain(|(sid, _)| sid != &old_id);
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;

        let new_id = if was_replay {
            let id = SessionId::new();
            self.session_manager.register_mock_session(id.clone());
            self.session_manager
                .set_session_metadata(&id, None, model, effort, mode);
            id
        } else {
            let mut config = base_engine_config.unwrap_or_default();
            config.backend = prev_backend;
            config.conversation_id = None;
            config.model = model.clone();
            config.effort = effort.clone();
            config.mode = mode.clone();
            match self.session_manager.create_subprocess_session(config).await {
                Ok(id) => id,
                Err(_) => {
                    let id = SessionId::new();
                    self.session_manager.register_mock_session(id.clone());
                    self.session_manager
                        .set_session_metadata(&id, None, model, effort, mode);
                    id
                }
            }
        };

        let mut fresh_tab = TabState::new(new_id.clone(), String::from("New session"));
        fresh_tab.backend = prev_backend;
        self.tabs[0] = fresh_tab;
        self.active_tab_index = 0;
        self.session_manager.set_active_session(new_id);
        self.dirty = true;
        Some(old_id)
    }
}
