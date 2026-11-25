use crate::config::BackendKind;
use crate::infra::history::TitleSource;
use crate::ui::app::App;
use crate::ui::modal::{Modal, ModalAction};

fn backend_badge(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Agy => "Agy",
        BackendKind::OpenCode => "OpenCode",
    }
}

fn is_default_tab_title(title: &str) -> bool {
    let trimmed = title.trim();
    trimmed.is_empty() || trimmed == "New session" || trimmed.starts_with("Session ")
}

impl App {
    pub(crate) fn sync_session_to_history(&mut self, session_id: &crate::protocol::SessionId) {
        if let Some(meta) = self.session_manager.get_metadata(session_id) {
            let tab_state = self
                .tabs
                .iter()
                .find(|t| &t.session_id == session_id)
                .map(|t| (t.title.clone(), t.custom_title, t.title_source, t.backend));
            let (tab_raw_title, tab_custom, tab_source, backend) = tab_state
                .map(|(title, custom, source, backend)| {
                    let effective_source = if custom { TitleSource::Custom } else { source };
                    (Some(title), custom, effective_source, Some(backend))
                })
                .unwrap_or((None, false, TitleSource::Default, None));
            let tab_title = tab_raw_title.filter(|t| !is_default_tab_title(t));

            let agent_entry = if self.history.is_in_memory() {
                None
            } else {
                meta.conversation_id
                    .as_deref()
                    .and_then(|cid| self.history.cached_agent_entry(cid))
            };
            let semantic_agent = agent_entry
                .as_ref()
                .filter(|(_, src)| *src == TitleSource::Semantic)
                .map(|(t, _)| t.clone());
            let provisional_agent = agent_entry
                .as_ref()
                .filter(|(_, src)| *src == TitleSource::Provisional)
                .map(|(t, _)| t.clone());

            let (final_title, final_source) = if tab_custom || tab_source == TitleSource::Custom {
                (tab_title.or(semantic_agent), TitleSource::Custom)
            } else if let Some(semantic) = semantic_agent {
                (Some(semantic), TitleSource::Semantic)
            } else if let Some(existing) = tab_title {
                let src = if tab_source == TitleSource::Default {
                    TitleSource::Provisional
                } else {
                    tab_source
                };
                (Some(existing), src)
            } else if let Some(prompt_title) = meta
                .last_prompt
                .as_deref()
                .map(crate::ui::history::clean_session_title)
            {
                (Some(prompt_title), TitleSource::Provisional)
            } else if let Some(prov) = provisional_agent {
                (Some(prov), TitleSource::Provisional)
            } else {
                (None, TitleSource::Default)
            };

            if let Some(ref t) = final_title {
                if let Some(tab) = self.tabs.iter_mut().find(|tb| &tb.session_id == session_id) {
                    if !tab.custom_title
                        && (final_source.can_override(tab.title_source)
                            || is_default_tab_title(&tab.title))
                    {
                        tab.title = t.clone();
                        tab.title_source = final_source;
                    }
                }
            }
            if meta.conversation_id.is_none() && meta.last_prompt.is_none() && final_title.is_none()
            {
                return;
            }
            let existing_ts = self
                .history
                .snapshot()
                .sessions
                .iter()
                .find(|s| {
                    s.id == session_id.as_str()
                        || (meta.conversation_id.is_some()
                            && s.conversation_id.as_ref() == meta.conversation_id.as_ref())
                })
                .map(|s| s.created_at)
                .unwrap_or(0);

            let record = crate::ui::history::SessionRecord {
                id: session_id.as_str().to_string(),
                conversation_id: meta.conversation_id.clone(),
                backend,
                model: meta.model.clone(),
                mode: meta.mode.clone(),
                effort: meta.effort.clone(),
                title: final_title,
                title_source: final_source,
                created_at: existing_ts,
            };
            self.history.record_session(record);
        }
    }

    pub(crate) fn open_tab_conversation_ids(&self) -> std::collections::HashSet<String> {
        let mut open_ids = std::collections::HashSet::new();
        let snap = self.history.snapshot();
        for tab in &self.tabs {
            open_ids.insert(tab.session_id.as_str().to_string());
            if let Some(Some(cid)) = self
                .session_manager
                .get_metadata(&tab.session_id)
                .map(|m| m.conversation_id.clone())
            {
                open_ids.insert(cid);
            }
            for s in snap
                .sessions
                .iter()
                .filter(|s| s.id == tab.session_id.as_str())
            {
                if let Some(ref cid) = s.conversation_id {
                    open_ids.insert(cid.clone());
                }
            }
        }
        open_ids
    }

    pub fn open_resume_modal(&mut self, initial_query: Option<&str>) {
        let tab_session_ids: Vec<_> = self.tabs.iter().map(|t| t.session_id.clone()).collect();
        for sid in tab_session_ids {
            self.sync_session_to_history(&sid);
        }

        let snap = self.history.snapshot();
        let mut items = Vec::new();
        let mut refresh_cids = Vec::new();

        for (idx, tab) in self.tabs.iter().enumerate() {
            let display_title = if tab.title.trim().is_empty() {
                String::from("New session")
            } else {
                tab.title.clone()
            };
            let tab_conv = self
                .session_manager
                .get_metadata(&tab.session_id)
                .and_then(|m| m.conversation_id.clone());
            if !tab.custom_title && tab.title_source < TitleSource::Semantic {
                if let Some(ref cid) = tab_conv {
                    refresh_cids.push(cid.clone());
                }
            }
            let rel_time = snap
                .sessions
                .iter()
                .find(|s| {
                    s.id == tab.session_id.as_str()
                        || (tab_conv.is_some() && s.conversation_id == tab_conv)
                })
                .map(|s| s.relative_time())
                .unwrap_or_else(|| String::from("just now"));

            items.push(
                crate::ui::modal::SelectItem::new(
                    format!("tab-{}", idx),
                    display_title,
                    ModalAction::SwitchTab(idx),
                )
                .with_badge(backend_badge(tab.backend))
                .with_shortcut(rel_time)
                .with_category("Current Sessions"),
            );
        }

        let open_ids = self.open_tab_conversation_ids();

        let mut past_sessions: Vec<_> = snap
            .sessions
            .iter()
            .filter(|s| {
                s.conversation_id
                    .as_ref()
                    .map(|c| !c.trim().is_empty())
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !self.history.is_in_memory() {
            for ext in &snap.agent_sessions {
                if let Some(ref cid) = ext.conversation_id {
                    if let Some(existing) = past_sessions
                        .iter_mut()
                        .find(|p| p.conversation_id.as_ref() == Some(cid))
                    {
                        let title_missing = existing.title.is_none()
                            || existing.title.as_deref() == Some(cid.as_str());
                        if (title_missing
                            || (ext.title_source.can_override(existing.title_source)
                                && existing.title_source < TitleSource::Custom))
                            && ext.title.is_some()
                        {
                            existing.title = ext.title.clone();
                            existing.title_source = ext.title_source;
                        }
                        if existing.backend.is_none() {
                            existing.backend = ext.backend;
                        }
                        if existing.model.is_none() {
                            existing.model = ext.model.clone();
                        }
                        if existing.effort.is_none() {
                            existing.effort = ext.effort.clone();
                        }
                        if existing.created_at <= 0 && ext.created_at > 0 {
                            existing.created_at = ext.created_at;
                        }
                    } else {
                        past_sessions.push(ext.clone());
                    }
                }
            }
            for cid in refresh_cids {
                self.history.request_agent_refresh(Some(cid));
            }
            self.history.request_agent_refresh(None);
        }
        past_sessions.sort_by_key(|a| std::cmp::Reverse(a.created_at));

        let mut seen_conv_ids = std::collections::HashSet::new();
        for s in past_sessions {
            let conv_id = s.conversation_id.clone().unwrap_or_default();
            if conv_id.is_empty()
                || open_ids.contains(&conv_id)
                || open_ids.contains(&s.id)
                || !seen_conv_ids.insert(conv_id.clone())
            {
                continue;
            }
            let valid_title = s
                .title
                .as_ref()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty() && *t != conv_id);
            let title = match valid_title {
                Some(t) => crate::ui::history::clamp_display_title(t, 42),
                None => continue,
            };
            let rel_time = s.relative_time();
            let badge = backend_badge(s.effective_backend());
            items.push(
                crate::ui::modal::SelectItem::new(
                    conv_id.clone(),
                    title,
                    ModalAction::Respawn {
                        conversation_id: Some(conv_id),
                        model: s.model,
                        effort: s.effort,
                        mode: s.mode,
                    },
                )
                .with_badge(badge)
                .with_shortcut(rel_time)
                .with_category("Past Sessions")
                .with_deletable(true),
            );
        }

        let mut select = crate::ui::modal::DialogSelect::new("Resume Session", items);
        if let Some(q) = initial_query {
            select.query = q.to_string();
        }
        self.modals.push(Modal::Select(select));
        self.dirty = true;
    }
}
