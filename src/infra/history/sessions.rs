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
        let snapshot_tx = &self.snapshot_tx;
        if let Some(ref mut mem) = self.in_memory {
            if is_deleted_record(&record, &mem.deleted_ids) {
                return;
            }
            let existing_pos = mem.sessions.iter().position(|s| {
                s.id == record.id
                    || (record.conversation_id.is_some()
                        && s.conversation_id.is_some()
                        && s.conversation_id == record.conversation_id)
            });
            if let Some(pos) = existing_pos {
                let existing = &mem.sessions[pos];
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
                let existing_source =
                    normalize_title_source(&existing.title, existing.title_source);
                if existing.title.is_some()
                    && (!record.title_source.can_override(existing_source)
                        || existing_source == record.title_source
                        || record.title.is_none())
                {
                    record.title = existing.title.clone();
                    record.title_source = existing_source;
                }
                if record.created_at <= 0 {
                    record.created_at = existing.created_at;
                }
                mem.sessions.remove(pos);
            }
            if record.created_at <= 0 {
                record.created_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs() as i64;
            }
            mem.sessions.push(record);
            mem.sessions
                .sort_by_key(|s| std::cmp::Reverse(s.created_at));
            if mem.sessions.len() > 500 {
                mem.sessions.truncate(500);
            }
            mem.publish(snapshot_tx);
            return;
        }

        if let Some(ref cid) = record.conversation_id {
            if let Some((cached_title, cached_source)) = self.cached_agent_entry(cid) {
                if cached_source == TitleSource::Custom
                    || (record.title_source != TitleSource::Custom
                        && cached_source.can_override(record.title_source)
                        && cached_source != record.title_source)
                {
                    record.title = Some(cached_title);
                    record.title_source = cached_source;
                }
            }
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
        let snapshot_tx = &self.snapshot_tx;
        if let Some(ref mut mem) = self.in_memory {
            for s in &mut mem.sessions {
                if s.id == id_or_conv || s.conversation_id.as_deref() == Some(id_or_conv) {
                    s.created_at = now;
                }
            }
            mem.sessions
                .sort_by_key(|s| std::cmp::Reverse(s.created_at));
            mem.publish(snapshot_tx);
            return;
        }
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
        if trimmed.is_empty() || norm.is_empty() {
            return;
        }
        let snapshot_tx = &self.snapshot_tx;
        if let Some(ref mut mem) = self.in_memory {
            if is_deleted_key(&mem.deleted_ids, key) {
                return;
            }
            for s in &mut mem.sessions {
                if matches_id(s, key, norm) {
                    if is_deleted_record(s, &mem.deleted_ids) {
                        return;
                    }
                    if source == TitleSource::Custom || source.can_override(s.title_source) {
                        s.title = Some(trimmed.to_string());
                        s.title_source = source;
                    }
                }
            }
            mem.publish(snapshot_tx);
            return;
        }
        let snap = self.snapshot();
        let resolved_cid = snap
            .sessions
            .iter()
            .find(|s| matches_id(s, key, norm))
            .and_then(|s| s.conversation_id.as_deref())
            .filter(|c| !c.trim().is_empty());
        let target_norm = resolved_cid.unwrap_or(norm);
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
        if trimmed.is_empty() || norm.is_empty() {
            return;
        }
        let snapshot_tx = &self.snapshot_tx;
        if let Some(ref mut mem) = self.in_memory {
            if is_deleted_key(&mem.deleted_ids, conv_id) {
                return;
            }
            for s in &mut mem.sessions {
                if matches_id(s, conv_id, norm) {
                    s.title = Some(trimmed.to_string());
                    s.title_source = TitleSource::Custom;
                }
            }
            mem.publish(snapshot_tx);
            return;
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RenameAgentTitle {
                conv_id: norm.to_string(),
                title: trimmed.to_string(),
            });
        }
    }

    pub fn cached_agent_title(&self, conv_id: &str) -> Option<String> {
        if self.write_tx.is_some() {
            let _ = self.send_flush_timeout(false, Duration::from_millis(2000));
        }
        let snap = self.snapshot();
        snap.agent_sessions
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            .and_then(|s| s.title.clone())
    }

    pub fn cached_agent_entry(&self, conv_id: &str) -> Option<(String, TitleSource)> {
        let snap = self.snapshot();
        let item = snap
            .agent_sessions
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))?;
        let title = item.title.clone()?;
        Some((title, item.title_source))
    }

    pub fn cached_agent_timestamp(&self, conv_id: &str) -> Option<i64> {
        if self.write_tx.is_some() {
            let _ = self.send_flush_timeout(false, Duration::from_millis(2000));
        }
        let snap = self.snapshot();
        snap.agent_sessions
            .iter()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))
            .map(|s| s.created_at)
            .filter(|ts| *ts > 0)
    }

    pub fn cached_agent_conversations(&self) -> Vec<SessionRecord> {
        if self.write_tx.is_some() {
            let _ = self.send_flush_timeout(false, Duration::from_millis(2000));
        }
        self.snapshot().agent_sessions.clone()
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
        let snap = self.snapshot();
        collect_matching_keys(&snap.sessions, &mut keys);
        collect_matching_keys(&snap.agent_sessions, &mut keys);
        let snapshot_tx = &self.snapshot_tx;
        if let Some(ref mut mem) = self.in_memory {
            collect_matching_keys(&mem.sessions, &mut keys);
            mem.deleted_ids.extend(keys.iter().cloned());
            let deleted = &mem.deleted_ids;
            mem.sessions.retain(|s| !is_deleted_record(s, deleted));
            mem.publish(snapshot_tx);
            return;
        }
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
                updates.push(item);
            }
        }
        updates
    }

    pub fn sessions(&self) -> Vec<SessionRecord> {
        if self.write_tx.is_some() {
            let _ = self.send_flush_timeout(false, Duration::from_millis(2000));
        }
        self.snapshot().sessions.clone()
    }

    pub fn resumable_sessions(&self) -> Vec<SessionRecord> {
        if self.write_tx.is_some() {
            let _ = self.send_flush_timeout(false, Duration::from_millis(2000));
        }
        self.snapshot()
            .sessions
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
