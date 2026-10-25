use std::time::Instant;

use frontal::protocol::StepState;
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::highlight::IncrementalCodeHighlighter;
use frontal::ui::markdown::normalize_streaming_markdown;

#[test]
fn test_markdown_streaming_fuzzing_and_unclosed_fences() {
    let doc = r#"# Main Title
Here is some intro text with `inline code` and **bold text**.

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

And a python script:
```python
def hello():
    print("world")
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
    assert!(initial_lines > 5);

    let hits_1 = cache.cache_hits();
    let misses_1 = cache.cache_misses();
    assert_eq!(misses_1, 3);
    assert_eq!(hits_1, 0);

    let collapsed = cache.toggle_block_collapsed(thought_id);
    assert!(collapsed);

    let collapsed_lines = cache.ensure_visual_lines(80);
    assert!(collapsed_lines < initial_lines);

    let hits_2 = cache.cache_hits();
    let misses_2 = cache.cache_misses();

    assert_eq!(misses_2 - misses_1, 1);
    assert_eq!(hits_2 - hits_1, 2);

    let uncollapsed = cache.toggle_block_collapsed(thought_id);
    assert!(!uncollapsed);

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
    let text = slice
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<Vec<_>>()
                .join("")
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("[DONE] read_file"));
    assert!(text.contains("[ERROR] read_file"));
    assert!(text.contains("FileNotFound"));

    cache.consolidate_active_turn(None);
    assert_eq!(cache.blocks_count(), 2);
}
