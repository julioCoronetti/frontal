use crate::protocol::SessionId;
use crate::ui::app::App;
use crate::ui::tab::TabState;

impl App {
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
        self.active_session = self.tabs[self.active_tab_index].session_id.clone();
        if let Some(danger) = &self.tabs[self.active_tab_index].pending_danger {
            self.modals
                .push(crate::ui::modal::Modal::DangerConfirmation(
                    crate::ui::modal::DangerConfirmationModal::new(danger.clone()),
                ));
        }
        self.tabs[self.active_tab_index].has_unread = false;
        self.session_manager
            .set_active_session(self.active_session.clone());
        self.dirty = true;
    }

    pub fn add_tab(&mut self, session_id: SessionId, title: Option<String>) -> usize {
        self.popover.close();
        self.modals.remove_danger_modal();
        let title = title.unwrap_or_else(|| format!("Session {}", self.tabs.len() + 1));
        let new_tab = TabState::new(session_id, title);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.active_session = self.tabs[self.active_tab_index].session_id.clone();
        self.tabs[self.active_tab_index].has_unread = false;
        self.session_manager
            .set_active_session(self.active_session.clone());
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
        self.active_session = self.tabs[self.active_tab_index].session_id.clone();
        if let Some(danger) = &self.tabs[self.active_tab_index].pending_danger {
            self.modals
                .push(crate::ui::modal::Modal::DangerConfirmation(
                    crate::ui::modal::DangerConfirmationModal::new(danger.clone()),
                ));
        }
        self.tabs[self.active_tab_index].has_unread = false;
        self.session_manager
            .set_active_session(self.active_session.clone());
        self.dirty = true;
        Some(removed_tab.session_id)
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
        let final_conv_id = conversation_id.or(prev_conv_id.clone());
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
                final_conv_id.clone(),
                final_model,
                final_effort,
                final_mode,
            );
            id
        } else {
            let mut config = base_engine_config.unwrap_or_default();
            config.conversation_id = final_conv_id.clone();
            config.model = final_model;
            config.effort = final_effort;
            config.mode = final_mode;
            self.session_manager
                .create_subprocess_session(config)
                .await?
        };
        self.active_session = new_id.clone();
        self.session_manager.set_active_session(new_id.clone());
        self.tabs[self.active_tab_index].session_id = new_id.clone();
        self.tabs[self.active_tab_index].waiting_for_response = false;
        self.pending_danger = None;
        self.suspended_events.clear();
        self.queued_prompts.clear();
        self.modals.remove_danger_modal();

        let is_different_conv = match (&final_conv_id, &prev_conv_id) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) => true,
            _ => false,
        };
        if is_different_conv {
            self.last_turn_usage = None;
            self.accumulated_usage = crate::protocol::TokenUsage::default();
        }
        if is_different_conv || self.cache.is_empty() {
            if let Some(ref cid) = final_conv_id {
                let cid_clone = cid.clone();
                let mut hydrated_cache = crate::ui::cache::RenderCache::new();
                let (hydrated, new_cache, maybe_title) = tokio::task::spawn_blocking(move || {
                    let ok = crate::ui::history::hydrate_cache_from_transcript(
                        &cid_clone,
                        &mut hydrated_cache,
                    );
                    let title = crate::ui::history::query_agent_conversation_title(&cid_clone);
                    (ok, hydrated_cache, title)
                })
                .await
                .unwrap_or((false, crate::ui::cache::RenderCache::new(), None));

                if hydrated {
                    self.cache = new_cache;
                    self.viewport.scroll_to_bottom();
                } else if is_different_conv {
                    self.cache.clear();
                }
                if let Some(title) = maybe_title {
                    self.tabs[self.active_tab_index].title = title;
                }
            } else if is_different_conv {
                self.cache.clear();
            }
        }

        self.sync_session_to_history(&new_id);
        self.status_text = format!("Respawned session: {}", new_id);
        self.dirty = true;
        Ok(new_id)
    }
}
