use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub enum HistoryCommand {
    Record(String),
    Flush(Sender<()>),
}

pub struct PromptHistory {
    entries: Vec<String>,
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
                        HistoryCommand::Flush(done_tx) => {
                            let _ = done_tx.send(());
                        }
                    }
                }
            }
        });

        Ok(Self {
            entries,
            history_idx: None,
            draft_backup: None,
            write_tx: Some(tx),
        })
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;

        Ok(Self {
            entries,
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
                 created_at INTEGER NOT NULL
             );",
        )?;
        Ok(())
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
}
