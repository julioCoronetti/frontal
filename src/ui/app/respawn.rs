use crate::config::BackendKind;
use crate::protocol::SessionId;
use crate::ui::app::App;

impl App {
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
                let history_tx = self.history.write_tx_clone();
                tokio::task::spawn_blocking(move || {
                    crate::infra::history::PromptHistory::flush_db_from_tx(history_tx.as_ref());
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

        let shares_active_conv =
            !was_replay && final_conv_id.is_some() && final_conv_id == prev_conv_id;
        let prev_meta = if shares_active_conv {
            self.session_manager.get_metadata(&old_id).cloned()
        } else {
            None
        };
        if shares_active_conv {
            self.pending_engine_prompts
                .retain(|(sid, _)| sid != &old_id);
            let _ = self
                .session_manager
                .shutdown_session(&old_id, std::time::Duration::from_millis(500))
                .await;
        }

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
            let dormant_config = shares_active_conv.then(|| config.clone());
            match self.session_manager.create_subprocess_session(config).await {
                Ok(id) => id,
                Err(err) => {
                    if let (Some(mut meta), Some(cfg)) = (prev_meta, dormant_config) {
                        meta.status = crate::infra::session::SessionStatus::Closed;
                        self.session_manager
                            .save_dormant_metadata(old_id.clone(), meta, cfg);
                    }
                    self.tabs[tab_idx].waiting_for_response = false;
                    self.tabs[tab_idx].status_text = format!("Failed to respawn session: {}", err);
                    self.dirty = true;
                    return Err(err);
                }
            }
        };
        if !shares_active_conv {
            self.pending_engine_prompts
                .retain(|(sid, _)| sid != &old_id);
            self.session_manager
                .detach_and_shutdown_background(&old_id, std::time::Duration::from_millis(500));
        } else {
            self.session_manager.remove_dormant_metadata(&old_id);
        }
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
            tokio::task::spawn_blocking(move || {
                let _ = config_to_save.save();
            });
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
        self.apply_hydrated_session(tab_idx, hydrated_session, is_different_conv, model_changed);

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

    fn apply_hydrated_session(
        &mut self,
        tab_idx: usize,
        hydrated_session: Option<crate::ui::history::HydratedSession>,
        is_different_conv: bool,
        model_changed: bool,
    ) {
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
