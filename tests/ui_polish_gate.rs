use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::protocol::{SessionId, StepState};
use frontal::session::SessionManager;
use frontal::ui::app::App;
use frontal::ui::cache::{BlockKind, RenderCache, TurnMeta};
use frontal::ui::modal::{CommandPaletteModal, DiffModal, HotRespawnModal, ModelEffortModal};
use frontal::ui::theme::BG_USER_CONTAINER;
use frontal::ui::viewport::ConversationViewport;

#[test]
fn test_opencode_visual_layout_and_symmetry() {
    let sm = SessionManager::new();
    let id = SessionId::new();
    let mut app = App::new(sm, id);

    app.cache.push_user_message("Hello from user prompt");
    app.cache.append_streaming_delta(
        BlockKind::Assistant,
        "Hello from assistant response\nSecond line",
    );
    let meta = TurnMeta {
        mode: "auto".to_string(),
        model: "gemini-4-argon-high".to_string(),
        duration_seconds: Some(1.5),
        output_tokens: Some(300),
    };
    app.cache
        .consolidate_active_turn_with_meta(None, Some(meta));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf = terminal.backend().buffer();
    let rendered_text = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered_text.contains('●'));
    assert!(rendered_text.contains("Session 1"));
    assert!(rendered_text.contains(&app.cwd_display));
    assert!(rendered_text.contains("Ctrl+P: Palette"));
    assert!(!rendered_text.contains("#hex"));
    assert!(!rendered_text.contains("/model"));
    assert!(rendered_text.contains("▣ auto · gemini-4-argon-high · 1.5s · 200.0 tok/s"));

    let mut found_user_bg = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).unwrap();
            if cell.bg == BG_USER_CONTAINER {
                found_user_bg = true;
                break;
            }
        }
        if found_user_bg {
            break;
        }
    }
    assert!(found_user_bg);

    let visual_lines = app.cache.visible_slice(0, 20);
    assert!(visual_lines.len() >= 3);
    let assistant_first_line = &visual_lines[2];
    assert_eq!(assistant_first_line.spans[0].content.as_ref(), "   ");
}

#[test]
fn test_clipboard_selection_no_padding_or_indent_pollution() {
    let mut cache = RenderCache::new();
    cache.push_user_message("user prompt text");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant markdown text");
    cache.consolidate_active_turn(None);
    cache.append_streaming_delta(BlockKind::Thought, "internal thought reasoning");
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 3);
    viewport.update_selection(0, 19);
    let user_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(user_selected, "user prompt text");
    assert!(!user_selected.starts_with(" ┃ "));
    assert!(!user_selected.ends_with(' '));

    viewport.start_selection(2, 3);
    viewport.update_selection(2, 26);
    let assistant_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(assistant_selected, "assistant markdown text");
    assert!(!assistant_selected.starts_with("   "));

    let total = cache.total_lines();
    assert!(total >= 5);
    viewport.start_selection(total - 1, 5);
    viewport.update_selection(total - 1, 31);
    let thought_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(thought_selected, "internal thought reasoning");
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
fn test_adversarial_terminal_dimensions_no_panic() {
    let sm = SessionManager::new();
    let id = SessionId::new();
    let mut app = App::new(sm, id);

    app.cache
        .push_user_message("Long user message to test wrapping in narrow dimensions");
    app.cache.append_streaming_delta(
        BlockKind::Assistant,
        "Assistant multi-line response\n```rust\nlet x = 10;\n```\n| col1 | col2 |\n|---|---|\n| a | b |",
    );
    app.cache.consolidate_active_turn(None);
    app.cache
        .append_streaming_delta(BlockKind::Thought, "Thought text for extreme dimensions");
    app.cache.consolidate_active_turn(None);
    app.cache.upsert_tool_call(
        Some(1),
        "write_file",
        StepState::Active,
        Some("{\"path\":\"src/lib.rs\"}"),
        None,
    );

    let adversarial_sizes = [(100, 30), (39, 12), (20, 8), (10, 5), (1, 1)];

    for &(w, h) in &adversarial_sizes {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        let draw_res = terminal.draw(|f| app.render(f));
        assert!(draw_res.is_ok());
    }

    let model_modal = ModelEffortModal::new();
    let cmd_modal = CommandPaletteModal::new();
    let respawn_modal = HotRespawnModal::new(None, None, None, None);
    let diff_modal = DiffModal::new("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new");

    for &(w, h) in &[(20, 8), (1, 1)] {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| model_modal.render(f, f.area())).unwrap();
        terminal.draw(|f| cmd_modal.render(f, f.area())).unwrap();
        terminal
            .draw(|f| respawn_modal.render(f, f.area()))
            .unwrap();
        terminal.draw(|f| diff_modal.render(f, f.area())).unwrap();
    }
}
