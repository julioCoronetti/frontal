use std::io::BufRead;
use std::path::Path;

use crate::infra::history::{
    load_hydration_data, load_transcript_blocks_and_usage, parse_transcript_reader,
    HydrationClient, HydrationData, StoredMessage, TelemetrySnapshot, TitleSource, TranscriptBlock,
};
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

pub fn hydrate_session_from_data(data: HydrationData) -> HydratedSession {
    let mut cache = RenderCache::new();
    let hydrated = if !data.blocks.is_empty() {
        push_transcript_blocks(data.blocks, &mut cache) > 0
    } else if !data.stored_messages.is_empty() {
        hydrate_stored_messages(&data.stored_messages, &mut cache);
        true
    } else {
        false
    };
    HydratedSession {
        hydrated,
        cache,
        title: data.title,
        is_custom_title: data.is_custom_title,
        title_source: data.title_source,
        effort: data.effort,
        telemetry: data.telemetry,
    }
}

pub fn hydrate_session_with_client(conv_id: &str, client: &HydrationClient) -> HydratedSession {
    hydrate_session_from_data(client.load(conv_id))
}

pub fn hydrate_session_blocking(conv_id: &str, db_path: Option<&Path>) -> HydratedSession {
    hydrate_session_from_data(load_hydration_data(conv_id, db_path))
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

pub fn hydrate_conversation(
    conv_id: &str,
    cache: &mut RenderCache,
    db_path: Option<&Path>,
) -> bool {
    let data = load_hydration_data(conv_id, db_path);
    if !data.blocks.is_empty() {
        return push_transcript_blocks(data.blocks, cache) > 0;
    }
    if !data.stored_messages.is_empty() {
        hydrate_stored_messages(&data.stored_messages, cache);
        return true;
    }
    false
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

pub fn hydrate_cache_from_transcript(conv_id: &str, cache: &mut RenderCache) -> bool {
    let (blocks, _) = load_transcript_blocks_and_usage(conv_id);
    push_transcript_blocks(blocks, cache) > 0
}
