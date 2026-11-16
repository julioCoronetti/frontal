use std::time::Instant;

use frontal::ui::cache::{wrap_text_utf8, BlockKind, RenderCache};
use frontal::ui::highlight::IncrementalCodeHighlighter;
use frontal::ui::markdown::{
    normalize_streaming_markdown, parse_markdown_segments, MarkdownSegment,
};
use frontal::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BG_SIDEBAR, TEXT_MUTED, TEXT_PRIMARY};
use ratatui::style::Modifier;

#[test]
fn test_markdown_streaming_fuzzing_and_unclosed_fences() {
    let doc = r#"# Main Title
Intro text with `inline code` and **bold text**.
## Section 1
- Item 1
- Item 2 with *italic*
- [x] Completed task
| Header 1 | Header 2 |
|---|---|
| Value A | Value B |
```rust
fn compute(val: i32) -> i32 {
    val * 2
}
```
"#;

    for chunk_size in [1, 2, 5, 13] {
        let mut cache = RenderCache::new();
        let mut byte_offset = 0;
        let bytes = doc.as_bytes();

        while byte_offset < bytes.len() {
            let mut end = (byte_offset + chunk_size).min(bytes.len());
            while end < bytes.len() && !doc.is_char_boundary(end) {
                end += 1;
            }
            let chunk = &doc[byte_offset..end];
            cache.append_streaming_delta(BlockKind::Assistant, chunk);
            let lines = cache.ensure_visual_lines(80);
            assert!(lines > 0);
            byte_offset = end;
        }

        cache.consolidate_active_turn(None);
        let final_lines = cache.ensure_visual_lines(80);
        assert!(final_lines > 10);
    }

    let loose_list = "1. Item 1\n\n2. Item 2\n\n- Bullet 1\n\n- Bullet 2\n";
    let mut list_cache = RenderCache::new();
    list_cache.append_streaming_delta(BlockKind::Assistant, loose_list);
    list_cache.consolidate_active_turn(None);
    let list_lines_count = list_cache.ensure_visual_lines(80);
    let list_slice = list_cache.visible_slice(0, list_lines_count);
    let text: String = list_slice
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text.contains("1. Item 1") && text.contains("2. Item 2"));
    assert!(text.contains("• Bullet 1") && text.contains("• Bullet 2"));
    for line in list_slice {
        let l_txt: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_ne!(l_txt.trim(), "•");
        assert_ne!(l_txt.trim(), "1.");
        assert_ne!(l_txt.trim(), "2.");
    }
}

#[test]
fn test_unclosed_code_fence_normalization() {
    let partial = "Here is code:\n```rust\nfn main() {";
    let norm = normalize_streaming_markdown(partial, false);
    assert!(norm.has_unclosed_fence);
    assert_eq!(norm.fence_lang.as_deref(), Some("rust"));
    assert!(norm.text.ends_with("```\n"));

    let partial_no_newline = "Here is code:\n```rust";
    let norm2 = normalize_streaming_markdown(partial_no_newline, false);
    assert!(norm2.has_unclosed_fence);
    assert_eq!(norm2.fence_lang.as_deref(), Some("rust"));
    assert!(norm2.text.ends_with("```\n"));

    let closed = "Here is code:\n```rust\nfn main() {}\n```\n";
    let norm3 = normalize_streaming_markdown(closed, false);
    assert!(!norm3.has_unclosed_fence);
}

#[test]
fn test_tilde_code_fence_normalization_and_rendering() {
    let unclosed_tilde = "Texto antes\n~~~rust\nlet x = 42;";
    let normalized = normalize_streaming_markdown(unclosed_tilde, false);
    assert!(normalized.has_unclosed_fence);
    assert_eq!(normalized.fence_lang.as_deref(), Some("rust"));
    assert!(normalized.text.ends_with("~~~\n"));

    let tilde_fence = "Texto antes\n~~~rust\nlet x = 42;\n~~~\nTexto depois";
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(BlockKind::Assistant, tilde_fence);
    cache.consolidate_active_turn(None);
    let lines = cache.ensure_visual_lines(80);
    assert!(lines > 0);

    let slice = cache.visible_slice(0, 10);
    let text: String = slice
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(text.contains("┌─ rust"));
}

#[test]
fn test_incremental_code_highlighting_checkpoints_and_budget() {
    let mut hl = IncrementalCodeHighlighter::new();
    let mut code = String::new();

    for i in 0..50 {
        code.push_str(&format!("fn func_{}() -> i32 {{ {} }}\n", i, i));
    }

    let spans = hl.highlight_code("rust", &code);
    assert_eq!(spans.len(), 50);
    assert_eq!(hl.checkpoints_count(), 50);

    code.push_str("fn func_50() -> i32 { 50 }\n");
    let spans2 = hl.highlight_code("rust", &code);
    assert_eq!(spans2.len(), 51);
    assert_eq!(hl.checkpoints_count(), 51);

    let mut cache = RenderCache::new();
    let mut large_block = String::from("```rust\n");
    for i in 0..300 {
        large_block.push_str(&format!("let x_{} = {} + 1;\n", i, i));
    }
    cache.append_streaming_delta(BlockKind::Assistant, &large_block);
    cache.ensure_visual_lines(100);

    let start = Instant::now();
    cache.append_streaming_delta(BlockKind::Assistant, "let final_line = 999;\n```\n");
    let total = cache.ensure_visual_lines(100);
    let elapsed = start.elapsed();

    assert!(total > 300);
    assert!(
        elapsed.as_millis() < 50,
        "Incremental render took {:?}, exceeding 50ms budget",
        elapsed
    );
}

#[test]
fn test_syntect_checkpoint_invalidation_on_middle_edit() {
    let mut hl = IncrementalCodeHighlighter::new();
    let code_v1 = "fn main() {\n    let a = 1;\n    let b = 2;\n    println!(\"{}\", a + b);\n}";
    let res1 = hl.highlight_code("rust", code_v1);
    assert_eq!(res1.len(), 5);

    let code_v2 = "fn main() {\n    let b = 2;\n    println!(\"{}\", b);\n}";
    let res2 = hl.highlight_code("rust", code_v2);
    assert_eq!(res2.len(), 4);
}

#[test]
fn test_zero_markdown_reparse_on_resize_of_consolidated_block() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "# Title\nParagraph with **bold** text.\n- item 1\n- item 2",
    );
    cache.consolidate_active_turn(None);

    let lines_80 = cache.ensure_visual_lines(80);
    assert!(lines_80 > 0 && cache.cache_misses() == 1 && cache.cache_hits() == 0);

    let lines_60 = cache.ensure_visual_lines(60);
    assert!(lines_60 > 0 && cache.cache_misses() == 2 && cache.cache_hits() == 0);

    let lines_60_cached = cache.ensure_visual_lines(60);
    assert_eq!(lines_60, lines_60_cached);
    assert_eq!(cache.cache_hits(), 1);
    assert_eq!(cache.cache_misses(), 2);

    let lines_100 = cache.ensure_visual_lines(100);
    assert!(lines_100 > 0 && cache.cache_misses() == 3);
}

#[test]
fn test_table_wrapping_on_narrow_widths() {
    let mut cache = RenderCache::new();
    let table_md = "| Column 1 with long title | Column 2 with very long content |\n|---|---|\n| row 1 value a | row 1 value b |\n| row 2 value c | row 2 value d |";
    cache.append_streaming_delta(BlockKind::Assistant, table_md);
    cache.consolidate_active_turn(None);

    let lines_narrow = cache.ensure_visual_lines(20);
    assert!(lines_narrow > 0);
    let slice = cache.visible_slice(0, lines_narrow);
    for line in slice {
        let len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(len <= 20);
    }

    let lines_super_narrow = cache.ensure_visual_lines(10);
    assert!(lines_super_narrow > 0);
}

#[test]
fn test_utf8_word_wrap_safety() {
    let text = "Olá, Senhor! Testando acentuação e caracteres especiais: coração, ação, 🦀, café.";
    let wrapped_5 = wrap_text_utf8(text, 5);
    for line in &wrapped_5 {
        assert!(line.chars().count() <= 5);
    }

    let wrapped_1 = wrap_text_utf8("ação", 1);
    assert_eq!(wrapped_1, vec!["a", "ç", "ã", "o"]);
}

#[test]
fn test_markdown_inline_code_strips_backticks_and_uses_atlas_crimson() {
    let segs = parse_markdown_segments("Use `cargo test` here", true);
    assert_eq!(segs.len(), 1);
    let MarkdownSegment::Paragraph(spans) = &segs[0] else {
        panic!("expected paragraph");
    };
    let code_span = spans
        .iter()
        .find(|s| s.content.as_ref() == "cargo test")
        .expect("inline code span without backticks");
    assert_eq!(code_span.style.fg, Some(ACCENT_HIGHLIGHT));
    assert_eq!(code_span.style.bg, Some(BG_SIDEBAR));
    assert!(!spans.iter().any(|s| s.content.contains('`')));
}

#[test]
fn test_markdown_heading_levels_and_inline_code_inheritance() {
    let segs = parse_markdown_segments("# H1 `cmd`\n## H2\n### H3\n#### H4", true);
    assert_eq!(segs.len(), 4);

    let MarkdownSegment::Heading {
        level: 1,
        spans: h1,
    } = &segs[0]
    else {
        panic!("expected H1");
    };
    assert_eq!(h1[0].style.fg, Some(ACCENT_HIGHLIGHT));
    assert!(h1[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD | Modifier::UNDERLINED));
    let h1_code = h1.iter().find(|s| s.content.as_ref() == "cmd").unwrap();
    assert_eq!(h1_code.style.fg, Some(ACCENT_HIGHLIGHT));
    assert_eq!(h1_code.style.bg, Some(BG_SIDEBAR));
    assert!(h1_code.style.add_modifier.contains(Modifier::BOLD));

    let MarkdownSegment::Heading {
        level: 2,
        spans: h2,
    } = &segs[1]
    else {
        panic!("expected H2");
    };
    assert_eq!(h2[0].style.fg, Some(ACCENT_PRIMARY));
    assert!(h2[0].style.add_modifier.contains(Modifier::BOLD));

    let MarkdownSegment::Heading {
        level: 3,
        spans: h3,
    } = &segs[2]
    else {
        panic!("expected H3");
    };
    assert_eq!(h3[0].style.fg, Some(TEXT_PRIMARY));
    assert!(h3[0].style.add_modifier.contains(Modifier::BOLD));

    let MarkdownSegment::Heading {
        level: 4,
        spans: h4,
    } = &segs[3]
    else {
        panic!("expected H4");
    };
    assert_eq!(h4[0].style.fg, Some(TEXT_MUTED));
    assert!(h4[0]
        .style
        .add_modifier
        .contains(Modifier::BOLD | Modifier::ITALIC));
}

#[test]
fn test_markdown_nested_list_depth_and_continuation_fallback() {
    let md = "1. First\n   - Sub A\n     - Sub B\n\n   Continuation line\n2. Second";
    let segs = parse_markdown_segments(md, true);
    let items: Vec<(usize, &str, String)> = segs
        .iter()
        .filter_map(|s| match s {
            MarkdownSegment::ListItem {
                depth,
                bullet,
                spans,
            } => {
                let txt: String = spans.iter().map(|sp| sp.content.as_ref()).collect();
                Some((*depth, bullet.as_str(), txt))
            }
            _ => None,
        })
        .collect();

    assert_eq!(items.len(), 5);
    assert_eq!(items[0], (1, "1. ", "First".to_string()));
    assert_eq!(items[1], (2, "◦ ", "Sub A".to_string()));
    assert_eq!(items[2], (3, "▪ ", "Sub B".to_string()));
    assert_eq!(items[3], (1, "  ", "Continuation line".to_string()));
    assert_eq!(items[4], (1, "2. ", "Second".to_string()));
}
