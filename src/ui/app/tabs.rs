use crate::config::BackendKind;
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
        self.active_session = self.tabs[self.active_tab_index].session_id.clone();
        self.tabs[self.active_tab_index].has_unread = false;
        self.session_manager
            .set_active_session(self.active_session.clone());
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
        let mode = Some(self.active_mode_name());

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
            config.model = model;
            config.effort = effort;
            config.mode = mode;
            match self.session_manager.create_subprocess_session(config).await {
                Ok(id) => id,
                Err(_) => {
                    let id = SessionId::new();
                    self.session_manager.register_mock_session(id.clone());
                    id
                }
            }
        };

        let mut fresh_tab = TabState::new(new_id.clone(), String::from("New session"));
        fresh_tab.backend = prev_backend;
        self.tabs[0] = fresh_tab;
        self.active_tab_index = 0;
        self.active_session = new_id.clone();
        self.session_manager.set_active_session(new_id);
        self.dirty = true;
        Some(old_id)
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
        let model = model
            .map(|value| crate::config::sanitize_model_id(&value))
            .filter(|value| !value.is_empty());
        let (prev_conv_id, prev_model, prev_effort, prev_mode) = {
            let meta = self.session_manager.get_metadata(&old_id);
            (
                meta.and_then(|m| m.conversation_id.clone()),
                meta.and_then(|m| m.model.clone())
                    .map(|value| crate::config::sanitize_model_id(&value))
                    .filter(|value| !value.is_empty()),
                meta.and_then(|m| m.effort.clone()),
                meta.and_then(|m| m.mode.clone()),
            )
        };
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;
        let prev_backend = base_engine_config
            .as_ref()
            .map(|config| config.backend)
            .unwrap_or(self.tabs[self.active_tab_index].backend);
        let requested_backend = conversation_id.as_deref().map(|id| {
            if id.starts_with("ses_") {
                BackendKind::OpenCode
            } else {
                BackendKind::Agy
            }
        });
        let target_backend = requested_backend.unwrap_or(self.user_config.backend);
        if let Some(backend) = requested_backend {
            self.user_config.backend = backend;
        }
        let backend_changed = target_backend != prev_backend;
        let final_conv_id = match conversation_id {
            Some(id) => Some(id),
            None if backend_changed => None,
            None => prev_conv_id
                .clone()
                .filter(|id| backend_compatible(target_backend, id)),
        };
        let model_changed = model
            .as_ref()
            .map(|requested| prev_model.as_ref() != Some(requested))
            .unwrap_or(false);
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
            config.backend = target_backend;
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
        self.tabs[self.active_tab_index].backend = target_backend;
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
            self.steps_seen = 0;
            self.last_step_index = None;
            self.turns_completed = 0;
            self.last_turn_duration_seconds = None;
        }
        self.in_flight_usage = None;
        if is_different_conv || model_changed {
            self.context_window_hint = None;
            self.served_model = None;
        }
        if is_different_conv || self.cache.is_empty() {
            if let Some(ref cid) = final_conv_id {
                let cid_clone = cid.clone();
                let db_path = self.history.path().map(|path| path.to_path_buf());
                let mut hydrated_cache = crate::ui::cache::RenderCache::new();
                let (hydrated, new_cache, maybe_title, maybe_telemetry) =
                    tokio::task::spawn_blocking(move || {
                        let ok = crate::ui::history::hydrate_conversation(
                            &cid_clone,
                            &mut hydrated_cache,
                            db_path.as_deref(),
                        );
                        let title =
                            crate::ui::history::conversation_title(&cid_clone, db_path.as_deref());
                        let telemetry = db_path
                            .as_deref()
                            .and_then(|path| {
                                crate::infra::history::telemetry::load(path, &cid_clone)
                            })
                            .or_else(|| {
                                crate::infra::history::opencode::load_usage(&cid_clone).map(
                                    |(usage, model, duration_millis)| {
                                        crate::infra::history::TelemetrySnapshot {
                                            last: usage,
                                            accumulated: usage,
                                            steps: 0,
                                            turns: 0,
                                            duration_millis,
                                            context_window: None,
                                            model,
                                        }
                                    },
                                )
                            });
                        (ok, hydrated_cache, title, telemetry)
                    })
                    .await
                    .unwrap_or((
                        false,
                        crate::ui::cache::RenderCache::new(),
                        None,
                        None,
                    ));

                if hydrated {
                    self.cache = new_cache;
                    self.viewport.scroll_to_bottom();
                } else if is_different_conv {
                    self.cache.clear();
                }
                if let Some(title) = maybe_title {
                    self.tabs[self.active_tab_index].title = title;
                }
                if let Some(snapshot) = maybe_telemetry {
                    let fallback_model = snapshot.model.clone();
                    self.last_turn_usage = Some(snapshot.last);
                    self.accumulated_usage = snapshot.accumulated;
                    self.steps_seen = snapshot.steps;
                    self.last_step_index = None;
                    self.turns_completed = snapshot.turns;
                    self.last_turn_duration_seconds = snapshot
                        .duration_millis
                        .map(|millis| millis as f64 / 1000.0);
                    if !model_changed {
                        if let Some(window) = snapshot.context_window {
                            self.context_window_hint = Some(window);
                        }
                        if let Some(model) = snapshot.model.filter(|model| !model.trim().is_empty())
                        {
                            self.served_model = Some(model);
                        }
                    }
                    let has_meta = self
                        .cache
                        .blocks()
                        .iter()
                        .rev()
                        .find(|block| matches!(block.kind, crate::ui::cache::BlockKind::Assistant))
                        .map(|block| block.turn_meta.is_some())
                        .unwrap_or(false);
                    if !has_meta && (snapshot.turns > 0 || snapshot.last.output_tokens > 0) {
                        let mode = self.active_mode_name();
                        let model = fallback_model.unwrap_or_else(|| self.active_model_name());
                        let meta = crate::ui::cache::TurnMeta {
                            mode,
                            model,
                            duration_seconds: snapshot
                                .duration_millis
                                .map(|millis| millis as f64 / 1000.0),
                            output_tokens: (snapshot.last.output_tokens > 0)
                                .then_some(snapshot.last.output_tokens),
                        };
                        self.cache.set_last_assistant_turn_meta(meta);
                    }
                }
            } else if is_different_conv {
                self.cache.clear();
            }
        }

        self.tabs[self.active_tab_index].persisted_block_id = self
            .cache
            .blocks()
            .last()
            .map(|block| block.id)
            .unwrap_or(0);
        self.sync_session_to_history(&new_id);
        self.status_text = format!("Respawned session: {}", new_id);
        self.dirty = true;
        Ok(new_id)
    }
}

fn backend_compatible(backend: BackendKind, conversation_id: &str) -> bool {
    match backend {
        BackendKind::OpenCode => conversation_id.starts_with("ses_"),
        BackendKind::Agy => !conversation_id.starts_with("ses_"),
    }
}
