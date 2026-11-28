use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub mod agent;
mod api;
mod external;
mod hydrate;
pub mod messages;
mod migrations;
pub mod opencode;
mod session_types;
mod sessions;
mod sync;
pub mod telemetry;
pub mod transcript;
mod worker;

pub use agent::*;
pub use hydrate::{
    conversation_title, load_hydration_data, load_transcript_blocks_and_usage,
    resolve_session_telemetry, HydrationData,
};
pub use messages::{
    conversation_messages_at, conversation_title_at, default_history_path, StoredMessage,
};
pub use session_types::{format_relative_time, infer_backend, SessionRecord, TitleSource};
pub use telemetry::TelemetrySnapshot;
pub use transcript::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistorySnapshot {
    pub version: u64,
    pub sessions: Vec<SessionRecord>,
    pub agent_sessions: Vec<SessionRecord>,
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
    DeleteSession {
        keys: Vec<String>,
    },
    LoadEntries {
        done_tx: SyncSender<Vec<String>>,
    },
    LoadHydration {
        conv_id: String,
        done_tx: SyncSender<HydrationData>,
    },
    Flush {
        wait_external: bool,
        done_tx: Sender<()>,
    },
}

#[derive(Clone)]
pub struct HydrationClient {
    write_tx: Option<Sender<HistoryCommand>>,
    db_path: Option<PathBuf>,
}

impl HydrationClient {
    pub fn load(&self, conv_id: &str) -> HydrationData {
        if let Some(ref tx) = self.write_tx {
            let (done_tx, done_rx) = sync_channel(1);
            if tx
                .send(HistoryCommand::LoadHydration {
                    conv_id: conv_id.to_string(),
                    done_tx,
                })
                .is_ok()
            {
                if let Ok(data) = done_rx.recv_timeout(Duration::from_secs(5)) {
                    return data;
                }
            }
        }
        load_hydration_data(conv_id, self.db_path.as_deref())
    }
}

pub(super) struct InMemoryStore {
    pub(super) sessions: Vec<SessionRecord>,
    pub(super) deleted_ids: HashSet<String>,
}

impl InMemoryStore {
    pub(super) fn publish(&self, snapshot_tx: &watch::Sender<Arc<HistorySnapshot>>) {
        let deleted = &self.deleted_ids;
        let sessions: Vec<SessionRecord> = self
            .sessions
            .iter()
            .filter(|s| !sessions::is_deleted_record(s, deleted))
            .cloned()
            .collect();
        snapshot_tx.send_modify(|cur| {
            let mut agent_sessions = cur.agent_sessions.clone();
            agent_sessions.retain(|s| !sessions::is_deleted_record(s, deleted));
            let next_ver = cur.version.saturating_add(1);
            *cur = Arc::new(HistorySnapshot {
                version: next_ver,
                sessions,
                agent_sessions,
            });
        });
    }
}

pub struct PromptHistory {
    entries: Vec<String>,
    entries_loaded: bool,
    entries_rx: Option<watch::Receiver<(bool, Vec<String>)>>,
    history_idx: Option<usize>,
    draft_backup: Option<String>,
    db_path: Option<PathBuf>,
    write_tx: Option<Sender<HistoryCommand>>,
    agent_updates_rx: Option<Receiver<(String, String)>>,
    in_memory: Option<InMemoryStore>,
    snapshot_tx: watch::Sender<Arc<HistorySnapshot>>,
    _snapshot_rx: watch::Receiver<Arc<HistorySnapshot>>,
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
        let (tx, rx) = channel::<HistoryCommand>();
        let (agent_updates_tx, agent_updates_rx) = channel::<(String, String)>();
        let (entries_tx, entries_rx) = watch::channel((false, Vec::new()));
        let (snapshot_tx, _snapshot_rx) = watch::channel(Arc::new(HistorySnapshot {
            version: 0,
            sessions: Vec::new(),
            agent_sessions: Vec::new(),
        }));
        worker::spawn_history_worker(
            db_path.to_path_buf(),
            rx,
            agent_db_path,
            opencode_db_path,
            agent_updates_tx,
            entries_tx,
            snapshot_tx.clone(),
        );

        Ok(Self {
            entries: Vec::new(),
            entries_loaded: false,
            entries_rx: Some(entries_rx),
            history_idx: None,
            draft_backup: None,
            db_path: Some(db_path.to_path_buf()),
            write_tx: Some(tx),
            agent_updates_rx: Some(agent_updates_rx),
            in_memory: None,
            snapshot_tx,
            _snapshot_rx,
        })
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        worker::init_connection(&conn)?;
        let entries = Self::load_recent_entries(&conn, 500)?;
        let sessions = Self::load_recent_sessions(&conn, 500)?;
        let (snapshot_tx, _snapshot_rx) = watch::channel(Arc::new(HistorySnapshot {
            version: 0,
            sessions: sessions.clone(),
            agent_sessions: Vec::new(),
        }));

        Ok(Self {
            entries,
            entries_loaded: true,
            entries_rx: None,
            history_idx: None,
            draft_backup: None,
            db_path: None,
            write_tx: None,
            agent_updates_rx: None,
            in_memory: Some(InMemoryStore {
                sessions,
                deleted_ids: HashSet::new(),
            }),
            snapshot_tx,
            _snapshot_rx,
        })
    }

    pub(super) fn load_deleted_ids(conn: &Connection) -> HashSet<String> {
        let mut deleted = HashSet::new();
        if let Ok(mut stmt) = conn.prepare("SELECT conversation_id FROM deleted_sessions") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                for id in rows.flatten() {
                    deleted.insert(id);
                }
            }
        }
        deleted
    }

    pub(super) fn load_recent_entries(
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

    fn merge_entries(mut loaded: Vec<String>, local: &[String]) -> Vec<String> {
        for item in local {
            if loaded.last().map(|s| s.as_str()) != Some(item.as_str()) {
                loaded.push(item.clone());
            }
        }
        loaded
    }

    fn ensure_entries_loaded(&mut self) {
        if self.entries_loaded {
            return;
        }
        if let Some(ref rx) = self.entries_rx {
            let guard = rx.borrow();
            if guard.0 {
                let prev_len = self.entries.len();
                self.entries = Self::merge_entries(guard.1.clone(), &self.entries);
                let added = self.entries.len().saturating_sub(prev_len);
                if let Some(idx) = self.history_idx {
                    self.history_idx = Some(idx.saturating_add(added));
                }
                self.entries_loaded = true;
            }
        }
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
}

impl HydrationClient {
    pub fn flush_with_timeout(&self, timeout: Duration) -> bool {
        send_flush_timeout(self.write_tx.as_ref(), true, timeout)
    }
}

fn send_flush_timeout(
    write_tx: Option<&Sender<HistoryCommand>>,
    wait_external: bool,
    timeout: Duration,
) -> bool {
    if let Some(tx) = write_tx {
        let (done_tx, done_rx) = channel();
        if tx
            .send(HistoryCommand::Flush {
                wait_external,
                done_tx,
            })
            .is_ok()
        {
            return done_rx.recv_timeout(timeout).is_ok();
        }
    }
    true
}

impl PromptHistory {
    pub fn snapshot(&self) -> Arc<HistorySnapshot> {
        self.snapshot_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<HistorySnapshot>> {
        self.snapshot_tx.subscribe()
    }

    pub fn hydration_client(&self) -> HydrationClient {
        HydrationClient {
            write_tx: self.write_tx.clone(),
            db_path: self.db_path.clone(),
        }
    }

    pub fn flush(&self) {
        let _ = self.flush_with_timeout(Duration::from_millis(2000));
    }

    pub fn flush_with_timeout(&self, timeout: Duration) -> bool {
        self.send_flush_timeout(true, timeout)
    }

    fn flush_db(&self) {
        let _ = self.send_flush_timeout(false, Duration::from_millis(500));
    }

    pub(super) fn send_flush_timeout(&self, wait_external: bool, timeout: Duration) -> bool {
        send_flush_timeout(self.write_tx.as_ref(), wait_external, timeout)
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
        self.ensure_entries_loaded();
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

    pub fn entries(&self) -> Vec<String> {
        if self.entries_loaded {
            return self.entries.clone();
        }
        if let Some(ref rx) = self.entries_rx {
            let guard = rx.borrow();
            if guard.0 {
                return Self::merge_entries(guard.1.clone(), &self.entries);
            }
        }
        if let Some(ref tx) = self.write_tx {
            let (done_tx, done_rx) = sync_channel(1);
            if tx.send(HistoryCommand::LoadEntries { done_tx }).is_ok() {
                if let Ok(loaded) = done_rx.recv_timeout(Duration::from_millis(2000)) {
                    return Self::merge_entries(loaded, &self.entries);
                }
            }
        }
        self.entries.clone()
    }

    pub fn is_navigating(&self) -> bool {
        self.history_idx.is_some()
    }
}

impl Drop for PromptHistory {
    fn drop(&mut self) {
        self.flush_db();
    }
}
