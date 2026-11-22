use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::BackendKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TitleSource {
    #[default]
    Default = 0,
    Provisional = 1,
    Semantic = 2,
    Custom = 3,
}

impl TitleSource {
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Provisional,
            2 => Self::Semantic,
            3.. => Self::Custom,
            _ => Self::Default,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub conversation_id: Option<String>,
    pub backend: Option<BackendKind>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub title: Option<String>,
    pub title_source: TitleSource,
    pub created_at: i64,
}

pub(super) fn is_deleted_key(deleted_ids: &HashSet<String>, key: &str) -> bool {
    let raw = key.strip_prefix("ext-").unwrap_or(key);
    deleted_ids.contains(key) || deleted_ids.contains(raw)
}

pub(super) fn is_deleted_record(record: &SessionRecord, deleted_ids: &HashSet<String>) -> bool {
    !deleted_ids.is_empty()
        && (is_deleted_key(deleted_ids, &record.id)
            || record
                .conversation_id
                .as_deref()
                .is_some_and(|cid| is_deleted_key(deleted_ids, cid)))
}

pub(super) fn matches_id(s: &SessionRecord, key: &str, norm: &str) -> bool {
    s.id == key
        || s.id == norm
        || s.id.strip_prefix("ext-") == Some(norm)
        || s.conversation_id
            .as_deref()
            .is_some_and(|c| c == key || c.strip_prefix("ext-").unwrap_or(c) == norm)
}

pub(super) fn collect_matching_keys(records: &[SessionRecord], keys: &mut HashSet<String>) {
    for s in records {
        if keys.contains(&s.id)
            || s.conversation_id
                .as_deref()
                .is_some_and(|c| keys.contains(c))
        {
            keys.insert(s.id.clone());
            if let Some(cid) = s
                .conversation_id
                .as_deref()
                .filter(|c| !c.trim().is_empty())
            {
                keys.insert(cid.to_string());
            }
        }
    }
}

pub fn format_relative_time(created_at: i64) -> String {
    if created_at <= 0 {
        return "just now".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let diff = now.saturating_sub(created_at);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

pub fn infer_backend(conversation_id: Option<&str>, explicit: Option<BackendKind>) -> BackendKind {
    if let Some(backend) = explicit {
        return backend;
    }
    match conversation_id {
        Some(id) if id.starts_with("ses_") => BackendKind::OpenCode,
        _ => BackendKind::Agy,
    }
}

pub fn normalize_title_source(title: &Option<String>, source: TitleSource) -> TitleSource {
    match title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => TitleSource::Default,
        Some("New session") => TitleSource::Default,
        Some(_) if source == TitleSource::Default => TitleSource::Semantic,
        Some(_) => source,
    }
}

impl SessionRecord {
    pub fn relative_time(&self) -> String {
        format_relative_time(self.created_at)
    }

    pub fn effective_backend(&self) -> BackendKind {
        infer_backend(self.conversation_id.as_deref(), self.backend)
    }
}
