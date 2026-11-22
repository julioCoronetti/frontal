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
            self.tabs[0].title = String::from("New session");
            self.tabs[0].custom_title = false;
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
        let tab_idx = self.active_tab_index;
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
        let prev_backend = base_engine_config
            .as_ref()
            .map(|config| config.backend)
            .unwrap_or(self.tabs[tab_idx].backend);
        let requested_backend = conversation_id.as_deref().map(|id| {
            self.history
                .sessions()
                .iter()
                .find(|s| s.conversation_id.as_deref() == Some(id))
                .and_then(|s| s.backend)
                .unwrap_or_else(|| crate::infra::history::infer_backend(Some(id), None))
        });
        let is_explicit_config_change = conversation_id.is_none();
        let target_backend = requested_backend.unwrap_or(self.user_config.backend);
        let backend_changed = target_backend != prev_backend;
        let final_conv_id = match conversation_id {
            Some(id) => Some(id),
            None if backend_changed => None,
            None => prev_conv_id
                .clone()
                .filter(|id| backend_compatible(target_backend, id)),
        };
        let is_different_conv = match (&final_conv_id, &prev_conv_id) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) => true,
            _ => false,
        };
        let should_hydrate = is_different_conv || self.tabs[tab_idx].cache.is_empty();
        let hydrated_session = if should_hydrate {
            if let Some(ref cid) = final_conv_id {
                let cid_clone = cid.clone();
                let db_path = self.history.path().map(|path| path.to_path_buf());
                tokio::task::spawn_blocking(move || {
                    crate::ui::history::hydrate_session_blocking(&cid_clone, db_path.as_deref())
                })
                .await
                .ok()
            } else {
                None
            }
        } else {
            None
        };

        let hydrated_effort_direct = hydrated_session.as_ref().and_then(|h| h.effort.clone());
        let (hydrated_model, hydrated_effort) = hydrated_session
            .as_ref()
            .and_then(|h| h.telemetry.as_ref())
            .and_then(|t| t.model.as_deref())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|raw| {
                let sanitized = crate::config::sanitize_model_id(raw);
                if target_backend == BackendKind::Agy && !sanitized.contains('/') {
                    let (base, eff) =
                        crate::transport::EngineConfig::split_model_effort(&sanitized);
                    (
                        (!base.is_empty()).then(|| base.to_string()),
                        eff.map(str::to_string),
                    )
                } else if is_model_compatible_with_backend(target_backend, &sanitized) {
                    (
                        Some(sanitized).filter(|s| !s.is_empty()),
                        hydrated_effort_direct.clone(),
                    )
                } else {
                    (None, None)
                }
            })
            .unwrap_or((None, hydrated_effort_direct));

        let model_changed = model
            .as_ref()
            .map(|requested| prev_model.as_ref() != Some(requested))
            .unwrap_or(false);
        let inherited_prev_model = (!is_different_conv && !backend_changed)
            .then_some(prev_model)
            .flatten()
            .filter(|m| is_model_compatible_with_backend(target_backend, m));
        let inherited_prev_effort = (!is_different_conv && !backend_changed)
            .then_some(prev_effort)
            .flatten();
        let user_fallback_model = (!backend_changed
            && is_model_compatible_with_backend(target_backend, &self.user_config.model))
        .then(|| crate::config::sanitize_model_id(&self.user_config.model))
        .filter(|m| !m.is_empty());
        let has_hydrated_model = hydrated_model.is_some();
        let final_model = model
            .or(hydrated_model)
            .or(inherited_prev_model)
            .or(user_fallback_model.clone());
        let final_effort = effort
            .or(hydrated_effort)
            .or(inherited_prev_effort)
            .or_else(|| {
                (!has_hydrated_model && user_fallback_model.is_some())
                    .then(|| self.user_config.effort.clone())
            });
        let requested_mode = mode
            .map(|value| {
                BackendKind::normalize_mode(&value)
                    .map(str::to_string)
                    .unwrap_or(value)
            })
            .filter(|value| target_backend.supports_mode(value));
        let inherited_prev_mode = (!is_different_conv && !backend_changed)
            .then_some(prev_mode)
            .flatten()
            .filter(|value| was_replay || target_backend.supports_mode(value))
            .map(|value| {
                if was_replay {
                    value
                } else {
                    BackendKind::normalize_mode(&value)
                        .map(str::to_string)
                        .unwrap_or(value)
                }
            });
        let fallback_mode = BackendKind::normalize_mode(&self.user_config.mode)
            .filter(|mode| target_backend.supports_mode(mode))
            .unwrap_or(crate::config::DEFAULT_MODE)
            .to_string();
        let final_mode = requested_mode
            .or(inherited_prev_mode)
            .or(Some(fallback_mode));

        let new_id = if was_replay {
            let id = SessionId::new();
            self.session_manager.register_mock_session_with_metadata(
                id.clone(),
                final_conv_id.clone(),
                final_model.clone(),
                final_effort.clone(),
                final_mode.clone(),
            );
            id
        } else {
            let mut config = base_engine_config.unwrap_or_default();
            config.backend = target_backend;
            config.conversation_id = final_conv_id.clone();
            config.model = final_model.clone();
            config.effort = final_effort.clone();
            config.mode = final_mode.clone();
            self.session_manager
                .create_subprocess_session(config)
                .await
                .inspect_err(|err| {
                    self.tabs[tab_idx].status_text = format!("Failed to respawn session: {}", err);
                    self.dirty = true;
                })?
        };
        self.pending_engine_prompts
            .retain(|(sid, _)| sid != &old_id);
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;
        if let Some(backend) = requested_backend {
            self.user_config.backend = backend;
        }
        if is_explicit_config_change {
            if let Some(m) = &final_model {
                self.user_config.model = m.clone();
            }
            if let Some(e) = &final_effort {
                self.user_config.effort = e.clone();
            }
            if let Some(md) = &final_mode {
                self.user_config.mode = md.clone();
            }
            let config_to_save = self.user_config.clone();
            let _ = tokio::task::spawn_blocking(move || config_to_save.save()).await;
        }
        self.active_session = new_id.clone();
        self.session_manager.set_active_session(new_id.clone());
        self.tabs[tab_idx].session_id = new_id.clone();
        self.tabs[tab_idx].backend = target_backend;
        self.tabs[tab_idx].waiting_for_response = false;
        self.tabs[tab_idx].pending_danger = None;
        self.tabs[tab_idx].suspended_events.clear();
        self.tabs[tab_idx].queued_prompts.clear();
        self.modals.remove_danger_modal();

        if is_different_conv {
            self.tabs[tab_idx].last_turn_usage = None;
            self.tabs[tab_idx].accumulated_usage = crate::protocol::TokenUsage::default();
            self.tabs[tab_idx].steps_seen = 0;
            self.tabs[tab_idx].last_step_index = None;
            self.tabs[tab_idx].turns_completed = 0;
            self.tabs[tab_idx].last_turn_duration_seconds = None;
        }
        self.tabs[tab_idx].in_flight_usage = None;
        if is_different_conv || model_changed {
            self.tabs[tab_idx].context_window_hint = None;
            self.tabs[tab_idx].served_model = None;
        }
        if let Some(hydrated_data) = hydrated_session {
            if hydrated_data.hydrated {
                self.tabs[tab_idx].cache = hydrated_data.cache;
                self.tabs[tab_idx].viewport.scroll_to_bottom();
            } else if is_different_conv {
                self.tabs[tab_idx].cache.clear();
            }
            if let Some(title) = hydrated_data.title {
                self.tabs[tab_idx].title = title;
                self.tabs[tab_idx].custom_title = hydrated_data.is_custom_title;
                self.tabs[tab_idx].title_source = hydrated_data.title_source;
            } else if is_different_conv {
                self.tabs[tab_idx].custom_title = hydrated_data.is_custom_title;
                self.tabs[tab_idx].title_source = hydrated_data.title_source;
            }
            if let Some(snapshot) = hydrated_data.telemetry {
                let fallback_model = snapshot.model.clone();
                self.tabs[tab_idx].last_turn_usage = Some(snapshot.last);
                self.tabs[tab_idx].accumulated_usage = snapshot.accumulated;
                self.tabs[tab_idx].steps_seen = snapshot.steps;
                self.tabs[tab_idx].last_step_index = None;
                self.tabs[tab_idx].turns_completed = snapshot.turns;
                self.tabs[tab_idx].last_turn_duration_seconds = snapshot
                    .duration_millis
                    .map(|millis| millis as f64 / 1000.0);
                if !model_changed {
                    if let Some(window) = snapshot.context_window {
                        self.tabs[tab_idx].context_window_hint = Some(window);
                    }
                    if let Some(model) = snapshot.model.filter(|model| !model.trim().is_empty()) {
                        self.tabs[tab_idx].served_model = Some(model);
                    }
                }
                let has_meta = self.tabs[tab_idx]
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
                    self.tabs[tab_idx].cache.set_last_assistant_turn_meta(meta);
                }
            }
        } else if is_different_conv {
            self.tabs[tab_idx].cache.clear();
        }

        self.tabs[tab_idx].persisted_block_id = self.tabs[tab_idx]
            .cache
            .blocks()
            .last()
            .map(|block| block.id)
            .unwrap_or(0);
        self.sync_session_to_history(&new_id);
        self.tabs[tab_idx].status_text = format!("Respawned session: {}", new_id);
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

fn is_model_compatible_with_backend(backend: BackendKind, model: &str) -> bool {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return false;
    }
    match backend {
        BackendKind::Agy => !trimmed.contains('/'),
        BackendKind::OpenCode => trimmed.contains('/'),
    }
}
