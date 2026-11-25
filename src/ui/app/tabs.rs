use crate::config::BackendKind;
use crate::infra::session::{SessionHandle, SessionManager};
use crate::protocol::{SessionEnvelope, SessionId};
use crate::transport::{EngineConfig, EngineError};
use crate::ui::app::App;
use crate::ui::tab::{OpToken, TabState};

pub(crate) struct PendingTabReset {
    pub token: OpToken,
    pub prev_backend: BackendKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    pub config: EngineConfig,
    pub event_sender: tokio::sync::mpsc::Sender<SessionEnvelope>,
}

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

    fn remove_tab_and_select_next(&mut self, index: usize) -> Option<SessionId> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None;
        }
        self.popover.close();
        self.modals.remove_danger_modal();
        let removed_tab = self.tabs.remove(index);
        self.pending_engine_prompts
            .retain(|(sid, _)| sid != &removed_tab.session_id);
        Some(removed_tab.session_id)
    }

    fn finish_close_tab(&mut self, index: usize) {
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
    }

    pub fn close_tab_sync(&mut self, index: usize) -> Option<SessionId> {
        let removed_id = self.remove_tab_and_select_next(index)?;
        self.session_manager
            .detach_and_shutdown_background(&removed_id, std::time::Duration::from_millis(500));
        self.finish_close_tab(index);
        Some(removed_id)
    }

    pub async fn close_tab(&mut self, index: usize) -> Option<SessionId> {
        let removed_id = self.remove_tab_and_select_next(index)?;
        let _ = self
            .session_manager
            .shutdown_session(&removed_id, std::time::Duration::from_millis(500))
            .await;
        self.finish_close_tab(index);
        Some(removed_id)
    }

    fn reset_single_tab_empty(&mut self) -> SessionId {
        self.tabs[0].title = String::from("New session");
        self.tabs[0].custom_title = false;
        self.tabs[0].title_source = crate::infra::history::TitleSource::Default;
        self.tabs[0].has_unread = false;
        self.tabs[0].transition_turn(crate::ui::tab::TurnEvent::RespawnReset);
        self.tabs[0].editor.clear();
        self.tabs[0].draft_snapshot = None;
        self.tabs[0].status_text = String::from("Ready");
        let _ = self.tabs[0].bump_generation();
        self.dirty = true;
        self.tabs[0].session_id.clone()
    }

    fn prepare_single_tab_reset(
        &mut self,
    ) -> (
        SessionId,
        Option<PendingTabReset>,
        BackendKind,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
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
        if was_replay {
            (old_id, None, prev_backend, model, effort, mode)
        } else {
            let mut config = base_engine_config.unwrap_or_default();
            config.backend = prev_backend;
            config.conversation_id = None;
            config.model = model.clone();
            config.effort = effort.clone();
            config.mode = mode.clone();
            let mut fresh_tab = TabState::new(SessionId::new(), String::from("New session"));
            fresh_tab.backend = prev_backend;
            let token = fresh_tab.op_token();
            let pending = PendingTabReset {
                token,
                prev_backend,
                model: model.clone(),
                effort: effort.clone(),
                mode: mode.clone(),
                config,
                event_sender: self.session_manager.event_sender(),
            };
            self.tabs[0] = fresh_tab;
            self.active_tab_index = 0;
            self.dirty = true;
            (old_id, Some(pending), prev_backend, model, effort, mode)
        }
    }

    pub(crate) fn close_or_reset_tab_sync(&mut self, index: usize) -> Option<PendingTabReset> {
        if self.tabs.len() > 1 {
            let _ = self.close_tab_sync(index);
            return None;
        }
        if index != 0 || self.tabs.is_empty() {
            return None;
        }
        self.popover.close();
        self.modals.remove_danger_modal();
        self.hit_regions.clear();
        self.hovered_tab_index = None;
        self.hovered_new_tab = false;

        if self.tabs[0].cache.blocks_count() == 0 && !self.tabs[0].is_waiting_for_response() {
            let _ = self.reset_single_tab_empty();
            return None;
        }

        let (old_id, maybe_pending, prev_backend, model, effort, mode) =
            self.prepare_single_tab_reset();
        self.session_manager
            .detach_and_shutdown_background(&old_id, std::time::Duration::from_millis(500));

        if let Some(pending) = maybe_pending {
            self.tabs[0].status_text = String::from("Starting session...");
            return Some(pending);
        }

        let id = SessionId::new();
        self.session_manager.register_mock_session(id.clone());
        self.session_manager
            .set_session_metadata(&id, None, model, effort, mode);
        let mut fresh_tab = TabState::new(id.clone(), String::from("New session"));
        fresh_tab.backend = prev_backend;
        self.tabs[0] = fresh_tab;
        self.active_tab_index = 0;
        self.session_manager.set_active_session(id);
        self.dirty = true;
        None
    }

    pub(crate) fn finish_tab_reset(
        &mut self,
        pending: PendingTabReset,
        res: Result<(SessionId, SessionHandle), EngineError>,
    ) {
        let Some(tab) = self.tabs.get_mut(0) else {
            if let Ok((_sid, handle)) = res {
                tokio::spawn(async move {
                    let _ = SessionManager::shutdown_handle(
                        handle,
                        std::time::Duration::from_millis(500),
                    )
                    .await;
                });
            }
            return;
        };
        if tab.op_token() != pending.token {
            if let Ok((_sid, handle)) = res {
                tokio::spawn(async move {
                    let _ = SessionManager::shutdown_handle(
                        handle,
                        std::time::Duration::from_millis(500),
                    )
                    .await;
                });
            }
            return;
        }

        let (new_id, spawn_err) = match res {
            Ok((id, handle)) => (self.session_manager.attach_engine(id, handle, false), None),
            Err(err) => {
                let id = SessionId::new();
                self.session_manager.register_mock_session(id.clone());
                self.session_manager.set_session_metadata(
                    &id,
                    None,
                    pending.model,
                    pending.effort,
                    pending.mode,
                );
                (id, Some(err.to_string()))
            }
        };

        let tab = &mut self.tabs[0];
        tab.session_id = new_id.clone();
        tab.backend = pending.prev_backend;
        if let Some(err) = spawn_err {
            tab.cache
                .push_error(&format!("Failed to start session: {}", err));
            tab.status_text = format!("Session error: {}", err);
        } else {
            tab.status_text = String::from("Ready");
        }
        self.session_manager.set_active_session(new_id);
        self.dirty = true;
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

        if self.tabs[0].cache.blocks_count() == 0 && !self.tabs[0].is_waiting_for_response() {
            return Some(self.reset_single_tab_empty());
        }

        let (old_id, maybe_pending, prev_backend, model, effort, mode) =
            self.prepare_single_tab_reset();
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;

        if let Some(pending) = maybe_pending {
            let res = SessionManager::spawn_engine_detached(
                pending.config.clone(),
                pending.event_sender.clone(),
            )
            .await;
            self.finish_tab_reset(pending, res);
            return Some(old_id);
        }

        let id = SessionId::new();
        self.session_manager.register_mock_session(id.clone());
        self.session_manager
            .set_session_metadata(&id, None, model, effort, mode);
        let mut fresh_tab = TabState::new(id.clone(), String::from("New session"));
        fresh_tab.backend = prev_backend;
        self.tabs[0] = fresh_tab;
        self.active_tab_index = 0;
        self.session_manager.set_active_session(id);
        self.dirty = true;
        Some(old_id)
    }
}
