use std::path::PathBuf;

use rusqlite::{params, Connection};

use super::SessionRecord;

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
        if s.to_lowercase().starts_with(pre) {
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
    let home = std::env::var("HOME").ok()?;
    let db_path = PathBuf::from(home).join(".gemini/antigravity-cli/conversation_summaries.db");
    let conn =
        Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
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
    let home = std::env::var("HOME").ok()?;
    let db_path = PathBuf::from(home).join(".gemini/antigravity-cli/conversation_summaries.db");
    let conn =
        Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
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
    let home = std::env::var("HOME").ok()?;
    let db_path = PathBuf::from(home).join(".gemini/antigravity-cli/conversation_summaries.db");
    let conn =
        Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
    let _ = conn.execute(
        "UPDATE conversation_summaries SET title = ?1 WHERE conversation_id = ?2",
        params![new_title, conv_id],
    );
    Some(())
}

pub fn load_agent_conversations() -> Vec<SessionRecord> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let db_path = PathBuf::from(home).join(".gemini/antigravity-cli/conversation_summaries.db");
    let conn =
        match Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
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
