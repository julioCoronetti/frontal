use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

use rusqlite::{params, Connection};

use super::sessions::is_deleted_key;
use super::{agent, infer_backend, opencode, transcript, SessionRecord, TitleSource};

pub(super) fn apply_pinned_titles(
    conn: &Connection,
    records: &mut Vec<SessionRecord>,
    pinned_titles: &HashMap<String, String>,
    deleted_ids: &HashSet<String>,
) {
    let mut needs_sort = false;
    for (cid, pinned_title) in pinned_titles {
        if is_deleted_key(deleted_ids, cid) {
            continue;
        }
        if let Some(existing) = records
            .iter_mut()
            .find(|s| s.conversation_id.as_deref() == Some(cid.as_str()))
        {
            existing.title = Some(pinned_title.clone());
            existing.title_source = TitleSource::Custom;
        } else {
            let known_ts: i64 = conn
                .query_row(
                    "SELECT created_at FROM sessions
                     WHERE conversation_id = ?1 OR id = ?1
                     ORDER BY created_at DESC LIMIT 1",
                    params![cid],
                    |row| row.get(0),
                )
                .unwrap_or(0)
                .max(0);
            if known_ts > 0 {
                needs_sort = true;
            }
            records.push(SessionRecord {
                id: format!("ext-{}", cid),
                conversation_id: Some(cid.clone()),
                backend: Some(infer_backend(Some(cid), None)),
                model: None,
                mode: None,
                effort: None,
                title: Some(pinned_title.clone()),
                title_source: TitleSource::Custom,
                created_at: known_ts,
            });
        }
    }
    if needs_sort {
        records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    }
}

pub(super) fn upsert_external_session(
    conn: &Connection,
    rec: &SessionRecord,
) -> rusqlite::Result<()> {
    let Some(ref cid) = rec.conversation_id else {
        return Ok(());
    };
    if cid.trim().is_empty() {
        return Ok(());
    }
    let backend = rec.effective_backend().as_str();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM sessions WHERE conversation_id = ?1 LIMIT 1",
            params![cid],
            |row| row.get(0),
        )
        .ok();
    if let Some(row_id) = existing_id {
        conn.execute(
            "UPDATE sessions SET
                 backend = COALESCE(backend, ?2),
                 model = COALESCE(?3, model),
                 effort = COALESCE(?4, effort),
                 title = CASE
                     WHEN custom_title != 0 THEN title
                     WHEN ?5 IS NOT NULL AND ?6 >= title_source THEN ?5
                     ELSE COALESCE(title, ?5)
                 END,
                 title_source = CASE
                     WHEN custom_title != 0 THEN title_source
                     ELSE MAX(title_source, ?6)
                 END,
                 created_at = CASE
                     WHEN created_at <= 0 AND ?7 > 0 THEN ?7
                     ELSE created_at
                 END
             WHERE id = ?1",
            params![
                row_id,
                backend,
                rec.model,
                rec.effort,
                rec.title,
                rec.title_source.as_i64(),
                rec.created_at,
            ],
        )?;
    } else {
        let custom_flag = i64::from(rec.title_source == TitleSource::Custom);
        conn.execute(
            "INSERT OR IGNORE INTO sessions
             (id, conversation_id, backend, model, mode, effort, title, title_source, custom_title, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                rec.id,
                cid,
                backend,
                rec.model,
                rec.mode,
                rec.effort,
                rec.title,
                rec.title_source.as_i64(),
                custom_flag,
                rec.created_at,
            ],
        )?;
    }
    Ok(())
}

pub(super) fn collect_known_models(
    conn: &Connection,
    agent_cache: &Arc<RwLock<Vec<SessionRecord>>>,
) -> HashMap<String, (Option<String>, Option<String>)> {
    let mut known = HashMap::new();
    if let Ok(guard) = agent_cache.read() {
        for rec in guard.iter() {
            if rec.model.is_some() {
                if let Some(ref cid) = rec.conversation_id {
                    known.insert(cid.clone(), (rec.model.clone(), rec.effort.clone()));
                }
            }
        }
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, model, effort FROM sessions
         WHERE conversation_id IS NOT NULL AND model IS NOT NULL AND model != ''",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        }) {
            for (cid, model, effort) in rows.flatten() {
                known.entry(cid).or_insert((model, effort));
            }
        }
    }
    known
}

pub(super) fn load_external_sql_conversations(
    agent_db: Option<&Path>,
    opencode_db: Option<&Path>,
    known_models: &HashMap<String, (Option<String>, Option<String>)>,
) -> Vec<SessionRecord> {
    let mut records = agent_db
        .map(agent::load_agent_conversations_at)
        .unwrap_or_default();
    for rec in &mut records {
        if rec.model.is_none() {
            if let Some(ref cid) = rec.conversation_id {
                if let Some((model, effort)) = known_models.get(cid) {
                    rec.model = model.clone();
                    rec.effort = effort.clone();
                }
            }
        }
    }
    if let Some(odb) = opencode_db {
        for rec in opencode::load_opencode_conversations_at(odb) {
            if let Some(ref cid) = rec.conversation_id {
                if !records
                    .iter()
                    .any(|existing| existing.conversation_id.as_deref() == Some(cid))
                {
                    records.push(rec);
                }
            }
        }
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    records
}

pub(super) fn load_external_conversations(
    agent_db: Option<&Path>,
    opencode_db: Option<&Path>,
    known_models: &HashMap<String, (Option<String>, Option<String>)>,
    probed_empty: &mut HashSet<String>,
) -> Vec<SessionRecord> {
    let mut records = load_external_sql_conversations(agent_db, opencode_db, known_models);
    let mut enriched = 0usize;
    for rec in &mut records {
        if rec.model.is_none() && rec.effective_backend() == crate::config::BackendKind::Agy {
            if let Some(ref cid) = rec.conversation_id {
                if probed_empty.contains(cid) || enriched >= 15 {
                    continue;
                }
                enriched += 1;
                if let Some(snapshot) = transcript::load_usage(cid) {
                    if let Some(raw_model) = snapshot.model.as_deref() {
                        let (base, effort) =
                            crate::transport::EngineConfig::split_model_effort(raw_model);
                        if !base.is_empty() {
                            rec.model = Some(base.to_string());
                        }
                        if let Some(eff) = effort {
                            rec.effort = Some(eff.to_string());
                        }
                    }
                }
                if rec.model.is_none() {
                    probed_empty.insert(cid.clone());
                }
            }
        }
    }
    records
}
