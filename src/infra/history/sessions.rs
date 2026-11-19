use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::{HistoryCommand, PromptHistory};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub conversation_id: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub title: Option<String>,
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

impl SessionRecord {
    pub fn relative_time(&self) -> String {
        format_relative_time(self.created_at)
    }
}

impl PromptHistory {
    pub(super) fn load_recent_sessions(
        conn: &Connection,
        limit: usize,
    ) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, model, mode, effort, title, created_at
             FROM sessions
             WHERE conversation_id IS NOT NULL AND conversation_id != ''
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                model: row.get(2)?,
                mode: row.get(3)?,
                effort: row.get(4)?,
                title: row.get(5)?,
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
            if record.model.is_none() {
                record.model = existing.model.clone();
            }
            if record.mode.is_none() {
                record.mode = existing.mode.clone();
            }
            if record.effort.is_none() {
                record.effort = existing.effort.clone();
            }
            if existing.title.is_some() {
                record.title = existing.title.clone();
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
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return;
        }
        for s in &mut self.sessions {
            if s.id == id_or_conv || s.conversation_id.as_deref() == Some(id_or_conv) {
                s.title = Some(trimmed.to_string());
            }
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RenameSession {
                id: id_or_conv.to_string(),
                title: trimmed.to_string(),
            });
        }
    }

    pub fn rename_agent_title(&mut self, conv_id: &str, new_title: &str) {
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.agent_cache.write() {
            if let Some(item) = guard
                .iter_mut()
                .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            {
                item.title = Some(trimmed.to_string());
            } else {
                guard.push(SessionRecord {
                    id: format!("ext-{}", conv_id),
                    conversation_id: Some(conv_id.to_string()),
                    model: None,
                    mode: None,
                    effort: None,
                    title: Some(trimmed.to_string()),
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
