use crate::ui::app::App;
use crate::ui::modal::{Modal, ModalAction};

impl App {
    pub(crate) fn sync_session_to_history(&mut self, session_id: &crate::protocol::SessionId) {
        if let Some(meta) = self.session_manager.get_metadata(session_id) {
            let tab_title = self
                .tabs
                .iter()
                .find(|t| &t.session_id == session_id)
                .map(|t| t.title.clone())
                .filter(|t| !t.starts_with("Session ") && !t.trim().is_empty());

            let agent_title = if self.history.is_in_memory() {
                None
            } else {
                meta.conversation_id
                    .as_deref()
                    .and_then(crate::ui::history::query_agent_conversation_title)
            };
            let final_title = agent_title
                .or_else(|| {
                    meta.last_prompt
                        .as_deref()
                        .map(crate::ui::history::clean_session_title)
                })
                .or(tab_title);

            if let Some(ref t) = final_title {
                if let Some(tab) = self.tabs.iter_mut().find(|tb| &tb.session_id == session_id) {
                    if tab.title.starts_with("Session ") || tab.title.trim().is_empty() {
                        tab.title = t.clone();
                    }
                }
            }
            if meta.conversation_id.is_none() && meta.last_prompt.is_none() && final_title.is_none()
            {
                return;
            }
            let existing_ts = self
                .history
                .sessions()
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
                model: meta.model.clone(),
                mode: meta.mode.clone(),
                effort: meta.effort.clone(),
                title: final_title,
                created_at: existing_ts,
            };
            self.history.record_session(record);
        }
    }

    pub fn open_resume_modal(&mut self, initial_query: Option<&str>) {
        self.sync_session_to_history(&self.active_session.clone());

        let mut items = Vec::new();

        for (idx, tab) in self.tabs.iter().enumerate() {
            let display_title = if tab.title.trim().is_empty() {
                format!("Session {}", idx + 1)
            } else {
                tab.title.clone()
            };
            let tab_conv = self
                .session_manager
                .get_metadata(&tab.session_id)
                .and_then(|m| m.conversation_id.clone());
            let rel_time = self
                .history
                .sessions()
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
                .with_shortcut(rel_time)
                .with_category("Current Sessions"),
            );
        }

        let mut open_ids = std::collections::HashSet::new();
        for tab in &self.tabs {
            open_ids.insert(tab.session_id.as_str().to_string());
            if let Some(Some(cid)) = self
                .session_manager
                .get_metadata(&tab.session_id)
                .map(|m| m.conversation_id.clone())
            {
                open_ids.insert(cid);
            }
            for s in self
                .history
                .sessions()
                .iter()
                .filter(|s| s.id == tab.session_id.as_str())
            {
                if let Some(ref cid) = s.conversation_id {
                    open_ids.insert(cid.clone());
                }
            }
        }

        let mut past_sessions = self.history.resumable_sessions();
        if !self.history.is_in_memory() {
            for ext in crate::ui::history::load_agent_conversations() {
                if let Some(ref cid) = ext.conversation_id {
                    if let Some(existing) = past_sessions
                        .iter_mut()
                        .find(|p| p.conversation_id.as_ref() == Some(cid))
                    {
                        if (existing.title.is_none()
                            || existing.title.as_deref() == Some(cid.as_str()))
                            && ext.title.is_some()
                        {
                            existing.title = ext.title.clone();
                        }
                    } else {
                        past_sessions.push(ext);
                    }
                }
            }
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
                .with_shortcut(rel_time)
                .with_category("Past Sessions"),
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
