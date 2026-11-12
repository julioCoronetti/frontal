use std::fs::File;
use std::io::{BufRead, BufReader};

pub use crate::infra::history::*;
use crate::ui::cache::RenderCache;

pub fn hydrate_cache_from_reader<R: BufRead>(reader: R, cache: &mut RenderCache) -> usize {
    let initial_count = cache.block_count();
    let blocks = parse_transcript_reader(reader);
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

pub fn hydrate_conversation(
    conv_id: &str,
    cache: &mut RenderCache,
    db_path: Option<&std::path::Path>,
) -> bool {
    if hydrate_cache_from_transcript(conv_id, cache) {
        return true;
    }
    let Some(path) = db_path else {
        return false;
    };
    let messages = crate::infra::history::conversation_messages_at(path, conv_id);
    if messages.is_empty() {
        return false;
    }
    for message in messages {
        match message.role.as_str() {
            "user" => {
                cache.push_user_message(&message.content);
            }
            "assistant" => {
                cache.push_historical_assistant_message(&message.content);
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
    true
}

pub fn conversation_title(conv_id: &str, db_path: Option<&std::path::Path>) -> Option<String> {
    query_agent_conversation_title(conv_id)
        .or_else(|| db_path.and_then(|path| conversation_title_at(path, conv_id)))
}

pub fn hydrate_cache_from_transcript(conv_id: &str, cache: &mut RenderCache) -> bool {
    let path = match transcript_path_for_conversation(conv_id) {
        Some(p) => p,
        None => return false,
    };
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let reader = BufReader::new(file);
    let count = hydrate_cache_from_reader(reader, cache);
    count > 0
}
