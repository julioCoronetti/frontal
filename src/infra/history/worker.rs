use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::{agent, messages, sync, telemetry, HistoryCommand, SessionRecord, TitleSource};

pub(super) fn init_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
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
             backend TEXT,
             model TEXT,
             mode TEXT,
             effort TEXT,
             title TEXT,
             title_source INTEGER NOT NULL DEFAULT 0,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS messages (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             conversation_id TEXT NOT NULL,
             role TEXT NOT NULL,
             content TEXT NOT NULL,
             tool_name TEXT,
             tool_args TEXT,
             step_index INTEGER,
             turn_meta TEXT,
             created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_messages_conversation
             ON messages(conversation_id, id);",
    )?;
    telemetry::ensure_schema(conn)?;
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN effort TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN custom_title INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN backend TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN title_source INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN turn_meta TEXT", []);
    let _ = conn.execute_batch(
        "UPDATE sessions SET title_source = 3 WHERE custom_title != 0 AND title_source < 3;
         UPDATE sessions SET title_source = 2
             WHERE custom_title = 0 AND title_source = 0
               AND title IS NOT NULL AND title != '' AND title != 'New session';
         UPDATE sessions SET backend = CASE
             WHEN conversation_id LIKE 'ses_%' THEN 'opencode'
             ELSE 'agy'
         END
         WHERE backend IS NULL AND conversation_id IS NOT NULL AND conversation_id != '';",
    );
    Ok(())
}

pub(super) fn spawn_history_worker(
    bg_path: PathBuf,
    rx: Receiver<HistoryCommand>,
    agent_db_path: Option<PathBuf>,
    opencode_db_path: Option<PathBuf>,
    agent_cache: Arc<RwLock<Vec<SessionRecord>>>,
    agent_updates_tx: Sender<(String, String)>,
) {
    if agent_db_path.is_some() || opencode_db_path.is_some() {
        let cache_clone = Arc::clone(&agent_cache);
        let adb_clone = agent_db_path.clone();
        let odb_clone = opencode_db_path.clone();
        thread::spawn(move || {
            let records = sync::load_external_conversations(
                adb_clone.as_deref(),
                odb_clone.as_deref(),
                &std::collections::HashMap::new(),
            );
            if let Ok(mut guard) = cache_clone.write() {
                if guard.is_empty() {
                    *guard = records;
                } else {
                    for rec in records {
                        if let Some(ref cid) = rec.conversation_id {
                            if !guard
                                .iter()
                                .any(|existing| existing.conversation_id.as_deref() == Some(cid))
                            {
                                guard.push(rec);
                            }
                        }
                    }
                }
            }
        });
    }
    thread::spawn(move || {
        let Ok(bg_conn) = Connection::open(&bg_path) else {
            return;
        };
        let _ = init_connection(&bg_conn);
        let mut pinned_titles = sync::load_pinned_titles(&bg_conn);
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
                    sync::handle_record_session(
                        &bg_conn,
                        record,
                        &mut pinned_titles,
                        agent_db_path.as_deref(),
                        opencode_db_path.as_deref(),
                    );
                }
                HistoryCommand::TouchSession { id, timestamp } => {
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET created_at = ?1 WHERE id = ?2 OR conversation_id = ?2",
                        params![timestamp, id],
                    );
                }
                HistoryCommand::RenameSession { id, title, source } => {
                    if source == TitleSource::Custom {
                        let _ = bg_conn.execute(
                            "UPDATE sessions SET title = ?1, title_source = 3, custom_title = 1
                             WHERE id = ?2 OR conversation_id = ?2",
                            params![title, id],
                        );
                    } else {
                        let _ = bg_conn.execute(
                            "UPDATE sessions SET title = ?1, title_source = ?2
                             WHERE (id = ?3 OR conversation_id = ?3)
                               AND custom_title = 0
                               AND title_source <= ?2",
                            params![title, source.as_i64(), id],
                        );
                    }
                }
                HistoryCommand::RenameAgentTitle { conv_id, title } => {
                    pinned_titles.insert(conv_id.clone(), title.clone());
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET title = ?1, title_source = 3, custom_title = 1
                         WHERE id = ?2 OR conversation_id = ?2",
                        params![title, conv_id],
                    );
                    if !conv_id.starts_with("ses_") {
                        if let Some(ref adb) = agent_db_path {
                            let _ =
                                agent::update_agent_conversation_title_at(adb, &conv_id, &title);
                        }
                    }
                    sync::update_cache_record(
                        &agent_cache,
                        &conv_id,
                        Some(title),
                        TitleSource::Custom,
                        0,
                    );
                }
                HistoryCommand::RefreshAgent(maybe_cid) => {
                    sync::handle_refresh_agent(
                        &bg_conn,
                        maybe_cid,
                        &pinned_titles,
                        agent_db_path.as_deref(),
                        opencode_db_path.as_deref(),
                        &agent_cache,
                        &agent_updates_tx,
                    );
                }
                HistoryCommand::AppendMessages {
                    conversation_id,
                    messages,
                } => {
                    messages::insert_messages(&bg_conn, &conversation_id, &messages);
                }
                HistoryCommand::RecordTelemetry {
                    conversation_id,
                    snapshot,
                } => {
                    let _ = telemetry::upsert(&bg_conn, &conversation_id, &snapshot);
                }
                HistoryCommand::Flush(done_tx) => {
                    let _ = done_tx.send(());
                }
            }
        }
    });
}
