use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};

use rusqlite::{params, Connection};

use super::external::{
    apply_pinned_titles, collect_known_models, load_external_conversations, upsert_external_session,
};
use super::sessions::normalize_title_source;
use super::{agent, infer_backend, opencode, SessionRecord, TitleSource};

pub(super) fn load_pinned_titles(conn: &Connection) -> HashMap<String, String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, title FROM sessions
         WHERE custom_title != 0 AND conversation_id IS NOT NULL AND title IS NOT NULL",
    ) else {
        return HashMap::new();
    };
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

pub(super) fn handle_record_session(
    conn: &Connection,
    mut record: SessionRecord,
    pinned_titles: &mut HashMap<String, String>,
    agent_db_path: Option<&Path>,
    opencode_db_path: Option<&Path>,
) {
    if record.created_at <= 0 {
        if let Some(ref cid) = record.conversation_id {
            if !cid.trim().is_empty() {
                let external_ts = if cid.starts_with("ses_") {
                    opencode_db_path.and_then(|odb| opencode::query_opencode_timestamp_at(odb, cid))
                } else {
                    agent_db_path
                        .and_then(|adb| agent::query_agent_conversation_timestamp_at(adb, cid))
                };
                if let Some(ts) = external_ts {
                    record.created_at = ts;
                }
            }
        }
    }
    if conn.execute_batch("BEGIN IMMEDIATE;").is_err() {
        return;
    }
    let mut is_custom = record.title_source == TitleSource::Custom;
    if let Some(ref cid) = record.conversation_id {
        if !cid.trim().is_empty() {
            if let Some(pinned) = pinned_titles
                .get(cid)
                .cloned()
                .or_else(|| pinned_titles.remove(&record.id))
            {
                pinned_titles.insert(cid.clone(), pinned.clone());
                is_custom = true;
                record.title = Some(pinned);
            } else if let Ok(mut stmt) = conn.prepare(
                "SELECT title FROM sessions
                 WHERE (conversation_id = ?1 OR id = ?2) AND custom_title != 0
                 LIMIT 1",
            ) {
                if let Ok(existing_title) = stmt.query_row(params![cid, record.id], |row| {
                    row.get::<_, Option<String>>(0)
                }) {
                    is_custom = true;
                    if let Some(t) = existing_title.filter(|t| !t.trim().is_empty()) {
                        pinned_titles.insert(cid.clone(), t.clone());
                        record.title = Some(t);
                    }
                }
            }
            if conn
                .execute(
                    "DELETE FROM sessions WHERE conversation_id = ?1 AND id != ?2",
                    params![cid, record.id],
                )
                .is_err()
            {
                let _ = conn.execute_batch("ROLLBACK;");
                return;
            }
        }
    }
    let title_source = if is_custom {
        TitleSource::Custom
    } else {
        normalize_title_source(&record.title, record.title_source)
    };
    let backend = record
        .backend
        .or_else(|| {
            record
                .conversation_id
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .map(|c| infer_backend(Some(c), None))
        })
        .map(|b| b.as_str().to_string());
    let custom_flag = i64::from(is_custom);
    let upsert_res = conn.execute(
        "INSERT INTO sessions (id, conversation_id, backend, model, mode, effort, title, title_source, custom_title, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
             conversation_id = COALESCE(excluded.conversation_id, sessions.conversation_id),
             backend = COALESCE(excluded.backend, sessions.backend),
             model = COALESCE(excluded.model, sessions.model),
             mode = COALESCE(excluded.mode, sessions.mode),
             effort = COALESCE(excluded.effort, sessions.effort),
             title = CASE
                 WHEN excluded.custom_title != 0 AND excluded.title IS NOT NULL THEN excluded.title
                 WHEN sessions.custom_title != 0 THEN sessions.title
                 WHEN excluded.title IS NOT NULL AND excluded.title_source >= sessions.title_source THEN excluded.title
                 ELSE COALESCE(sessions.title, excluded.title)
             END,
             title_source = MAX(sessions.title_source, excluded.title_source),
             custom_title = MAX(sessions.custom_title, excluded.custom_title),
             created_at = CASE
                 WHEN excluded.created_at > 0 THEN excluded.created_at
                 ELSE sessions.created_at
             END",
        params![
            record.id,
            record.conversation_id,
            backend,
            record.model,
            record.mode,
            record.effort,
            record.title,
            title_source.as_i64(),
            custom_flag,
            record.created_at,
        ],
    );
    if upsert_res.is_err() || conn.execute_batch("COMMIT;").is_err() {
        let _ = conn.execute_batch("ROLLBACK;");
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_refresh_agent(
    conn: &Connection,
    maybe_cid: Option<String>,
    pinned_titles: &HashMap<String, String>,
    deleted_ids: &HashSet<String>,
    agent_db_path: Option<&Path>,
    opencode_db_path: Option<&Path>,
    agent_cache: &Arc<RwLock<Vec<SessionRecord>>>,
    agent_updates_tx: &Sender<(String, String)>,
    probed_empty: &mut HashSet<String>,
) {
    match maybe_cid {
        Some(raw_cid) => {
            let cid = raw_cid.strip_prefix("ext-").unwrap_or(&raw_cid).to_string();
            if cid.is_empty()
                || super::sessions::is_deleted_key(deleted_ids, &raw_cid)
                || super::sessions::is_deleted_key(deleted_ids, &cid)
            {
                return;
            }
            let pinned = pinned_titles.get(&cid).cloned();
            let (title, title_source, ts) = if cid.starts_with("ses_") {
                let Some(odb) = opencode_db_path else {
                    return;
                };
                let t = if pinned.is_some() {
                    None
                } else {
                    opencode::query_opencode_title_at(odb, &cid)
                };
                let src = if t.is_some() {
                    TitleSource::Semantic
                } else {
                    TitleSource::Default
                };
                let ts = opencode::query_opencode_timestamp_at(odb, &cid).unwrap_or(0);
                (t, src, ts)
            } else {
                let Some(adb) = agent_db_path else {
                    return;
                };
                let (t, src) = if pinned.is_some() {
                    (None, TitleSource::Custom)
                } else {
                    match agent::query_agent_conversation_title_with_source_at(adb, &cid) {
                        Some((title, source)) => (Some(title), source),
                        None => (None, TitleSource::Default),
                    }
                };
                let ts = agent::query_agent_conversation_timestamp_at(adb, &cid).unwrap_or(0);
                (t, src, ts)
            };
            let cache_title = pinned.clone().or_else(|| title.clone());
            let cache_src = if pinned.is_some() {
                TitleSource::Custom
            } else {
                title_source
            };
            update_cache_record(agent_cache, &cid, cache_title, cache_src, ts);
            if title_source == TitleSource::Semantic {
                if let Some(resolved_title) = title {
                    let _ = conn.execute(
                        "UPDATE sessions SET title = ?1, title_source = 2
                         WHERE (id = ?2 OR conversation_id = ?2)
                           AND custom_title = 0 AND title_source <= 2",
                        params![resolved_title, cid],
                    );
                    let _ = agent_updates_tx.send((cid, resolved_title));
                }
            }
        }
        None => {
            let known_models = collect_known_models(conn, agent_cache);
            let mut records = load_external_conversations(
                agent_db_path,
                opencode_db_path,
                &known_models,
                probed_empty,
            );
            apply_pinned_titles(conn, &mut records, pinned_titles, deleted_ids);
            records.retain(|r| !super::sessions::is_deleted_record(r, deleted_ids));
            if conn.execute_batch("BEGIN IMMEDIATE;").is_ok() {
                let mut db_ok = true;
                for rec in &records {
                    if upsert_external_session(conn, rec).is_err() {
                        db_ok = false;
                        break;
                    }
                }
                if !db_ok || conn.execute_batch("COMMIT;").is_err() {
                    let _ = conn.execute_batch("ROLLBACK;");
                }
            }
            if let Ok(mut guard) = agent_cache.write() {
                *guard = records;
            }
        }
    }
}

pub(super) fn update_cache_record(
    agent_cache: &Arc<RwLock<Vec<SessionRecord>>>,
    conv_id: &str,
    title: Option<String>,
    title_source: TitleSource,
    ts: i64,
) {
    if let Ok(mut guard) = agent_cache.write() {
        if let Some(existing) = guard
            .iter_mut()
            .find(|s| s.conversation_id.as_deref() == Some(conv_id))
        {
            if title_source >= existing.title_source {
                if let Some(t) = title {
                    existing.title = Some(t);
                    existing.title_source = title_source;
                }
            }
            if ts > 0 {
                existing.created_at = ts;
            }
        } else {
            guard.push(SessionRecord {
                id: format!("ext-{}", conv_id),
                conversation_id: Some(conv_id.to_string()),
                backend: Some(infer_backend(Some(conv_id), None)),
                model: None,
                mode: None,
                effort: None,
                title,
                title_source,
                created_at: ts,
            });
        }
    }
}
