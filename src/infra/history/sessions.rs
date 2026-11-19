use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use crate::config::BackendKind;

use super::{HistoryCommand, PromptHistory};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TitleSource {
    #[default]
    Default = 0,
    Provisional = 1,
    Semantic = 2,
    Custom = 3,
}

impl TitleSource {
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Provisional,
            2 => Self::Semantic,
            3.. => Self::Custom,
            _ => Self::Default,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub conversation_id: Option<String>,
    pub backend: Option<BackendKind>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub title: Option<String>,
    pub title_source: TitleSource,
    pub created_at: i64,
}

pub fn format_relative_time(created_at: i64) -> String {
    if created_at <= 0 {
        return "just now".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let diff = now.saturating_sub(created_at);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

pub fn infer_backend(conversation_id: Option<&str>, explicit: Option<BackendKind>) -> BackendKind {
    if let Some(backend) = explicit {
        return backend;
    }
    match conversation_id {
        Some(id) if id.starts_with("ses_") => BackendKind::OpenCode,
        _ => BackendKind::Agy,
    }
}

pub fn normalize_title_source(title: &Option<String>, source: TitleSource) -> TitleSource {
    match title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => TitleSource::Default,
        Some("New session") => TitleSource::Default,
        Some(_) if source == TitleSource::Default => TitleSource::Semantic,
        Some(_) => source,
    }
}

impl SessionRecord {
    pub fn relative_time(&self) -> String {
        format_relative_time(self.created_at)
    }

    pub fn effective_backend(&self) -> BackendKind {
        infer_backend(self.conversation_id.as_deref(), self.backend)
    }
}

impl PromptHistory {
    pub(super) fn load_recent_sessions(
        conn: &Connection,
        limit: usize,
    ) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, model, mode, effort, title, created_at, backend, title_source, custom_title
             FROM sessions
             WHERE conversation_id IS NOT NULL AND conversation_id != ''
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let conv_id: Option<String> = row.get(1)?;
            let title: Option<String> = row.get(5)?;
            let backend_raw: Option<String> = row.get(7)?;
            let source_raw: i64 = row.get(8).unwrap_or(0);
            let custom_raw: i64 = row.get(9).unwrap_or(0);
            let backend = backend_raw
                .as_deref()
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map(BackendKind::parse)
                .or_else(|| conv_id.as_deref().map(|cid| infer_backend(Some(cid), None)));
            let title_source = if custom_raw != 0 {
                TitleSource::Custom
            } else {
                normalize_title_source(&title, TitleSource::from_i64(source_raw))
            };
            Ok(SessionRecord {
                id: row.get(0)?,
                conversation_id: conv_id,
                backend,
                model: row.get(2)?,
                mode: row.get(3)?,
                effort: row.get(4)?,
                title,
                title_source,
                created_at: row.get(6)?,
            })
        })?;
        let mut result = Vec::new();
        for r in rows.flatten() {
            result.push(r);
        }
        Ok(result)
    }

    pub fn record_session(&mut self, mut record: SessionRecord) {
        if record.id.trim().is_empty() {
            return;
        }
        record.title_source = normalize_title_source(&record.title, record.title_source);
        if record.backend.is_none() {
            if let Some(ref cid) = record.conversation_id {
                if !cid.trim().is_empty() {
                    record.backend = Some(infer_backend(Some(cid), None));
                }
            }
        }
        let existing_pos = self.sessions.iter().position(|s| {
            s.id == record.id
                || (record.conversation_id.is_some()
                    && s.conversation_id.is_some()
                    && s.conversation_id == record.conversation_id)
        });

        if let Some(pos) = existing_pos {
            let existing = &self.sessions[pos];
            record.id = existing.id.clone();
            if record.conversation_id.is_none() {
                record.conversation_id = existing.conversation_id.clone();
            }
            if record.backend.is_none() {
                record.backend = existing.backend;
            }
            if record.model.is_none() {
                record.model = existing.model.clone();
            }
            if record.mode.is_none() {
                record.mode = existing.mode.clone();
            }
            if record.effort.is_none() {
                record.effort = existing.effort.clone();
            }
            let existing_source = normalize_title_source(&existing.title, existing.title_source);
            if existing.title.is_some()
                && (existing_source >= record.title_source || record.title.is_none())
            {
                record.title = existing.title.clone();
                record.title_source = existing_source;
            }
            if record.created_at <= 0 {
                record.created_at = existing.created_at;
            }
            self.sessions.remove(pos);
        }

        if record.created_at <= 0 && !self.is_in_memory() {
            if let Some(ref cid) = record.conversation_id {
                if let Some(ts) = self.cached_agent_timestamp(cid) {
                    record.created_at = ts;
                }
            }
        }
        if record.created_at <= 0 {
            record.created_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs() as i64;
        }

        self.sessions.push(record.clone());
        self.sessions
            .sort_by_key(|s| std::cmp::Reverse(s.created_at));
        if self.sessions.len() > 500 {
            self.sessions.truncate(500);
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RecordSession(record));
        }
    }

    pub fn touch_session_timestamp(&mut self, id_or_conv: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as i64;
        for s in &mut self.sessions {
            if s.id == id_or_conv || s.conversation_id.as_deref() == Some(id_or_conv) {
                s.created_at = now;
            }
        }
        self.sessions
            .sort_by_key(|s| std::cmp::Reverse(s.created_at));
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::TouchSession {
                id: id_or_conv.to_string(),
                timestamp: now,
            });
        }
    }

    pub fn is_in_memory(&self) -> bool {
        self.write_tx.is_none()
    }

    pub fn rename_session(&mut self, id_or_conv: &str, new_title: &str) {
        self.rename_session_with_source(id_or_conv, new_title, TitleSource::Semantic);
    }

    pub fn rename_session_with_source(
        &mut self,
        id_or_conv: &str,
        new_title: &str,
        source: TitleSource,
    ) {
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return;
        }
        for s in &mut self.sessions {
            if (s.id == id_or_conv || s.conversation_id.as_deref() == Some(id_or_conv))
                && source >= s.title_source
            {
                s.title = Some(trimmed.to_string());
                s.title_source = source;
            }
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RenameSession {
                id: id_or_conv.to_string(),
                title: trimmed.to_string(),
                source,
            });
        }
    }

    pub fn rename_agent_title(&mut self, conv_id: &str, new_title: &str) {
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return;
        }
        for s in &mut self.sessions {
            if s.id == conv_id || s.conversation_id.as_deref() == Some(conv_id) {
                s.title = Some(trimmed.to_string());
                s.title_source = TitleSource::Custom;
            }
        }
        if let Ok(mut guard) = self.agent_cache.write() {
            if let Some(item) = guard
                .iter_mut()
                .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            {
                item.title = Some(trimmed.to_string());
                item.title_source = TitleSource::Custom;
            } else {
                guard.push(SessionRecord {
                    id: format!("ext-{}", conv_id),
                    conversation_id: Some(conv_id.to_string()),
                    backend: Some(infer_backend(Some(conv_id), None)),
                    model: None,
                    mode: None,
                    effort: None,
                    title: Some(trimmed.to_string()),
                    title_source: TitleSource::Custom,
                    created_at: 0,
                });
            }
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RenameAgentTitle {
                conv_id: conv_id.to_string(),
                title: trimmed.to_string(),
            });
        }
    }

    pub fn cached_agent_title(&self, conv_id: &str) -> Option<String> {
        let guard = self.agent_cache.read().ok()?;
        guard
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            .and_then(|s| s.title.clone())
    }

    pub fn cached_agent_entry(&self, conv_id: &str) -> Option<(String, TitleSource)> {
        let guard = self.agent_cache.read().ok()?;
        let item = guard
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))?;
        let title = item.title.clone()?;
        Some((title, item.title_source))
    }

    pub fn cached_agent_timestamp(&self, conv_id: &str) -> Option<i64> {
        let guard = self.agent_cache.read().ok()?;
        guard
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            .map(|s| s.created_at)
            .filter(|ts| *ts > 0)
    }

    pub fn cached_agent_conversations(&self) -> Vec<SessionRecord> {
        self.agent_cache
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn request_agent_refresh(&self, conv_id: Option<String>) {
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RefreshAgent(conv_id));
        }
    }

    pub fn poll_agent_title_updates(&self) -> Vec<(String, String)> {
        let mut updates = Vec::new();
        if let Some(ref rx) = self.agent_updates_rx {
            while let Ok(item) = rx.try_recv() {
                updates.push(item);
            }
        }
        updates
    }

    pub fn sessions(&self) -> &[SessionRecord] {
        &self.sessions
    }

    pub fn resumable_sessions(&self) -> Vec<SessionRecord> {
        self.sessions
            .iter()
            .filter(|s| {
                s.conversation_id
                    .as_ref()
                    .map(|c| !c.trim().is_empty())
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}
