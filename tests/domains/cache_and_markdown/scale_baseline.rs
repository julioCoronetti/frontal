use frontal::ui::cache::{BlockKind, RenderCache};

fn populate_large_session(cache: &mut RenderCache, turns: usize) {
    for i in 0..turns {
        cache.push_user_message(&format!(
            "User prompt {} with enough words to wrap across narrow terminal widths cleanly",
            i
        ));
        if i % 3 == 0 {
            cache.push_historical_thought(&format!(
                "Thought step {} analyzing module boundaries and cache invariants",
                i
            ));
        }
        if i % 4 == 0 {
            cache.push_historical_tool_call(
                Some(i as u64),
                &format!("tool_{}", i),
                Some("{\"arg\":1}"),
                Some("ok\nline2"),
            );
        }
        cache.push_historical_assistant_message(&format!(
            "Assistant response {} explaining deterministic rendering and L1/L2 caching.\n\n```rust\nlet x_{} = {};\n```",
            i, i, i
        ));
    }
}

#[test]
fn test_scale_baseline_thousands_of_blocks_and_steady_viewport() {
    let mut cache = RenderCache::new();
    populate_large_session(&mut cache, 900);
    assert!(cache.blocks_count() >= 2_000);

    let total = cache.ensure_visual_lines(80);
    assert!(total >= cache.blocks_count());

    let misses_after_warm = cache.cache_misses();
    let total_repeat = cache.ensure_visual_lines(80);
    assert_eq!(total_repeat, total);
    assert_eq!(cache.cache_misses() - misses_after_warm, 0);

    let mid = total / 2;
    let slice = cache.visible_slice(mid, 40);
    assert_eq!(slice.len(), 40);

    let extracted = cache.extract_text_range(mid, 0, mid + 3, 20);
    assert!(!extracted.trim().is_empty());

    let before_toggle = cache.cache_misses();
    assert!(cache.toggle_latest_thought().is_some());
    let total_toggled = cache.ensure_visual_lines(80);
    assert_ne!(total_toggled, total);
    assert!(cache.cache_misses() - before_toggle <= 1);
}

#[test]
fn test_scale_baseline_hundreds_of_streaming_deltas_isolated() {
    let mut cache = RenderCache::new();
    populate_large_session(&mut cache, 500);
    assert!(cache.blocks_count() >= 1_000);

    cache.ensure_visual_lines(80);

    for i in 0..300 {
        let before = cache.cache_misses();
        cache.append_streaming_delta(BlockKind::Assistant, &format!(" delta_{}", i));
        cache.ensure_visual_lines(80);
        let delta_misses = cache.cache_misses() - before;
        assert!(
            delta_misses <= 1,
            "Expected at most 1 L2 miss per streaming frame, got {} at delta {}",
            delta_misses,
            i
        );
    }

    let before_burst = cache.cache_misses();
    for i in 0..200 {
        cache.append_streaming_delta(BlockKind::Assistant, &format!(" burst_{}", i));
    }
    let total_after = cache.ensure_visual_lines(80);
    assert!(total_after > 0);
    let burst_misses = cache.cache_misses() - before_burst;
    assert!(
        burst_misses <= 1,
        "Expected at most 1 L2 miss for coalesced delta burst, got {}",
        burst_misses
    );
    let tail_start = total_after.saturating_sub(60);
    let tail_text = cache.extract_text_range(tail_start, 0, total_after.saturating_sub(1), 200);
    assert!(tail_text.contains("delta_299"));
    assert!(tail_text.contains("burst_199"));
}

#[test]
fn test_scale_baseline_resize_sequence_monotonicity() {
    let mut cache = RenderCache::new();
    populate_large_session(&mut cache, 450);
    assert!(cache.blocks_count() >= 1_000);

    let widths = [120u16, 80, 40, 80, 120];
    let mut lines_by_width = Vec::new();

    for &w in &widths {
        let total = cache.ensure_visual_lines(w);
        assert!(total > 0);
        let misses_after = cache.cache_misses();
        let total_steady = cache.ensure_visual_lines(w);
        assert_eq!(total_steady, total);
        assert_eq!(cache.cache_misses() - misses_after, 0);

        let tail_start = total.saturating_sub(25);
        let tail_slice = cache.visible_slice(tail_start, 25);
        assert_eq!(tail_slice.len(), total.min(25));
        lines_by_width.push((w, total));
    }

    let lines_120 = lines_by_width[0].1;
    let lines_80 = lines_by_width[1].1;
    let lines_40 = lines_by_width[2].1;
    assert!(lines_40 > lines_120);
    assert!(lines_40 >= lines_80);
    assert!(lines_80 >= lines_120);
    assert_eq!(lines_by_width[3].1, lines_80);
    assert_eq!(lines_by_width[4].1, lines_120);
}

#[test]
fn test_scale_baseline_thought_streaming_and_consolidate() {
    let mut cache = RenderCache::new();
    populate_large_session(&mut cache, 500);
    let base_blocks = cache.blocks_count();
    assert!(base_blocks >= 1_000);
    cache.ensure_visual_lines(80);

    for i in 0..200 {
        let before = cache.cache_misses();
        cache.append_streaming_delta(BlockKind::Thought, &format!(" step_{}", i));
        cache.ensure_visual_lines(80);
        let delta_misses = cache.cache_misses() - before;
        assert!(
            delta_misses <= 1,
            "Expected at most 1 L2 miss per thought delta, got {} at {}",
            delta_misses,
            i
        );
    }

    assert!(cache.has_unconsolidated_blocks());
    let before_consolidate = cache.cache_misses();
    cache.consolidate_active_turn(Some("final summary"));
    assert!(!cache.has_unconsolidated_blocks());
    assert_eq!(cache.blocks_count(), base_blocks + 2);

    let total_after = cache.ensure_visual_lines(80);
    assert!(total_after > 0);
    assert!(cache.cache_misses() - before_consolidate <= 2);

    let tail_start = total_after.saturating_sub(10);
    let tail_text = cache.extract_text_range(tail_start, 0, total_after.saturating_sub(1), 200);
    assert!(tail_text.contains("final summary"));

    let steady_before = cache.cache_misses();
    cache.ensure_visual_lines(80);
    assert_eq!(cache.cache_misses() - steady_before, 0);
}
