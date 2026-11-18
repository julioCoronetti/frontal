use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::{agent, messages, telemetry, HistoryCommand, SessionRecord};

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
             model TEXT,
             mode TEXT,
             effort TEXT,
             title TEXT,
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
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN turn_meta TEXT", []);
    Ok(())
}

pub(super) fn spawn_history_worker(
    bg_path: PathBuf,
    rx: Receiver<HistoryCommand>,
    agent_db_path: Option<PathBuf>,
    agent_cache: Arc<RwLock<Vec<SessionRecord>>>,
    agent_updates_tx: Sender<(String, String)>,
) {
    if let Some(adb) = agent_db_path.clone() {
        let cache_clone = Arc::clone(&agent_cache);
        thread::spawn(move || {
            let records = agent::load_agent_conversations_at(&adb);
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
        let mut pinned_titles: HashMap<String, String> = HashMap::new();
        if let Ok(mut stmt) = bg_conn.prepare(
            "SELECT conversation_id, title FROM sessions
             WHERE custom_title != 0 AND conversation_id IS NOT NULL AND title IS NOT NULL",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                for (cid, title) in rows.flatten() {
                    pinned_titles.insert(cid, title);
                }
            }
        }
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
                HistoryCommand::RecordSession(mut record) => {
                    let mut is_custom = false;
                    if let Some(ref cid) = record.conversation_id {
                        if !cid.trim().is_empty() {
                            if let Some(pinned) = pinned_titles.get(cid) {
                                is_custom = true;
                                record.title = Some(pinned.clone());
                            } else if let Ok(mut stmt) = bg_conn.prepare(
                                "SELECT title FROM sessions
                                 WHERE (conversation_id = ?1 OR id = ?2) AND custom_title != 0
                                 LIMIT 1",
                            ) {
                                if let Ok(existing_title) = stmt
                                    .query_row(params![cid, record.id], |row| {
                                        row.get::<_, Option<String>>(0)
                                    })
                                {
                                    is_custom = true;
                                    if let Some(t) = existing_title.filter(|t| !t.trim().is_empty())
                                    {
                                        pinned_titles.insert(cid.clone(), t.clone());
                                        record.title = Some(t);
                                    }
                                }
                            }
                            if let Some(ref adb) = agent_db_path {
                                if let Some(ts) =
                                    agent::query_agent_conversation_timestamp_at(adb, cid)
                                {
                                    record.created_at = ts;
                                }
                            }
                            let _ = bg_conn.execute(
                                "DELETE FROM sessions WHERE conversation_id = ?1 AND id != ?2",
                                params![cid, record.id],
                            );
                        }
                    }
                    let custom_flag = i64::from(is_custom);
                    let _ = bg_conn.execute(
                        "INSERT INTO sessions (id, conversation_id, model, mode, effort, title, custom_title, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(id) DO UPDATE SET
                             conversation_id = COALESCE(excluded.conversation_id, sessions.conversation_id),
                             model = COALESCE(excluded.model, sessions.model),
                             mode = COALESCE(excluded.mode, sessions.mode),
                             effort = COALESCE(excluded.effort, sessions.effort),
                             title = CASE
                                 WHEN excluded.custom_title != 0 AND excluded.title IS NOT NULL THEN excluded.title
                                 WHEN sessions.title IS NULL OR sessions.title = '' OR sessions.title = 'New session' THEN COALESCE(excluded.title, sessions.title)
                                 ELSE COALESCE(sessions.title, excluded.title)
                             END,
                             custom_title = MAX(sessions.custom_title, excluded.custom_title),
                             created_at = CASE
                                 WHEN excluded.created_at > 0 THEN excluded.created_at
                                 ELSE sessions.created_at
                             END",
                        params![
                            record.id,
                            record.conversation_id,
                            record.model,
                            record.mode,
                            record.effort,
                            record.title,
                            custom_flag,
                            record.created_at,
                        ],
                    );
                }
                HistoryCommand::TouchSession { id, timestamp } => {
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET created_at = ?1 WHERE id = ?2 OR conversation_id = ?2",
                        params![timestamp, id],
                    );
                }
                HistoryCommand::RenameSession { id, title } => {
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET title = ?1 WHERE id = ?2 OR conversation_id = ?2",
                        params![title, id],
                    );
                }
                HistoryCommand::RenameAgentTitle { conv_id, title } => {
                    pinned_titles.insert(conv_id.clone(), title.clone());
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET title = ?1, custom_title = 1 WHERE id = ?2 OR conversation_id = ?2",
                        params![title, conv_id],
                    );
                    if let Some(ref adb) = agent_db_path {
                        let _ = agent::update_agent_conversation_title_at(adb, &conv_id, &title);
                    }
                    if let Ok(mut guard) = agent_cache.write() {
                        if let Some(item) = guard
                            .iter_mut()
                            .find(|s| s.conversation_id.as_deref() == Some(conv_id.as_str()))
                        {
                            item.title = Some(title);
                        } else {
                            guard.push(SessionRecord {
                                id: format!("ext-{}", conv_id),
                                conversation_id: Some(conv_id),
                                model: None,
                                mode: None,
                                effort: None,
                                title: Some(title),
                                created_at: 0,
                            });
                        }
                    }
                }
                HistoryCommand::RefreshAgent(maybe_cid) => {
                    let Some(ref adb) = agent_db_path else {
                        continue;
                    };
                    match maybe_cid {
                        Some(cid) => {
                            let pinned = pinned_titles.get(&cid).cloned();
                            let title = if pinned.is_some() {
                                None
                            } else {
                                agent::query_agent_conversation_title_at(adb, &cid)
                            };
                            let ts = agent::query_agent_conversation_timestamp_at(adb, &cid)
                                .unwrap_or(0);
                            if let Ok(mut guard) = agent_cache.write() {
                                if let Some(existing) = guard
                                    .iter_mut()
                                    .find(|s| s.conversation_id.as_deref() == Some(cid.as_str()))
                                {
                                    if let Some(ref pinned_title) = pinned {
                                        existing.title = Some(pinned_title.clone());
                                    } else if title.is_some() {
                                        existing.title = title.clone();
                                    }
                                    if ts > 0 {
                                        existing.created_at = ts;
                                    }
                                } else {
                                    guard.push(SessionRecord {
                                        id: format!("ext-{}", cid),
                                        conversation_id: Some(cid.clone()),
                                        model: None,
                                        mode: None,
                                        effort: None,
                                        title: pinned.clone().or_else(|| title.clone()),
                                        created_at: ts,
                                    });
                                }
                            }
                            if let Some(resolved_title) = title {
                                let _ = agent_updates_tx.send((cid, resolved_title));
                            }
                        }
                        None => {
                            let mut records = agent::load_agent_conversations_at(adb);
                            for (cid, pinned_title) in &pinned_titles {
                                if let Some(existing) = records
                                    .iter_mut()
                                    .find(|s| s.conversation_id.as_deref() == Some(cid.as_str()))
                                {
                                    existing.title = Some(pinned_title.clone());
                                } else {
                                    records.push(SessionRecord {
                                        id: format!("ext-{}", cid),
                                        conversation_id: Some(cid.clone()),
                                        model: None,
                                        mode: None,
                                        effort: None,
                                        title: Some(pinned_title.clone()),
                                        created_at: 0,
                                    });
                                }
                            }
                            if let Ok(mut guard) = agent_cache.write() {
                                *guard = records;
                            }
                        }
                    }
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
