use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use tokio::sync::watch;

use super::sessions::{is_deleted_key, is_deleted_record};
use super::{
    agent, external, hydrate, messages, migrations, sync, telemetry, HistoryCommand,
    HistorySnapshot, PromptHistory, SessionRecord, TitleSource,
};

pub(super) fn init_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
    migrations::migrate(conn)
}

pub(super) fn spawn_history_worker(
    bg_path: PathBuf,
    rx: Receiver<HistoryCommand>,
    agent_db_path: Option<PathBuf>,
    opencode_db_path: Option<PathBuf>,
    agent_updates_tx: Sender<(String, String)>,
    entries_tx: watch::Sender<(bool, Vec<String>)>,
    snapshot_tx: watch::Sender<Arc<HistorySnapshot>>,
) {
    thread::spawn(move || {
        if let Some(parent) = bg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(bg_conn) = Connection::open(&bg_path) else {
            return;
        };
        if init_connection(&bg_conn).is_err() {
            return;
        }
        let initial_entries = PromptHistory::load_recent_entries(&bg_conn, 500).unwrap_or_default();
        let _ = entries_tx.send((true, initial_entries));
        let mut deleted_ids = PromptHistory::load_deleted_ids(&bg_conn);
        let mut sessions = PromptHistory::load_recent_sessions(&bg_conn, 500).unwrap_or_default();
        sessions.retain(|s| !is_deleted_record(s, &deleted_ids));
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
        let agent_cache = Arc::new(RwLock::new(Vec::new()));
        let publish_snapshot = |conn: &Connection, deleted: &HashSet<String>, bump: bool| {
            if let Ok(mut list) = PromptHistory::load_recent_sessions(conn, 500) {
                list.retain(|s| !is_deleted_record(s, deleted));
                let agent_sessions: Vec<SessionRecord> = agent_cache
                    .read()
                    .map(|g| {
                        g.iter()
                            .filter(|s| !is_deleted_record(s, deleted))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                snapshot_tx.send_modify(|cur| {
                    let next_ver = if bump {
                        cur.version.saturating_add(1)
                    } else {
                        cur.version
                    };
                    *cur = Arc::new(HistorySnapshot {
                        version: next_ver,
                        sessions: list,
                        agent_sessions,
                    });
                });
            }
        };
        pinned_titles.extend(sync::load_pinned_titles(&bg_conn));
        let mut probed_empty = std::collections::HashSet::new();
        if agent_db_path.is_some() || opencode_db_path.is_some() {
            let mut records = external::load_external_sql_conversations(
                agent_db_path.as_deref(),
                opencode_db_path.as_deref(),
                &known_models,
            );
            external::apply_pinned_titles(&bg_conn, &mut records, &pinned_titles, &deleted_ids);
            records.retain(|r| !is_deleted_record(r, &deleted_ids));
            if let Ok(mut guard) = agent_cache.write() {
                *guard = records;
            }
            publish_snapshot(&bg_conn, &deleted_ids, true);
        } else if !sessions.is_empty() {
            publish_snapshot(&bg_conn, &deleted_ids, false);
        }
        let mut pending_deletes: Vec<thread::JoinHandle<()>> = Vec::new();
        while let Ok(cmd) = rx.recv() {
            pending_deletes.retain(|h| !h.is_finished());
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
                    if is_deleted_record(&record, &deleted_ids) {
                        continue;
                    }
                    sync::handle_record_session(
                        &bg_conn,
                        record,
                        &mut pinned_titles,
                        agent_db_path.as_deref(),
                        opencode_db_path.as_deref(),
                    );
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::TouchSession { id, timestamp } => {
                    let _ = bg_conn.execute(
                        "UPDATE sessions SET created_at = ?1 WHERE id = ?2 OR conversation_id = ?2",
                        params![timestamp, id],
                    );
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::RenameSession { id, title, source } => {
                    let norm = id.strip_prefix("ext-").unwrap_or(&id);
                    if norm.is_empty()
                        || is_deleted_key(&deleted_ids, &id)
                        || is_deleted_key(&deleted_ids, norm)
                    {
                        continue;
                    }
                    let conv_id: Option<String> = bg_conn
                        .query_row(
                            "SELECT conversation_id FROM sessions
                             WHERE id = ?1 OR id = ?2 OR conversation_id = ?2
                             LIMIT 1",
                            params![id, norm],
                            |row| row.get(0),
                        )
                        .ok()
                        .flatten()
                        .filter(|c: &String| !c.trim().is_empty());
                    if conv_id
                        .as_deref()
                        .is_some_and(|c| is_deleted_key(&deleted_ids, c))
                    {
                        continue;
                    }
                    if source == TitleSource::Custom {
                        let _ = bg_conn.execute(
                            "UPDATE sessions SET title = ?1, title_source = 3, custom_title = 1
                             WHERE id = ?2 OR id = ?3 OR conversation_id = ?3",
                            params![title, id, norm],
                        );
                        if let Some(cid) = conv_id {
                            pinned_titles.insert(cid.clone(), title.clone());
                            sync::update_cache_record(
                                &agent_cache,
                                &cid,
                                Some(title),
                                TitleSource::Custom,
                                0,
                            );
                        }
                    } else {
                        let _ = bg_conn.execute(
                            "UPDATE sessions SET title = ?1, title_source = ?2
                             WHERE (id = ?3 OR id = ?4 OR conversation_id = ?4)
                               AND custom_title = 0
                               AND title_source <= ?2",
                            params![title, source.as_i64(), id, norm],
                        );
                    }
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::RenameAgentTitle { conv_id, title } => {
                    let cid = conv_id.strip_prefix("ext-").unwrap_or(&conv_id).to_string();
                    if cid.is_empty()
                        || is_deleted_key(&deleted_ids, &conv_id)
                        || is_deleted_key(&deleted_ids, &cid)
                    {
                        continue;
                    }
                    pinned_titles.insert(cid.clone(), title.clone());
                    let updated = bg_conn
                        .execute(
                            "UPDATE sessions SET title = ?1, title_source = 3, custom_title = 1
                             WHERE id = ?2 OR conversation_id = ?2",
                            params![title, cid],
                        )
                        .unwrap_or(0);
                    if updated == 0 {
                        let backend = super::infer_backend(Some(&cid), None).as_str();
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or(Duration::ZERO)
                            .as_secs() as i64;
                        let _ = bg_conn.execute(
                            "INSERT OR IGNORE INTO sessions
                             (id, conversation_id, backend, title, title_source, custom_title, created_at)
                             VALUES (?1, ?2, ?3, ?4, 3, 1, ?5)",
                            params![format!("ext-{}", cid), cid, backend, title, now],
                        );
                    }
                    if !cid.starts_with("ses_") {
                        if let Some(ref adb) = agent_db_path {
                            let _ = agent::update_agent_conversation_title_at(adb, &cid, &title);
                        }
                    }
                    sync::update_cache_record(
                        &agent_cache,
                        &cid,
                        Some(title),
                        TitleSource::Custom,
                        0,
                    );
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::RefreshAgent(maybe_cid) => {
                    sync::handle_refresh_agent(
                        &bg_conn,
                        maybe_cid,
                        &pinned_titles,
                        &deleted_ids,
                        agent_db_path.as_deref(),
                        opencode_db_path.as_deref(),
                        &agent_cache,
                        &agent_updates_tx,
                        &mut probed_empty,
                    );
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::AppendMessages {
                    conversation_id,
                    messages,
                } => {
                    if !is_deleted_key(&deleted_ids, &conversation_id) {
                        let _ = messages::insert_messages(&bg_conn, &conversation_id, &messages);
                    }
                }
                HistoryCommand::RecordTelemetry {
                    conversation_id,
                    snapshot,
                } => {
                    if !is_deleted_key(&deleted_ids, &conversation_id) {
                        let _ = telemetry::upsert(&bg_conn, &conversation_id, &snapshot);
                    }
                }
                HistoryCommand::DeleteSession { keys } => {
                    handle_delete_session(
                        &bg_conn,
                        keys,
                        &mut deleted_ids,
                        &mut pinned_titles,
                        &agent_cache,
                        opencode_db_path.is_some(),
                        &mut pending_deletes,
                    );
                    publish_snapshot(&bg_conn, &deleted_ids, true);
                }
                HistoryCommand::LoadEntries { done_tx } => {
                    let entries =
                        PromptHistory::load_recent_entries(&bg_conn, 500).unwrap_or_default();
                    let _ = done_tx.send(entries);
                }
                HistoryCommand::LoadHydration { conv_id, done_tx } => {
                    let data = hydrate::load_hydration_data_with_conn(Some(&bg_conn), &conv_id);
                    let _ = done_tx.send(data);
                }
                HistoryCommand::Flush {
                    wait_external,
                    done_tx,
                } => {
                    pending_deletes.retain(|h| !h.is_finished());
                    if wait_external {
                        for handle in pending_deletes.drain(..) {
                            let _ = handle.join();
                        }
                    }
                    let _ = done_tx.send(());
                }
            }
        }
        for handle in pending_deletes.drain(..) {
            let _ = handle.join();
        }
    });
}

fn handle_delete_session(
    conn: &Connection,
    initial_keys: Vec<String>,
    deleted_ids: &mut HashSet<String>,
    pinned_titles: &mut std::collections::HashMap<String, String>,
    agent_cache: &Arc<RwLock<Vec<SessionRecord>>>,
    allow_opencode_cli: bool,
    pending_deletes: &mut Vec<thread::JoinHandle<()>>,
) {
    if conn.execute_batch("BEGIN IMMEDIATE;").is_err() {
        return;
    }
    let mut keys = HashSet::new();
    for k in initial_keys {
        let trimmed = k.trim();
        if !trimmed.is_empty() {
            keys.insert(trimmed.to_string());
            if let Some(raw) = trimmed.strip_prefix("ext-") {
                if !raw.is_empty() {
                    keys.insert(raw.to_string());
                }
            }
        }
    }
    let seed_keys: Vec<String> = keys.iter().cloned().collect();
    if let Ok(mut stmt) = conn
        .prepare("SELECT id, conversation_id FROM sessions WHERE id = ?1 OR conversation_id = ?1")
    {
        for seed in &seed_keys {
            if let Ok(rows) = stmt.query_map(params![seed], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            }) {
                for (sid, maybe_cid) in rows.flatten() {
                    keys.insert(sid);
                    if let Some(cid) = maybe_cid.filter(|c| !c.trim().is_empty()) {
                        keys.insert(cid);
                    }
                }
            }
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let mut db_ok = true;
    for key in &keys {
        if conn
            .execute(
                "INSERT OR REPLACE INTO deleted_sessions (conversation_id, deleted_at) VALUES (?1, ?2)",
                params![key, now],
            )
            .is_err()
            || conn
                .execute(
                    "DELETE FROM sessions WHERE id = ?1 OR conversation_id = ?1",
                    params![key],
                )
                .is_err()
            || conn
                .execute("DELETE FROM messages WHERE conversation_id = ?1", params![key])
                .is_err()
            || telemetry::delete(conn, key).is_err()
        {
            db_ok = false;
            break;
        }
    }
    if !db_ok || conn.execute_batch("COMMIT;").is_err() {
        let _ = conn.execute_batch("ROLLBACK;");
        return;
    }
    for key in &keys {
        deleted_ids.insert(key.clone());
        pinned_titles.remove(key);
    }
    if let Ok(mut guard) = agent_cache.write() {
        guard.retain(|r| !is_deleted_record(r, deleted_ids));
    }
    if allow_opencode_cli {
        pending_deletes.retain(|h| !h.is_finished());
        for ses_id in keys
            .into_iter()
            .filter(|k| crate::transport::is_valid_opencode_session_id(k))
        {
            pending_deletes.push(thread::spawn(move || {
                let _ = crate::transport::delete_opencode_session(&ses_id);
            }));
        }
    }
}
