use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod agent;
mod api;
pub mod messages;
pub mod opencode;
pub mod telemetry;
pub mod transcript;
mod worker;

pub use agent::*;
pub use messages::{
    conversation_messages_at, conversation_title_at, default_history_path, StoredMessage,
};
pub use telemetry::TelemetrySnapshot;
pub use transcript::*;

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

pub enum HistoryCommand {
    Record(String),
    RecordSession(SessionRecord),
    TouchSession {
        id: String,
        timestamp: i64,
    },
    RenameSession {
        id: String,
        title: String,
    },
    RenameAgentTitle {
        conv_id: String,
        title: String,
    },
    RefreshAgent(Option<String>),
    AppendMessages {
        conversation_id: String,
        messages: Vec<StoredMessage>,
    },
    RecordTelemetry {
        conversation_id: String,
        snapshot: TelemetrySnapshot,
    },
    Flush(Sender<()>),
}

pub struct PromptHistory {
    entries: Vec<String>,
    sessions: Vec<SessionRecord>,
    history_idx: Option<usize>,
    draft_backup: Option<String>,
    db_path: Option<PathBuf>,
    write_tx: Option<Sender<HistoryCommand>>,
    agent_cache: Arc<RwLock<Vec<SessionRecord>>>,
    agent_updates_rx: Option<Receiver<(String, String)>>,
}

impl PromptHistory {
    pub fn open(db_path: &Path) -> Result<Self, rusqlite::Error> {
        Self::open_with_agent_db(db_path, agent::default_agent_db_path())
    }

    pub fn open_with_agent_db(
        db_path: &Path,
        agent_db_path: Option<PathBuf>,
    ) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(db_path)?;
        worker::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;

        let (tx, rx) = channel::<HistoryCommand>();
        let (agent_updates_tx, agent_updates_rx) = channel::<(String, String)>();
        let agent_cache = Arc::new(RwLock::new(Vec::new()));
        worker::spawn_history_worker(
            db_path.to_path_buf(),
            rx,
            agent_db_path,
            Arc::clone(&agent_cache),
            agent_updates_tx,
        );

        Ok(Self {
            entries,
            sessions,
            history_idx: None,
            draft_backup: None,
            db_path: Some(db_path.to_path_buf()),
            write_tx: Some(tx),
            agent_cache,
            agent_updates_rx: Some(agent_updates_rx),
        })
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        worker::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;

        Ok(Self {
            entries,
            sessions,
            history_idx: None,
            draft_backup: None,
            db_path: None,
            write_tx: None,
            agent_cache: Arc::new(RwLock::new(Vec::new())),
            agent_updates_rx: None,
        })
    }

    fn load_recent_sessions(
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

    fn load_recent_entries(
        conn: &Connection,
        limit: usize,
    ) -> Result<Vec<String>, rusqlite::Error> {
        let mut stmt =
            conn.prepare("SELECT prompt FROM prompt_history ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        for r in rows.flatten() {
            if !r.trim().is_empty() {
                result.push(r);
            }
        }
        result.reverse();
        Ok(result)
    }

    pub fn record_prompt(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }

        if self.entries.last().map(|s| s.as_str()) != Some(trimmed) {
            self.entries.push(trimmed.to_string());
            if let Some(ref tx) = self.write_tx {
                let _ = tx.send(HistoryCommand::Record(trimmed.to_string()));
            }
        }

        self.history_idx = None;
        self.draft_backup = None;
    }

    pub fn flush(&self) {
        if let Some(ref tx) = self.write_tx {
            let (done_tx, done_rx) = channel();
            if tx.send(HistoryCommand::Flush(done_tx)).is_ok() {
                let _ = done_rx.recv();
            }
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    pub fn append_messages(&self, conversation_id: String, messages: Vec<StoredMessage>) {
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::AppendMessages {
                conversation_id,
                messages,
            });
        }
    }

    pub fn prev_history(&mut self, current_draft: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        if self.history_idx.is_none() {
            self.draft_backup = Some(current_draft.to_string());
            let last_idx = self.entries.len() - 1;
            self.history_idx = Some(last_idx);
            return self.entries.get(last_idx).cloned();
        }

        let idx = self.history_idx?;
        if idx > 0 {
            let prev_idx = idx - 1;
            self.history_idx = Some(prev_idx);
            self.entries.get(prev_idx).cloned()
        } else {
            self.entries.first().cloned()
        }
    }

    pub fn next_history(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        let idx = self.history_idx?;
        if idx + 1 < self.entries.len() {
            let next_idx = idx + 1;
            self.history_idx = Some(next_idx);
            self.entries.get(next_idx).cloned()
        } else {
            self.history_idx = None;
            self.draft_backup.take()
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn is_navigating(&self) -> bool {
        self.history_idx.is_some()
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

impl Drop for PromptHistory {
    fn drop(&mut self) {
        self.flush();
    }
}
