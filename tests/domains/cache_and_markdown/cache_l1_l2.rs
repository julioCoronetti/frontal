use frontal::protocol::{MediaItem, PromptPayload, StepState};
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BG_USER_CONTAINER, BORDER_SUBTLE, TEXT_MUTED,
    TEXT_PRIMARY,
};
use ratatui::style::{Color, Modifier};

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

    let payload = PromptPayload::new(
        "eae me diz a resposta dessa questao aqui:",
        vec![MediaItem {
            mime_type: "image/png".to_string(),
            uri: "/home/julio/.frontal/cache/media/img_500x500_caa4ffb2.png".to_string(),
        }],
    )
    .with_chat_text("eae me diz a resposta dessa questao aqui: [Image 1]");
    let display = payload.display_for_chat();
    assert_eq!(
        display,
        "eae me diz a resposta dessa questao aqui: [Image 1]\n\n[file] clipboard"
    );

    let mut media_cache = RenderCache::new();
    media_cache.push_user_message(&display);
    let total = media_cache.ensure_visual_lines(60);
    let lines = media_cache.visible_slice(0, total);

    let marker_line = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.as_ref() == "[Image 1]"))
        .expect("expected intact [Image 1] span");
    let marker_span = marker_line
        .spans
        .iter()
        .find(|s| s.content.as_ref() == "[Image 1]")
        .unwrap();
    assert_eq!(marker_span.style.fg, Some(ACCENT_HIGHLIGHT));
    assert_eq!(marker_span.style.bg, Some(BG_USER_CONTAINER));
    assert!(marker_span.style.add_modifier.contains(Modifier::BOLD));

    let badge_line = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.as_ref() == " file "))
        .expect("expected file badge span");
    let badge_span = badge_line
        .spans
        .iter()
        .find(|s| s.content.as_ref() == " file ")
        .unwrap();
    assert_eq!(badge_span.style.fg, Some(TEXT_PRIMARY));
    assert_eq!(badge_span.style.bg, Some(ACCENT_RUBY));
    assert!(badge_span.style.add_modifier.contains(Modifier::BOLD));

    let source_span = badge_line
        .spans
        .iter()
        .find(|s| s.content.as_ref() == " clipboard")
        .unwrap();
    assert_eq!(source_span.style.fg, Some(TEXT_MUTED));
    assert_eq!(source_span.style.bg, Some(BG_USER_CONTAINER));

    let narrow_total = media_cache.ensure_visual_lines(46);
    let narrow_lines = media_cache.visible_slice(0, narrow_total);
    assert!(narrow_lines
        .iter()
        .any(|l| l.spans.iter().any(|s| s.content.as_ref() == "[Image 1]")));

    let copied = media_cache.extract_text_range(1, 0, narrow_total.saturating_sub(2), 100);
    assert!(copied.contains("[Image 1]"));
    assert!(copied.contains("file  clipboard"));
    assert!(!copied.contains("▌"));

    let mut plain_cache = RenderCache::new();
    plain_cache.push_user_message("please inspect this\n\nfile readme.md");
    let plain_total = plain_cache.ensure_visual_lines(60);
    let plain_lines = plain_cache.visible_slice(0, plain_total);
    assert!(!plain_lines
        .iter()
        .any(|l| l.spans.iter().any(|s| s.content.as_ref() == " file ")));
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

#[test]
fn test_thought_and_tool_blocks_use_atlas_crimson_palette() {
    let mut cache = RenderCache::new();
    let tid = cache.append_streaming_delta(BlockKind::Thought, "reasoning line");
    cache.consolidate_active_turn(None);
    cache.toggle_block_collapsed(tid);
    cache.upsert_tool_call(Some(1), "run_cmd", StepState::Active, None, Some("running"));
    cache.upsert_tool_call(Some(2), "read_file", StepState::Done, None, None);
    cache.upsert_tool_call(Some(3), "write_file", StepState::Error, None, Some("err"));

    let total = cache.ensure_visual_lines(80);
    let lines = cache.visible_slice(0, total);
    assert_eq!(lines[0].spans[0].content.as_ref(), "  ");
    assert_eq!(lines[0].spans[1].style.fg, Some(TEXT_MUTED));
    assert_eq!(lines[2].spans[0].content.as_ref(), "  ");
    assert_eq!(lines[2].spans[1].style.fg, Some(BORDER_SUBTLE));

    let active = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("run_cmd")))
        .unwrap();
    assert_eq!(active.spans[0].content.as_ref(), "  ");
    assert_eq!(active.spans[1].style.fg, Some(ACCENT_HIGHLIGHT));

    let done = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("read_file")))
        .unwrap();
    assert_eq!(done.spans[0].content.as_ref(), "  ");
    assert_eq!(done.spans[1].style.fg, Some(ACCENT_PRIMARY));

    let err = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("write_file")))
        .unwrap();
    assert_eq!(err.spans[0].content.as_ref(), "  ");
    assert_eq!(err.spans[1].style.fg, Some(Color::Red));
}

#[test]
fn test_extract_text_range_handles_usize_max_and_interior_lines() {
    let mut cache = RenderCache::new();
    cache.push_user_message("pergunta");
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "linha 1\n\nlinha 2\n\nlinha 3\n\nlinha 4",
    );
    cache.consolidate_active_turn(None);
    let total = cache.ensure_visual_lines(80);

    let full = cache.extract_text_range(0, 0, usize::MAX, 1000);
    assert!(full.contains("pergunta"));
    assert!(full.contains("linha 1"));
    assert!(full.contains("linha 2"));
    assert!(full.contains("linha 3"));
    assert!(full.contains("linha 4"));

    let mid = cache.extract_text_range(1, 0, total.saturating_sub(2), 100);
    assert!(mid.contains("linha 1"));
    assert!(mid.contains("linha 2"));
}
