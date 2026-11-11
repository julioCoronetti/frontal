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
