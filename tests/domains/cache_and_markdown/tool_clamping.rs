use frontal::protocol::StepState;
use frontal::ui::cache::{
    clamp_tool_output, RenderCache, TOOL_OUTPUT_HEAD_LINES, TOOL_OUTPUT_MAX_LINE_CHARS,
    TOOL_OUTPUT_TAIL_LINES,
};

#[test]
fn test_clamp_tool_output_under_and_at_limit_unchanged() {
    assert_eq!(clamp_tool_output(""), "");

    let short = (1..=10)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(clamp_tool_output(&short), short);

    let exact_limit = TOOL_OUTPUT_HEAD_LINES + TOOL_OUTPUT_TAIL_LINES;
    let at_limit = (1..=exact_limit)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(clamp_tool_output(&at_limit), at_limit);
}

#[test]
fn test_clamp_tool_output_elides_middle_preserving_head_and_tail() {
    let exact_limit = TOOL_OUTPUT_HEAD_LINES + TOOL_OUTPUT_TAIL_LINES;
    let over_one = (1..=exact_limit + 1)
        .map(|i| format!("row {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let clamped_one = clamp_tool_output(&over_one);
    assert!(clamped_one.contains("[... 1 lines elided in UI ...]"));
    assert!(clamped_one.contains("row 100"));
    assert!(!clamped_one.contains("\nrow 101\n"));
    assert!(clamped_one.contains("row 102"));
    assert!(clamped_one.contains("row 151"));

    let large = (1..=1000)
        .map(|i| format!("entry {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let clamped_large = clamp_tool_output(&large);
    assert!(clamped_large.contains("[... 850 lines elided in UI ...]"));
    assert!(clamped_large.contains("entry 1\n"));
    assert!(clamped_large.contains("entry 100\n"));
    assert!(!clamped_large.contains("entry 500"));
    assert!(clamped_large.contains("entry 951\n"));
    assert!(clamped_large.ends_with("entry 1000"));
}

#[test]
fn test_clamp_tool_output_single_dense_utf8_line_truncated_safely() {
    let dense = "🦀".repeat(50_000);
    let clamped = clamp_tool_output(&dense);
    assert!(clamped.ends_with("… [line truncated]"));
    let prefix = clamped.strip_suffix("… [line truncated]").unwrap();
    assert_eq!(prefix.chars().count(), TOOL_OUTPUT_MAX_LINE_CHARS);

    let mut cache = RenderCache::new();
    let block_id =
        cache.upsert_tool_call(Some(1), "read_file", StepState::Done, None, Some(&dense));
    cache.toggle_block_collapsed(block_id);
    let start = std::time::Instant::now();
    let total = cache.ensure_visual_lines(80);
    assert!(total > 0);
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
}

#[test]
fn test_render_cache_upsert_and_historical_tool_call_apply_clamping() {
    let large = (1..=300)
        .map(|i| format!("out {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let mut cache = RenderCache::new();
    let id1 = cache.upsert_tool_call(Some(1), "bash", StepState::Active, None, Some("starting"));
    assert_eq!(cache.blocks()[0].id, id1);
    cache.upsert_tool_call(Some(1), "bash", StepState::Done, None, Some(&large));
    assert!(cache.blocks()[0]
        .content
        .contains("[... 150 lines elided in UI ...]"));

    cache.push_historical_tool_call(Some(2), "grep", None, Some(&large));
    assert!(cache.blocks()[1]
        .content
        .contains("[... 150 lines elided in UI ...]"));
    assert!(!cache.blocks()[1].content.contains("out 150"));
    assert!(cache.blocks()[1].content.contains("out 300"));
}
