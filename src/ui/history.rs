use std::fs::File;
use std::io::{BufRead, BufReader};

pub use crate::infra::history::*;
use crate::ui::cache::RenderCache;

pub struct HydratedSession {
    pub hydrated: bool,
    pub cache: RenderCache,
    pub title: Option<String>,
    pub is_custom_title: bool,
    pub title_source: TitleSource,
    pub effort: Option<String>,
    pub telemetry: Option<TelemetrySnapshot>,
}

pub fn resolve_session_telemetry(
    conv_id: &str,
    db_path: Option<&std::path::Path>,
) -> Option<TelemetrySnapshot> {
    let local = db_path.and_then(|path| crate::infra::history::telemetry::load(path, conv_id));
    let fallback = if conv_id.starts_with("ses_") {
        crate::infra::history::opencode::load_usage(conv_id).map(
            |(usage, model, duration_millis)| TelemetrySnapshot {
                last: usage,
                accumulated: usage,
                steps: 0,
                turns: 0,
                duration_millis,
                context_window: None,
                model,
            },
        )
    } else {
        crate::infra::history::transcript::load_usage(conv_id)
    };
    merge_session_telemetry(conv_id, local, fallback)
}

fn merge_session_telemetry(
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

pub fn hydrate_session_blocking(
    conv_id: &str,
    db_path: Option<&std::path::Path>,
) -> HydratedSession {
    let mut cache = RenderCache::new();
    let (hydrated, transcript_usage) =
        hydrate_conversation_with_usage(conv_id, &mut cache, db_path);
    let is_custom_title = db_path
        .map(|path| crate::infra::history::messages::is_custom_title_at(path, conv_id))
        .unwrap_or(false);
    let (title, title_source) = if is_custom_title {
        let t = db_path
            .and_then(|path| conversation_title_at(path, conv_id))
            .or_else(|| conversation_title(conv_id, db_path));
        (t, TitleSource::Custom)
    } else if conv_id.starts_with("ses_") {
        if let Some(t) = crate::infra::history::opencode::query_opencode_title(conv_id) {
            (Some(t), TitleSource::Semantic)
        } else {
            let t = db_path.and_then(|path| conversation_title_at(path, conv_id));
            let src = if t.is_some() {
                TitleSource::Provisional
            } else {
                TitleSource::Default
            };
            (t, src)
        }
    } else if let Some((t, src)) = default_agent_db_path()
        .and_then(|path| query_agent_conversation_title_with_source_at(&path, conv_id))
    {
        (Some(t), src)
    } else {
        let t = db_path.and_then(|path| conversation_title_at(path, conv_id));
        let src = if t.is_some() {
            TitleSource::Provisional
        } else {
            TitleSource::Default
        };
        (t, src)
    };
    let mut telemetry = if conv_id.starts_with("ses_") {
        resolve_session_telemetry(conv_id, db_path)
    } else {
        let local = db_path.and_then(|path| crate::infra::history::telemetry::load(path, conv_id));
        merge_session_telemetry(conv_id, local, transcript_usage)
    };
    let effort = if conv_id.starts_with("ses_") {
        let (oc_model, oc_effort) = crate::infra::history::opencode::query_opencode_model(conv_id);
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
    HydratedSession {
        hydrated,
        cache,
        title,
        is_custom_title,
        title_source,
        effort,
        telemetry,
    }
}

fn push_transcript_blocks(blocks: Vec<TranscriptBlock>, cache: &mut RenderCache) -> usize {
    let initial_count = cache.block_count();
    for block in blocks {
        match block {
            TranscriptBlock::UserMessage(msg) => {
                cache.push_user_message(&msg);
            }
            TranscriptBlock::Thought(thought) => {
                cache.push_historical_thought(&thought);
            }
            TranscriptBlock::ToolCall {
                step_index,
                name,
                args,
                output,
            } => {
                cache.push_historical_tool_call(
                    step_index,
                    &name,
                    args.as_deref(),
                    output.as_deref(),
                );
            }
            TranscriptBlock::AssistantMessage(msg) => {
                cache.push_historical_assistant_message(&msg);
            }
        }
    }
    cache.consolidate_active_turn(None);
    cache.block_count().saturating_sub(initial_count)
}

pub fn hydrate_cache_from_reader<R: BufRead>(reader: R, cache: &mut RenderCache) -> usize {
    let blocks = parse_transcript_reader(reader);
    push_transcript_blocks(blocks, cache)
}

fn hydrate_conversation_with_usage(
    conv_id: &str,
    cache: &mut RenderCache,
    db_path: Option<&std::path::Path>,
) -> (bool, Option<TelemetrySnapshot>) {
    let transcript_usage = if !conv_id.starts_with("ses_") {
        let (hydrated, usage) = hydrate_cache_from_transcript_with_usage(conv_id, cache);
        if hydrated {
            return (true, usage);
        }
        usage
    } else {
        None
    };
    let local_messages = db_path
        .map(|path| crate::infra::history::conversation_messages_at(path, conv_id))
        .unwrap_or_default();
    let messages = if conv_id.starts_with("ses_") {
        let opencode_messages = crate::infra::history::opencode::load_opencode_messages(conv_id);
        if opencode_messages.len() > local_messages.len() {
            opencode_messages
        } else {
            local_messages
        }
    } else {
        local_messages
    };
    if messages.is_empty() {
        return (false, transcript_usage);
    }
    hydrate_stored_messages(&messages, cache);
    (true, transcript_usage)
}

pub fn hydrate_conversation(
    conv_id: &str,
    cache: &mut RenderCache,
    db_path: Option<&std::path::Path>,
) -> bool {
    hydrate_conversation_with_usage(conv_id, cache, db_path).0
}

fn hydrate_stored_messages(messages: &[StoredMessage], cache: &mut RenderCache) {
    for message in messages {
        match message.role.as_str() {
            "user" => {
                cache.push_user_message(&message.content);
            }
            "assistant" => {
                let turn_meta = message
                    .turn_meta
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<crate::ui::cache::TurnMeta>(raw).ok());
                cache.push_historical_assistant_message_with_meta(&message.content, turn_meta);
            }
            "thought" => {
                cache.push_historical_thought(&message.content);
            }
            "tool" => {
                cache.push_historical_tool_call(
                    message.step_index,
                    message.tool_name.as_deref().unwrap_or("tool"),
                    message.tool_args.as_deref(),
                    Some(message.content.as_str()),
                );
            }
            _ => {}
        }
    }
}

pub fn conversation_title(conv_id: &str, db_path: Option<&std::path::Path>) -> Option<String> {
    let external = if conv_id.starts_with("ses_") {
        crate::infra::history::opencode::query_opencode_title(conv_id)
    } else {
        query_agent_conversation_title(conv_id)
    };
    external.or_else(|| db_path.and_then(|path| conversation_title_at(path, conv_id)))
}

fn hydrate_cache_from_transcript_with_usage(
    conv_id: &str,
    cache: &mut RenderCache,
) -> (bool, Option<TelemetrySnapshot>) {
    let path = match transcript_path_for_conversation(conv_id) {
        Some(p) => p,
        None => return (false, None),
    };
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return (false, None),
    };
    let reader = BufReader::new(file);
    let (blocks, usage) = crate::infra::history::transcript::parse_transcript_all(reader);
    let count = push_transcript_blocks(blocks, cache);
    (count > 0, usage)
}

pub fn hydrate_cache_from_transcript(conv_id: &str, cache: &mut RenderCache) -> bool {
    hydrate_cache_from_transcript_with_usage(conv_id, cache).0
}
