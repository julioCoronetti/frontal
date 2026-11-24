use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use crate::config::BackendKind;

use super::session_types::{
    collect_matching_keys, infer_backend, matches_id, SessionRecord, TitleSource,
};
pub(super) use super::session_types::{is_deleted_key, is_deleted_record, normalize_title_source};
use super::{HistoryCommand, PromptHistory};

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
        Ok(rows.flatten().collect())
    }

    pub fn record_session(&mut self, mut record: SessionRecord) {
        if record.id.trim().is_empty() || is_deleted_record(&record, &self.deleted_ids) {
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

        if !self.is_in_memory() {
            if let Some(ref cid) = record.conversation_id {
                if let Some((cached_title, cached_source)) = self.cached_agent_entry(cid) {
                    if cached_source == TitleSource::Custom
                        || (record.title_source != TitleSource::Custom
                            && cached_source > record.title_source)
                    {
                        record.title = Some(cached_title);
                        record.title_source = cached_source;
                    }
                }
                if record.created_at <= 0 {
                    if let Some(ts) = self.cached_agent_timestamp(cid) {
                        record.created_at = ts;
                    }
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
        let key = id_or_conv.trim();
        let norm = key.strip_prefix("ext-").unwrap_or(key);
        let trimmed = new_title.trim();
        if trimmed.is_empty() || norm.is_empty() || is_deleted_key(&self.deleted_ids, key) {
            return;
        }
        let mut resolved_cid: Option<String> = None;
        for s in &mut self.sessions {
            if matches_id(s, key, norm) {
                if is_deleted_record(s, &self.deleted_ids) {
                    return;
                }
                if resolved_cid.is_none() {
                    resolved_cid = s.conversation_id.clone().filter(|c| !c.trim().is_empty());
                }
                if source == TitleSource::Custom || source >= s.title_source {
                    s.title = Some(trimmed.to_string());
                    s.title_source = source;
                }
            }
        }
        if resolved_cid
            .as_deref()
            .is_some_and(|c| is_deleted_key(&self.deleted_ids, c))
        {
            return;
        }
        let target_norm = resolved_cid.as_deref().unwrap_or(norm);
        if source == TitleSource::Custom {
            if let Ok(mut guard) = self.agent_cache.write() {
                for item in guard.iter_mut() {
                    if !is_deleted_record(item, &self.deleted_ids)
                        && (matches_id(item, key, norm)
                            || matches_id(item, target_norm, target_norm))
                    {
                        item.title = Some(trimmed.to_string());
                        item.title_source = TitleSource::Custom;
                    }
                }
            }
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RenameSession {
                id: target_norm.to_string(),
                title: trimmed.to_string(),
                source,
            });
        }
    }

    pub fn rename_agent_title(&mut self, conv_id: &str, new_title: &str) {
        let trimmed = new_title.trim();
        let norm = conv_id.strip_prefix("ext-").unwrap_or(conv_id);
        if trimmed.is_empty() || norm.is_empty() || is_deleted_key(&self.deleted_ids, conv_id) {
            return;
        }
        for s in &mut self.sessions {
            if matches_id(s, conv_id, norm) {
                s.title = Some(trimmed.to_string());
                s.title_source = TitleSource::Custom;
            }
        }
        if let Ok(mut guard) = self.agent_cache.write() {
            if let Some(item) = guard
                .iter_mut()
                .find(|s| s.conversation_id.as_deref() == Some(norm))
            {
                item.title = Some(trimmed.to_string());
                item.title_source = TitleSource::Custom;
            } else {
                guard.push(SessionRecord {
                    id: format!("ext-{}", norm),
                    conversation_id: Some(norm.to_string()),
                    backend: Some(infer_backend(Some(norm), None)),
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
                conv_id: norm.to_string(),
                title: trimmed.to_string(),
            });
        }
    }

    pub fn cached_agent_title(&self, conv_id: &str) -> Option<String> {
        let guard = self.agent_cache.read().ok()?;
        guard
            .iter()
            .find(|s| {
                !is_deleted_record(s, &self.deleted_ids)
                    && s.conversation_id.as_deref() == Some(conv_id)
            })
            .and_then(|s| s.title.clone())
    }

    pub fn cached_agent_entry(&self, conv_id: &str) -> Option<(String, TitleSource)> {
        let guard = self.agent_cache.read().ok()?;
        let item = guard.iter().find(|s| {
            !is_deleted_record(s, &self.deleted_ids)
                && s.conversation_id.as_deref() == Some(conv_id)
        })?;
        let title = item.title.clone()?;
        Some((title, item.title_source))
    }

    pub fn cached_agent_timestamp(&self, conv_id: &str) -> Option<i64> {
        let guard = self.agent_cache.read().ok()?;
        guard
            .iter()
            .find(|s| {
                !is_deleted_record(s, &self.deleted_ids)
                    && s.conversation_id.as_deref() == Some(conv_id)
            })
            .map(|s| s.created_at)
            .filter(|ts| *ts > 0)
    }

    pub fn cached_agent_conversations(&self) -> Vec<SessionRecord> {
        self.agent_cache
            .read()
            .map(|guard| {
                guard
                    .iter()
                    .filter(|s| !is_deleted_record(s, &self.deleted_ids))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn delete_session(&mut self, id_or_conv: &str) {
        let trimmed = id_or_conv.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut keys = HashSet::from([trimmed.to_string()]);
        if let Some(raw) = trimmed.strip_prefix("ext-").filter(|r| !r.is_empty()) {
            keys.insert(raw.to_string());
        }
        collect_matching_keys(&self.sessions, &mut keys);
        if let Ok(mut guard) = self.agent_cache.write() {
            collect_matching_keys(&guard, &mut keys);
            self.deleted_ids.extend(keys.iter().cloned());
            guard.retain(|s| !is_deleted_record(s, &self.deleted_ids));
        } else {
            self.deleted_ids.extend(keys.iter().cloned());
        }
        self.sessions
            .retain(|s| !is_deleted_record(s, &self.deleted_ids));
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::DeleteSession {
                keys: keys.into_iter().collect(),
            });
        }
    }

    pub fn request_agent_refresh(&self, conv_id: Option<String>) {
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RefreshAgent(conv_id));
        }
    }

    pub fn poll_agent_title_updates(&mut self) -> Vec<(String, String)> {
        let mut updates = Vec::new();
        if let Some(ref rx) = self.agent_updates_rx {
            while let Ok(item) = rx.try_recv() {
                let norm = item.0.strip_prefix("ext-").unwrap_or(&item.0);
                for s in &mut self.sessions {
                    if matches_id(s, &item.0, norm) && s.title_source <= TitleSource::Semantic {
                        s.title = Some(item.1.clone());
                        s.title_source = TitleSource::Semantic;
                    }
                }
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
                !is_deleted_record(s, &self.deleted_ids)
                    && s.conversation_id
                        .as_ref()
                        .map(|c| !c.trim().is_empty())
                        .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}
