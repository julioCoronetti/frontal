use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

pub enum HistoryCommand {
    Record(String),
    RecordSession(SessionRecord),
    Flush(Sender<()>),
}

pub struct PromptHistory {
    entries: Vec<String>,
    sessions: Vec<SessionRecord>,
    history_idx: Option<usize>,
    draft_backup: Option<String>,
    write_tx: Option<Sender<HistoryCommand>>,
}

impl PromptHistory {
    pub fn open(db_path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(db_path)?;
        Self::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;

        let (tx, rx) = channel::<HistoryCommand>();
        let bg_path = db_path.to_path_buf();
        thread::spawn(move || {
            if let Ok(bg_conn) = Connection::open(&bg_path) {
                let _ = Self::init_connection(&bg_conn);
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        HistoryCommand::Record(prompt) => {
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or(Duration::ZERO)
                                .as_secs() as i64;
                            let _ = bg_conn.execute(
                                "INSERT INTO prompt_history (prompt, created_at) VALUES (?1, ?2)",
                                params![prompt, now],
                            );
                        }
                        HistoryCommand::RecordSession(record) => {
                            if let Some(ref cid) = record.conversation_id {
                                if !cid.trim().is_empty() {
                                    let _ = bg_conn.execute(
                                        "DELETE FROM sessions WHERE conversation_id = ?1 AND id != ?2",
                                        params![cid, record.id],
                                    );
                                }
                            }
                            let _ = bg_conn.execute(
                                "INSERT INTO sessions (id, conversation_id, model, mode, effort, title, created_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                                 ON CONFLICT(id) DO UPDATE SET
                                     conversation_id = COALESCE(excluded.conversation_id, sessions.conversation_id),
                                     model = COALESCE(excluded.model, sessions.model),
                                     mode = COALESCE(excluded.mode, sessions.mode),
                                     effort = COALESCE(excluded.effort, sessions.effort),
                                     title = COALESCE(sessions.title, excluded.title),
                                     created_at = excluded.created_at",
                                params![
                                    record.id,
                                    record.conversation_id,
                                    record.model,
                                    record.mode,
                                    record.effort,
                                    record.title,
                                    record.created_at,
                                ],
                            );
                        }
                        HistoryCommand::Flush(done_tx) => {
                            let _ = done_tx.send(());
                        }
                    }
                }
            }
        });

        Ok(Self {
            entries,
            sessions,
            history_idx: None,
            draft_backup: None,
            write_tx: Some(tx),
        })
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;

        Ok(Self {
            entries,
            sessions,
            history_idx: None,
            draft_backup: None,
            write_tx: None,
        })
    }

    fn init_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.busy_timeout(Duration::from_millis(5000))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS prompt_history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 prompt TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 conversation_id TEXT,
                 model TEXT,
                 mode TEXT,
                 effort TEXT,
                 title TEXT,
                 created_at INTEGER NOT NULL
             );",
        )?;
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN effort TEXT", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", []);
        Ok(())
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
        if record.created_at == 0 {
            record.created_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs() as i64;
        }
        if let Some(pos) = self.sessions.iter().position(|s| {
            s.id == record.id
                || (record.conversation_id.is_some()
                    && s.conversation_id.is_some()
                    && s.conversation_id == record.conversation_id)
        }) {
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
            self.sessions.remove(pos);
        }
        self.sessions.insert(0, record.clone());
        if self.sessions.len() > 500 {
            self.sessions.truncate(500);
        }
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RecordSession(record));
        }
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
