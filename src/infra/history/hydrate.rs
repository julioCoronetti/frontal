use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::{
    agent, conversation_title_at, messages, opencode, query_agent_conversation_title,
    query_agent_conversation_title_with_source_at, telemetry, transcript, StoredMessage,
    TelemetrySnapshot, TitleSource, TranscriptBlock,
};

pub struct HydrationData {
    pub blocks: Vec<TranscriptBlock>,
    pub stored_messages: Vec<StoredMessage>,
    pub title: Option<String>,
    pub is_custom_title: bool,
    pub title_source: TitleSource,
    pub effort: Option<String>,
    pub telemetry: Option<TelemetrySnapshot>,
}

pub fn resolve_session_telemetry(
    conv_id: &str,
    db_path: Option<&Path>,
) -> Option<TelemetrySnapshot> {
    let local = db_path.and_then(|path| telemetry::load(path, conv_id));
    let fallback = if conv_id.starts_with("ses_") {
        opencode::load_usage(conv_id).map(|(usage, model, duration_millis)| TelemetrySnapshot {
            last: usage,
            accumulated: usage,
            steps: 0,
            turns: 0,
            duration_millis,
            context_window: None,
            model,
        })
    } else {
        transcript::load_usage(conv_id)
    };
    merge_session_telemetry(conv_id, local, fallback)
}

pub fn merge_session_telemetry(
    conv_id: &str,
    local: Option<TelemetrySnapshot>,
    fallback: Option<TelemetrySnapshot>,
) -> Option<TelemetrySnapshot> {
    match (local, fallback) {
        (Some(mut l), Some(f)) => {
            if conv_id.starts_with("ses_")
                && f.last.effective_total() > 0
                && (f.last.effective_total() != l.last.effective_total()
                    || f.accumulated.effective_total() > l.accumulated.effective_total())
            {
                let accumulated =
                    if f.accumulated.effective_total() > l.accumulated.effective_total() {
                        f.accumulated
                    } else {
                        l.accumulated
                    };
                return Some(TelemetrySnapshot {
                    last: f.last,
                    accumulated,
                    steps: l.steps,
                    turns: l.turns,
                    duration_millis: f.duration_millis.or(l.duration_millis),
                    context_window: l.context_window,
                    model: f.model.or(l.model),
                });
            }
            if !conv_id.starts_with("ses_") && f.steps > l.steps {
                return Some(TelemetrySnapshot {
                    last: f.last,
                    accumulated: f.accumulated,
                    steps: f.steps,
                    turns: f.turns,
                    duration_millis: l.duration_millis,
                    context_window: l.context_window,
                    model: f.model.or(l.model),
                });
            }
            if l.model
                .as_deref()
                .map(|m| m.trim().is_empty())
                .unwrap_or(true)
            {
                l.model = f.model;
            }
            Some(l)
        }
        (Some(l), None) => Some(l),
        (None, Some(f)) => Some(f),
        (None, None) => None,
    }
}

pub fn conversation_title(conv_id: &str, db_path: Option<&Path>) -> Option<String> {
    let external = if conv_id.starts_with("ses_") {
        opencode::query_opencode_title(conv_id)
    } else {
        query_agent_conversation_title(conv_id)
    };
    external.or_else(|| db_path.and_then(|path| conversation_title_at(path, conv_id)))
}

pub fn load_transcript_blocks_and_usage(
    conv_id: &str,
) -> (Vec<TranscriptBlock>, Option<TelemetrySnapshot>) {
    let Some(path) = super::transcript_path_for_conversation(conv_id) else {
        return (Vec::new(), None);
    };
    let Ok(file) = File::open(&path) else {
        return (Vec::new(), None);
    };
    transcript::parse_transcript_all(BufReader::new(file))
}

pub fn load_hydration_data(conv_id: &str, db_path: Option<&Path>) -> HydrationData {
    let conn = db_path
        .filter(|path| path.is_file())
        .and_then(|path| Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok())
        .inspect(|c| {
            let _ = c.busy_timeout(Duration::from_millis(300));
        });
    load_hydration_data_with_conn(conn.as_ref(), conv_id)
}

pub(super) fn load_hydration_data_with_conn(
    conn: Option<&Connection>,
    conv_id: &str,
) -> HydrationData {
    let is_ses = conv_id.starts_with("ses_");
    let (blocks, transcript_usage) = if !is_ses {
        load_transcript_blocks_and_usage(conv_id)
    } else {
        (Vec::new(), None)
    };
    let oc = if is_ses {
        Some(opencode::load_opencode_hydration(
            conv_id,
            blocks.is_empty(),
        ))
    } else {
        None
    };

    let stored_messages = if blocks.is_empty() {
        let local_messages = conn
            .and_then(|c| messages::conversation_messages_from_conn(c, conv_id).ok())
            .unwrap_or_default();
        if let Some(ref oc_data) = oc {
            if oc_data.messages.len() > local_messages.len() {
                oc_data.messages.clone()
            } else {
                local_messages
            }
        } else {
            local_messages
        }
    } else {
        Vec::new()
    };

    let is_custom_title = conn
        .and_then(|c| messages::is_custom_title_from_conn(c, conv_id).ok())
        .unwrap_or(false);
    let local_title = conn.and_then(|c| {
        messages::conversation_title_from_conn(c, conv_id)
            .ok()
            .flatten()
    });
    let (title, title_source) = if is_custom_title {
        let t = local_title.or_else(|| {
            if let Some(ref oc_data) = oc {
                oc_data.title.clone()
            } else {
                query_agent_conversation_title(conv_id)
            }
        });
        (t, TitleSource::Custom)
    } else if let Some(ref oc_data) = oc {
        if let Some(t) = oc_data.title.clone() {
            (Some(t), TitleSource::Semantic)
        } else {
            let src = if local_title.is_some() {
                TitleSource::Provisional
            } else {
                TitleSource::Default
            };
            (local_title, src)
        }
    } else if let Some((t, src)) = agent::default_agent_db_path()
        .and_then(|path| query_agent_conversation_title_with_source_at(&path, conv_id))
    {
        (Some(t), src)
    } else {
        let src = if local_title.is_some() {
            TitleSource::Provisional
        } else {
            TitleSource::Default
        };
        (local_title, src)
    };

    let local_telemetry = conn.and_then(|c| telemetry::load_from_conn(c, conv_id));
    let mut telemetry = if let Some(ref oc_data) = oc {
        let fallback = oc_data
            .usage
            .clone()
            .map(|(usage, model, duration_millis)| TelemetrySnapshot {
                last: usage,
                accumulated: usage,
                steps: 0,
                turns: 0,
                duration_millis,
                context_window: None,
                model,
            });
        merge_session_telemetry(conv_id, local_telemetry, fallback)
    } else {
        merge_session_telemetry(conv_id, local_telemetry, transcript_usage)
    };

    let effort = if let Some(oc_data) = oc {
        let (oc_model, oc_effort) = oc_data.model;
        if let Some(ref mut snap) = telemetry {
            if snap
                .model
                .as_deref()
                .map(|m| m.trim().is_empty())
                .unwrap_or(true)
            {
                snap.model = oc_model;
            }
        } else if let Some(m) = oc_model {
            telemetry = Some(TelemetrySnapshot {
                model: Some(m),
                ..Default::default()
            });
        }
        oc_effort
    } else {
        None
    };

    HydrationData {
        blocks,
        stored_messages,
        title,
        is_custom_title,
        title_source,
        effort,
        telemetry,
    }
}
