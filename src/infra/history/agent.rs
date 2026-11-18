use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection};

use super::SessionRecord;

const AGENT_READ_BUSY_TIMEOUT: Duration = Duration::from_millis(500);
const AGENT_WRITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

pub fn default_agent_db_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".gemini/antigravity-cli/conversation_summaries.db"))
}

pub fn clean_session_title(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or(raw).trim();
    let preambles = [
        "cara seguinte",
        "seguinte cara",
        "seguinte,",
        "seguinte",
        "por favor,",
        "por favor",
        "leia esse card:",
        "leia:",
    ];
    let mut s = first;
    for pre in &preambles {
        if s.get(..pre.len())
            .map(|p| p.eq_ignore_ascii_case(pre))
            .unwrap_or(false)
        {
            s = s[pre.len()..].trim_start_matches([' ', ':', ',', '-']);
            break;
        }
    }
    if s.is_empty() {
        return String::from("New Session");
    }
    let clamped: String = s.chars().take(40).collect();
    let mut chars = clamped.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::from("New Session"),
    }
}

pub fn clamp_display_title(t: &str, max_chars: usize) -> String {
    let single = t.lines().next().unwrap_or(t).trim();
    if single.chars().count() > max_chars {
        let mut tr: String = single.chars().take(max_chars.saturating_sub(3)).collect();
        tr.push_str("...");
        tr
    } else {
        single.to_string()
    }
}

pub fn query_agent_conversation_title(conv_id: &str) -> Option<String> {
    let db_path = default_agent_db_path()?;
    query_agent_conversation_title_at(&db_path, conv_id)
}

pub fn query_agent_conversation_title_at(db_path: &Path, conv_id: &str) -> Option<String> {
    let conn =
        Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(AGENT_READ_BUSY_TIMEOUT);
    let mut stmt = conn
        .prepare("SELECT title, preview FROM conversation_summaries WHERE conversation_id = ?1")
        .ok()?;
    let mut rows = stmt.query(params![conv_id]).ok()?;
    if let Some(row) = rows.next().ok()? {
        let title: String = row.get(0).unwrap_or_default();
        let preview: String = row.get(1).unwrap_or_default();
        let trimmed_title = title.trim();
        if !trimmed_title.is_empty() {
            return Some(trimmed_title.to_string());
        }
        let trimmed_preview = preview.trim();
        if !trimmed_preview.is_empty() {
            return Some(clean_session_title(trimmed_preview));
        }
    }
    None
}

pub fn query_agent_conversation_timestamp(conv_id: &str) -> Option<i64> {
    let db_path = default_agent_db_path()?;
    query_agent_conversation_timestamp_at(&db_path, conv_id)
}

pub fn query_agent_conversation_timestamp_at(db_path: &Path, conv_id: &str) -> Option<i64> {
    let conn =
        Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(AGENT_READ_BUSY_TIMEOUT);
    let mut stmt = conn
        .prepare("SELECT CAST(strftime('%s', last_modified_time) AS INTEGER) FROM conversation_summaries WHERE conversation_id = ?1")
        .ok()?;
    let mut rows = stmt.query(params![conv_id]).ok()?;
    if let Some(row) = rows.next().ok()? {
        let ts: i64 = row.get(0).unwrap_or(0);
        if ts > 0 {
            return Some(ts);
        }
    }
    None
}

pub fn update_agent_conversation_title(conv_id: &str, new_title: &str) -> Option<()> {
    let db_path = default_agent_db_path()?;
    update_agent_conversation_title_at(&db_path, conv_id, new_title)
}

pub fn update_agent_conversation_title_at(
    db_path: &Path,
    conv_id: &str,
    new_title: &str,
) -> Option<()> {
    let conn =
        Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).ok()?;
    let _ = conn.busy_timeout(AGENT_WRITE_BUSY_TIMEOUT);
    let _ = conn.execute(
        "UPDATE conversation_summaries SET title = ?1 WHERE conversation_id = ?2",
        params![new_title, conv_id],
    );
    Some(())
}

pub fn load_agent_conversations() -> Vec<SessionRecord> {
    match default_agent_db_path() {
        Some(db_path) => load_agent_conversations_at(&db_path),
        None => Vec::new(),
    }
}

pub fn load_agent_conversations_at(db_path: &Path) -> Vec<SessionRecord> {
    let conn =
        match Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
    let _ = conn.busy_timeout(AGENT_READ_BUSY_TIMEOUT);
    let mut stmt = match conn.prepare(
        "SELECT conversation_id, title, preview, CAST(strftime('%s', last_modified_time) AS INTEGER)
         FROM conversation_summaries WHERE conversation_id != '' ORDER BY last_modified_time DESC LIMIT 100",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut records = Vec::new();
    if let Ok(mut rows) = stmt.query([]) {
        while let Ok(Some(row)) = rows.next() {
            let conv_id: String = match row.get(0) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let title: String = row.get(1).unwrap_or_default();
            let preview: String = row.get(2).unwrap_or_default();
            let created_at: i64 = row.get(3).unwrap_or(0);
            let final_title = if !title.trim().is_empty() {
                Some(title.trim().to_string())
            } else if !preview.trim().is_empty() {
                Some(clean_session_title(&preview))
            } else {
                None
            };
            records.push(SessionRecord {
                id: format!("ext-{}", conv_id),
                conversation_id: Some(conv_id),
                model: None,
                mode: None,
                effort: None,
                title: final_title,
                created_at,
            });
        }
    }
    records
}
