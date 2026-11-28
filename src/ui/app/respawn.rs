use crate::config::BackendKind;
use crate::infra::session::{SessionMetadata, SessionStatus};
use crate::protocol::SessionId;
use crate::transport::{EngineConfig, EngineError};
use crate::ui::app::respawn_io::{run_respawn_io, RespawnIoInput, RespawnIoResult};
use crate::ui::app::App;
use crate::ui::history::HydratedSession;
use crate::ui::tab::{OpToken, TurnEvent};

pub(crate) struct RespawnPlan {
    pub token: OpToken,
    pub old_id: SessionId,
    pub was_replay: bool,
    pub target_backend: BackendKind,
    pub requested_backend: Option<BackendKind>,
    pub is_different_conv: bool,
    pub model_changed: bool,
    pub final_conv_id: Option<String>,
    pub final_mode: Option<String>,
    pub is_explicit_config_change: bool,
    pub shares_active_conv: bool,
    pub prev_meta: Option<SessionMetadata>,
    pub dormant_config: Option<EngineConfig>,
    pub warnings: Vec<String>,
}

impl App {
    pub(crate) fn prepare_respawn(
        &mut self,
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> (RespawnPlan, RespawnIoInput) {
        let tab_idx = self.active_tab_index;
        let token = self.tabs[tab_idx].bump_generation();
        let old_id = self.active_session_id().clone();
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
                .snapshot()
                .sessions
                .iter()
                .find(|s| s.conversation_id.as_deref() == Some(id))
                .and_then(|s| s.backend)
                .unwrap_or_else(|| crate::infra::history::infer_backend(Some(id), None))
        });
        let is_explicit_config_change =
            conversation_id.is_none() && (model.is_some() || effort.is_some() || mode.is_some());
        let target_backend = requested_backend.unwrap_or_else(|| {
            if model.is_some() {
                self.user_config.backend
            } else {
                prev_backend
            }
        });
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
        let engine_config = (!was_replay).then(|| {
            let mut config = base_engine_config
                .unwrap_or_else(|| EngineConfig::from_user_config(&self.user_config));
            config.backend = target_backend;
            config.conversation_id = final_conv_id.clone();
            config.mode = final_mode.clone();
            config
        });
        let dormant_config = shares_active_conv
            .then(|| {
                engine_config.clone().map(|mut c| {
                    c.model = model
                        .clone()
                        .or_else(|| inherited_prev_model.clone())
                        .or_else(|| user_fallback_model.clone());
                    c.effort = effort.clone().or_else(|| inherited_prev_effort.clone());
                    c
                })
            })
            .flatten();
        let mut plan_warnings = Vec::new();
        let detached_old = if shares_active_conv {
            self.pending_engine_prompts
                .retain(|(sid, _)| sid != &old_id);
            let detached = self.session_manager.detach_engine(&old_id);
            if let (Some(mut meta), Some(cfg)) = (prev_meta.clone(), dormant_config.clone()) {
                meta.status = SessionStatus::Closed;
                if !self
                    .session_manager
                    .save_dormant_metadata(old_id.clone(), meta, cfg)
                {
                    let msg = format!(
                        "Failed to archive dormant metadata for active session {}",
                        old_id
                    );
                    self.tabs[tab_idx].status_text = msg.clone();
                    plan_warnings.push(msg);
                    self.dirty = true;
                }
            }
            detached
        } else {
            None
        };

        let plan = RespawnPlan {
            token,
            old_id,
            was_replay,
            target_backend,
            requested_backend,
            is_different_conv,
            model_changed,
            final_conv_id: final_conv_id.clone(),
            final_mode,
            is_explicit_config_change,
            shares_active_conv,
            prev_meta,
            dormant_config,
            warnings: plan_warnings,
        };
        let io_input = RespawnIoInput {
            detached_old,
            should_hydrate,
            final_conv_id,
            hydration_client: self.history.hydration_client(),
            engine_config,
            event_sender: self.session_manager.event_sender(),
            target_backend,
            requested_model: model,
            requested_effort: effort,
            inherited_prev_model,
            inherited_prev_effort,
            user_fallback_model,
            user_config_effort: self.user_config.effort.clone(),
        };
        (plan, io_input)
    }

    pub(crate) fn apply_respawn_outcome(
        &mut self,
        plan: RespawnPlan,
        outcome: RespawnIoResult,
    ) -> Result<Option<SessionId>, EngineError> {
        let Some(tab_idx) = self.tabs.iter().position(|t| {
            t.tab_id == plan.token.tab_id && t.generation_id == plan.token.generation_id
        }) else {
            if plan.shares_active_conv && !self.tabs.iter().any(|t| t.session_id == plan.old_id) {
                self.session_manager.remove_dormant_metadata(&plan.old_id);
            }
            if let Some(Ok((_sid, handle))) = outcome.spawned {
                self.session_manager
                    .push_closing_handle(handle, std::time::Duration::from_millis(500));
            }
            return Ok(None);
        };

        let final_model = outcome.final_model;
        let final_effort = outcome.final_effort;
        let final_mode = outcome.final_mode.or(plan.final_mode);
        let mut all_warnings = plan.warnings;
        all_warnings.extend(outcome.warnings);

        let new_id = if plan.was_replay {
            let id = SessionId::new();
            self.session_manager.register_mock_session_with_metadata(
                id.clone(),
                plan.final_conv_id.clone(),
                final_model.clone(),
                final_effort.clone(),
                final_mode.clone(),
            );
            id
        } else {
            match outcome
                .spawned
                .unwrap_or_else(|| Err(EngineError::Protocol("missing spawned engine".into())))
            {
                Ok((id, handle)) => self.session_manager.attach_engine(id, handle, false),
                Err(err) => {
                    let dormant_ok = if let (Some(mut meta), Some(mut cfg)) =
                        (plan.prev_meta, plan.dormant_config)
                    {
                        cfg.model = final_model;
                        cfg.effort = final_effort;
                        cfg.mode = final_mode;
                        meta.status = SessionStatus::Closed;
                        self.session_manager
                            .save_dormant_metadata(plan.old_id.clone(), meta, cfg)
                    } else {
                        true
                    };
                    self.tabs[tab_idx].transition_turn(TurnEvent::RespawnReset);
                    if !dormant_ok {
                        all_warnings.push(format!(
                            "dormant rollback skipped for active session {}",
                            plan.old_id
                        ));
                    }
                    self.tabs[tab_idx].status_text = if all_warnings.is_empty() {
                        format!("Failed to respawn session: {}", err)
                    } else {
                        format!(
                            "Failed to respawn session: {} ({})",
                            err,
                            all_warnings.join("; ")
                        )
                    };
                    self.dirty = true;
                    return Err(err);
                }
            }
        };

        if !plan.shares_active_conv {
            self.pending_engine_prompts
                .retain(|(sid, _)| sid != &plan.old_id);
            self.session_manager.detach_and_shutdown_background(
                &plan.old_id,
                std::time::Duration::from_millis(500),
            );
        } else {
            self.session_manager.remove_dormant_metadata(&plan.old_id);
        }

        if let Some(backend) = plan.requested_backend {
            self.user_config.backend = backend;
        }
        if plan.is_explicit_config_change {
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
                let _ = crate::infra::config_store::save(&config_to_save);
            });
        }

        self.session_manager.set_active_session(new_id.clone());
        self.tabs[tab_idx].session_id = new_id.clone();
        self.tabs[tab_idx].backend = plan.target_backend;
        self.tabs[tab_idx].transition_turn(TurnEvent::RespawnReset);
        self.modals.remove_danger_modal();

        if plan.is_different_conv {
            self.tabs[tab_idx].last_turn_usage = None;
            self.tabs[tab_idx].accumulated_usage = crate::protocol::TokenUsage::default();
            self.tabs[tab_idx].steps_seen = 0;
            self.tabs[tab_idx].last_step_index = None;
            self.tabs[tab_idx].turns_completed = 0;
            self.tabs[tab_idx].last_turn_duration_seconds = None;
        }
        self.tabs[tab_idx].in_flight_usage = None;
        if plan.is_different_conv || plan.model_changed {
            self.tabs[tab_idx].context_window_hint = None;
            self.tabs[tab_idx].served_model = None;
        }
        self.apply_hydrated_session(
            tab_idx,
            outcome.hydrated_session,
            plan.is_different_conv,
            plan.model_changed,
        );

        self.sync_session_to_history(&new_id);
        self.tabs[tab_idx].status_text = if all_warnings.is_empty() {
            format!("Respawned session: {}", new_id)
        } else {
            format!(
                "Respawned session: {} ({})",
                new_id,
                all_warnings.join("; ")
            )
        };
        self.dirty = true;
        Ok(Some(new_id))
    }

    pub async fn respawn_active_session(
        &mut self,
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> Result<SessionId, EngineError> {
        let (plan, io_input) = self.prepare_respawn(conversation_id, model, effort, mode);
        let outcome = run_respawn_io(io_input).await;
        self.apply_respawn_outcome(plan, outcome)?
            .ok_or(EngineError::ProcessNotRunning)
    }

    fn apply_hydrated_session(
        &mut self,
        tab_idx: usize,
        hydrated_session: Option<HydratedSession>,
        is_different_conv: bool,
        model_changed: bool,
    ) {
        if let Some(hydrated_data) = hydrated_session {
            if hydrated_data.hydrated {
                self.tabs[tab_idx].cache = hydrated_data.cache;
                self.tabs[tab_idx].persisted_block_id = self.tabs[tab_idx]
                    .cache
                    .blocks()
                    .last()
                    .map(|block| block.id)
                    .unwrap_or(0);
                self.tabs[tab_idx].viewport.scroll_to_bottom();
            } else if is_different_conv {
                self.tabs[tab_idx].cache.clear();
                self.tabs[tab_idx].persisted_block_id = 0;
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
            self.tabs[tab_idx].persisted_block_id = 0;
        }
    }
}

fn backend_compatible(backend: BackendKind, conversation_id: &str) -> bool {
    match backend {
        BackendKind::OpenCode => conversation_id.starts_with("ses_"),
        BackendKind::Agy => !conversation_id.starts_with("ses_"),
    }
}

pub(crate) fn is_model_compatible_with_backend(backend: BackendKind, model: &str) -> bool {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return false;
    }
    match backend {
        BackendKind::Agy => !trimmed.contains('/'),
        BackendKind::OpenCode => trimmed.contains('/'),
    }
}

#[cfg(test)]
#[path = "respawn_tests.rs"]
mod tests;
