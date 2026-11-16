use frontal::protocol::StepState;
use frontal::ui::cache::{BlockKind, RenderCache};

#[test]
fn test_render_cache_l1_and_l2_o1_invariance() {
    let mut cache = RenderCache::new();

    cache.push_user_message("user prompt 1");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant response 1");
    cache.consolidate_active_turn(None);

    cache.push_user_message("user prompt 2");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant response 2");
    cache.consolidate_active_turn(None);

    let initial_lines = cache.ensure_visual_lines(80);
    assert!(initial_lines > 0);
    let misses_after_initial = cache.cache_misses();
    assert_eq!(misses_after_initial, 4);

    let lines_second = cache.ensure_visual_lines(80);
    assert_eq!(lines_second, initial_lines);
    assert_eq!(cache.cache_misses(), misses_after_initial);
    assert_eq!(cache.cache_hits(), 4);

    for i in 0..10 {
        cache.append_streaming_delta(BlockKind::Assistant, &format!(" token{}", i));
        cache.ensure_visual_lines(80);
    }

    assert_eq!(cache.cache_misses(), misses_after_initial + 10);
    assert_eq!(cache.cache_hits(), 4 + (4 * 10));
}

#[test]
fn test_render_cache_resize_invalidation_and_subsequent_hits() {
    let mut cache = RenderCache::new();
    cache.push_user_message("First long line that wraps differently across widths");
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "Second long line that wraps differently",
    );
    cache.consolidate_active_turn(None);

    let lines_80 = cache.ensure_visual_lines(80);
    let misses_80 = cache.cache_misses();
    assert_eq!(misses_80, 2);

    let lines_40 = cache.ensure_visual_lines(40);
    assert!(lines_40 >= lines_80);
    assert_eq!(cache.cache_misses(), 4);

    let hits_before = cache.cache_hits();
    let lines_40_again = cache.ensure_visual_lines(40);
    assert_eq!(lines_40_again, lines_40);
    assert_eq!(cache.cache_hits(), hits_before + 2);
    assert_eq!(cache.cache_misses(), 4);
}

#[test]
fn test_consolidate_active_turn_fallback_response() {
    let mut cache = RenderCache::new();
    cache.push_user_message("hello");
    cache.consolidate_active_turn(Some("fallback text"));
    assert_eq!(cache.blocks_count(), 2);
    assert!(cache.ensure_visual_lines(80) > 0);
    let text1: String = cache
        .visible_slice(0, 10)
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text1.contains("fallback text"));

    let mut cache2 = RenderCache::new();
    cache2.push_user_message("hello");
    cache2.append_streaming_delta(BlockKind::Assistant, "actual response");
    cache2.consolidate_active_turn(Some("fallback text"));
    assert_eq!(cache2.blocks_count(), 2);
    assert!(cache2.ensure_visual_lines(80) > 0);
    let text2: String = cache2
        .visible_slice(0, 10)
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text2.contains("actual response") && !text2.contains("fallback text"));
}

#[test]
fn test_thought_block_selective_l2_invalidation_and_collapse() {
    let mut cache = RenderCache::new();

    cache.push_user_message("Explique a arquitetura do sistema.");
    let thought_id = cache.append_streaming_delta(
        BlockKind::Thought,
        "Passo 1: Analisando requisitos...\nPasso 2: Verificando contratos...\nPasso 3: Sintetizando resposta...",
    );
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "A arquitetura e baseada em modulos desacoplados e loop assincrono a 60 FPS.",
    );
    cache.consolidate_active_turn(None);

    let initial_lines = cache.ensure_visual_lines(80);
    assert!(initial_lines >= 3);

    let hits_1 = cache.cache_hits();
    let misses_1 = cache.cache_misses();
    assert_eq!(misses_1, 3);
    assert_eq!(hits_1, 0);

    let expanded = cache.toggle_block_collapsed(thought_id);
    assert!(!expanded);

    let expanded_lines = cache.ensure_visual_lines(80);
    assert!(expanded_lines > initial_lines);

    let hits_2 = cache.cache_hits();
    let misses_2 = cache.cache_misses();

    assert_eq!(misses_2 - misses_1, 1);
    assert_eq!(hits_2 - hits_1, 2);

    let collapsed = cache.toggle_block_collapsed(thought_id);
    assert!(collapsed);

    let restored_lines = cache.ensure_visual_lines(80);
    assert_eq!(restored_lines, initial_lines);
}

#[test]
fn test_tool_cards_lifecycle_and_multiple_tools_same_turn() {
    let mut cache = RenderCache::new();

    let id1 = cache.upsert_tool_call(
        Some(10),
        "read_file",
        StepState::Active,
        Some(r#"path: "src/main.rs""#),
        None,
    );
    let lines1 = cache.ensure_visual_lines(80);
    assert!(lines1 >= 1);

    let id1_done = cache.upsert_tool_call(
        Some(10),
        "read_file",
        StepState::Done,
        Some(r#"path: "src/main.rs""#),
        Some("fn main() {}"),
    );
    assert_eq!(id1, id1_done);

    let id2 = cache.upsert_tool_call(
        Some(11),
        "read_file",
        StepState::Active,
        Some(r#"path: "non_existent.rs""#),
        None,
    );
    assert_ne!(id1, id2);

    let id2_err = cache.upsert_tool_call(
        Some(11),
        "read_file",
        StepState::Error,
        Some(r#"path: "non_existent.rs""#),
        Some("FileNotFound: non_existent.rs"),
    );
    assert_eq!(id2, id2_err);

    cache.ensure_visual_lines(80);
    let slice = cache.visible_slice(0, 100);
    let text: String = slice
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();

    assert!(text.contains("● read_file"));
    assert!(text.contains("FileNotFound"));

    cache.consolidate_active_turn(None);
    assert_eq!(cache.blocks_count(), 2);
}

#[test]
fn test_tool_card_collapsing_and_expansion() {
    let mut cache = RenderCache::new();
    let id = cache.upsert_tool_call(
        Some(1),
        "read_file",
        StepState::Done,
        Some("path: foo.rs"),
        Some("line 1\nline 2\nline 3\nline 4\nline 5"),
    );

    let initial_lines = cache.ensure_visual_lines(80);
    assert_eq!(initial_lines, 2);

    let toggled_id = cache.toggle_latest_tool();
    assert_eq!(toggled_id, Some(id));
    let expanded_lines = cache.ensure_visual_lines(80);
    assert!(expanded_lines >= 6);

    let toggled_back = cache.toggle_latest_tool();
    assert_eq!(toggled_back, Some(id));
    let collapsed_lines = cache.ensure_visual_lines(80);
    assert_eq!(collapsed_lines, 2);

    let slice = cache.visible_slice(0, 1);
    let text: String = slice
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text.contains("read_file"));
}

#[test]
fn test_user_message_accent_bar_styling() {
    let mut cache = RenderCache::new();
    cache.push_user_message("test message");
    cache.ensure_visual_lines(80);

    let slice = cache.visible_slice(0, 10);
    assert!(slice.len() >= 3);
    assert_eq!(slice[0].spans[0].content.as_ref(), "▌ ");
    let text_line = &slice[1];
    let spans = &text_line.spans;
    assert_eq!(spans[0].content.as_ref(), "▌ ");
    assert_eq!(spans[1].content.as_ref(), "test message");
    assert_eq!(slice[2].spans[0].content.as_ref(), "▌ ");
}

#[test]
fn test_render_cache_60fps_latency_and_zero_reallocation() {
    let mut cache = RenderCache::new();
    for i in 0..500 {
        if i % 4 == 0 {
            cache.push_user_message(&format!("user message {}", i));
        } else if i % 4 == 1 {
            cache.append_streaming_delta(
                BlockKind::Assistant,
                &format!("assistant message {}\nline 2", i),
            );
            cache.consolidate_active_turn(None);
        } else if i % 4 == 2 {
            cache.append_streaming_delta(BlockKind::Thought, &format!("thought block {}", i));
            cache.consolidate_active_turn(None);
        } else {
            cache.upsert_tool_call(
                Some(i as u64),
                "read_file",
                StepState::Done,
                Some("{\"path\":\"test.rs\"}"),
                Some("ok"),
            );
        }
    }

    cache.ensure_visual_lines(100);
    let misses_before = cache.cache_misses();

    let start = std::time::Instant::now();
    for _ in 0..60 {
        cache.ensure_visual_lines(100);
        let slice = cache.visible_slice(100, 30);
        assert_eq!(slice.len(), 30);
    }
    let elapsed = start.elapsed();

    let misses_after = cache.cache_misses();
    assert_eq!(misses_before, misses_after);

    let per_frame_latency = elapsed / 60;
    assert!(
        per_frame_latency < std::time::Duration::from_millis(2),
        "Per frame latency {:?} exceeds 2ms threshold",
        per_frame_latency
    );
}
