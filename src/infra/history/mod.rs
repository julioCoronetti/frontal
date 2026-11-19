use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};

pub mod agent;
mod api;
pub mod messages;
pub mod opencode;
mod sessions;
mod sync;
pub mod telemetry;
pub mod transcript;
mod worker;

pub use agent::*;
pub use messages::{
    conversation_messages_at, conversation_title_at, default_history_path, StoredMessage,
};
pub use sessions::{format_relative_time, infer_backend, SessionRecord, TitleSource};
pub use telemetry::TelemetrySnapshot;
pub use transcript::*;

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
        source: TitleSource,
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
        Self::open_with_external_dbs(
            db_path,
            agent::default_agent_db_path(),
            opencode::default_path(),
        )
    }

    pub fn open_with_agent_db(
        db_path: &Path,
        agent_db_path: Option<PathBuf>,
    ) -> Result<Self, rusqlite::Error> {
        Self::open_with_external_dbs(db_path, agent_db_path, None)
    }

    pub fn open_with_external_dbs(
        db_path: &Path,
        agent_db_path: Option<PathBuf>,
        opencode_db_path: Option<PathBuf>,
    ) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(db_path)?;
        worker::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;
        drop(conn);

        let (tx, rx) = channel::<HistoryCommand>();
        let (agent_updates_tx, agent_updates_rx) = channel::<(String, String)>();
        let agent_cache = Arc::new(RwLock::new(Vec::new()));
        let mut pinned_titles = std::collections::HashMap::new();
        let mut known_models = std::collections::HashMap::new();
        for s in &sessions {
            if let Some(ref cid) = s.conversation_id {
                if s.title_source == TitleSource::Custom {
                    if let Some(ref t) = s.title {
                        pinned_titles.insert(cid.clone(), t.clone());
                    }
                }
                if s.model.is_some() {
                    known_models.insert(cid.clone(), (s.model.clone(), s.effort.clone()));
                }
            }
        }
        worker::spawn_history_worker(
            db_path.to_path_buf(),
            rx,
            agent_db_path,
            opencode_db_path,
            Arc::clone(&agent_cache),
            agent_updates_tx,
            pinned_titles,
            known_models,
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
}

impl Drop for PromptHistory {
    fn drop(&mut self) {
        self.flush();
    }
}
