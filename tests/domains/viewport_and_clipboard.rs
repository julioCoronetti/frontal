use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;

use frontal::protocol::StepState;
use frontal::ui::cache::{BlockKind, RenderCache, TurnMeta};
use frontal::ui::clipboard::SystemClipboard;
use frontal::ui::editor::PromptEditor;
use frontal::ui::notify::{osc777_notification, osc9_notification, terminal_bell};
use frontal::ui::viewport::{ConversationViewport, ScrollMode};

use crate::common::{press_key, test_app};

#[test]
fn test_sticky_scroll_state_machine() {
    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(10, 5);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 5);

    viewport.sync_dimensions(20, 5);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 15);

    viewport.scroll_up(3);
    assert_eq!(viewport.mode(), ScrollMode::Unanchored { offset: 12 });
    assert_eq!(viewport.effective_offset(), 12);

    viewport.sync_dimensions(25, 5);
    assert_eq!(viewport.effective_offset(), 12);

    viewport.scroll_down(3);
    assert_eq!(viewport.effective_offset(), 15);

    viewport.scroll_down(10);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 20);
}

#[test]
fn test_viewport_resize_reanchoring() {
    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(100, 20);
    assert_eq!(viewport.mode(), ScrollMode::Anchored);

    viewport.scroll_up(30);
    assert!(matches!(viewport.mode(), ScrollMode::Unanchored { .. }));

    viewport.sync_dimensions(25, 20);
    assert_eq!(viewport.mode(), ScrollMode::Anchored);
}

#[test]
fn test_viewport_clean_text_selection_without_trailing_spaces() {
    let mut cache = RenderCache::new();
    cache.push_user_message("hello world");
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);
    viewport.start_selection(1, 0);
    viewport.update_selection(1, 14);

    let selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(selected, "hello world");
    assert!(!selected.ends_with(' '));
}

#[test]
fn test_clipboard_selection_no_padding_or_indent_pollution() {
    let mut cache = RenderCache::new();
    cache.push_user_message("user prompt text");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant markdown text");
    cache.consolidate_active_turn(None);
    let thought_id = cache.append_streaming_delta(BlockKind::Thought, "internal thought reasoning");
    cache.consolidate_active_turn(None);
    cache.toggle_block_collapsed(thought_id);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(1, 2);
    viewport.update_selection(1, 18);
    let user_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(user_selected, "user prompt text");
    assert!(!user_selected.starts_with("▌ "));
    assert!(!user_selected.ends_with(' '));

    viewport.start_selection(4, 0);
    viewport.update_selection(4, 23);
    let assistant_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(assistant_selected, "assistant markdown text");

    let total = cache.total_lines();
    assert!(total >= 7);
    viewport.start_selection(total - 2, 2);
    viewport.update_selection(total - 2, 28);
    let thought_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(thought_selected, "internal thought reasoning");
}

#[test]
fn test_system_clipboard_osc52_generation() {
    let payload = "hello world";
    let base64 = SystemClipboard::encode_base64(payload.as_bytes());
    assert_eq!(base64, "aGVsbG8gd29ybGQ=");

    let osc = SystemClipboard::format_osc52(payload);
    assert_eq!(osc, "\x1b]52;c;aGVsbG8gd29ybGQ=\x07");

    let mut isolated = SystemClipboard::new_isolated();
    assert!(isolated.is_isolated());
    assert_eq!(isolated.get_text(), None);
    isolated.set_text("isolated text").unwrap();
    assert_eq!(isolated.get_text(), Some("isolated text".to_string()));
}

#[test]
fn test_sensory_notifications_generation() {
    assert_eq!(terminal_bell(), b"\x07");

    let osc9 = osc9_notification("Turn completed successfully");
    assert_eq!(osc9, b"\x1b]9;Turn completed successfully\x07");

    let osc777 = osc777_notification("Frontal", "Model response done");
    assert_eq!(osc777, b"\x1b]777;notify;Frontal;Model response done\x07");

    let sanitized = osc9_notification("hello\x07world\x1btest");
    assert_eq!(sanitized, b"\x1b]9;helloworldtest\x07");
}

#[test]
fn test_prompt_editor_editing_and_submit() {
    let mut editor = PromptEditor::new();
    assert_eq!(editor.take_submitted_expanded(), None);

    editor.insert_char('h');
    editor.insert_char('i');
    assert_eq!(editor.text(), "hi");
    assert_eq!(editor.cursor_coordinates(), (0, 2));

    editor.backspace();
    assert_eq!(editor.text(), "h");
    assert_eq!(editor.cursor_coordinates(), (0, 1));

    editor.insert_str("ello world");
    assert_eq!(editor.text(), "hello world");
    assert_eq!(editor.cursor_coordinates(), (0, 11));

    let submitted = editor.take_submitted_expanded();
    assert_eq!(submitted.as_deref(), Some("hello world"));
    assert_eq!(editor.text(), "");
    assert_eq!(editor.cursor_coordinates(), (0, 0));
}

#[test]
fn test_paste_pill_preservation_across_history_navigation() {
    let (mut app, _) = test_app();
    app.history.record_prompt("historical command");

    let big_paste = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8";
    app.editor.insert_str("prefix: ");
    app.editor.insert_paste(big_paste);
    app.editor.insert_str(" :suffix");

    assert_eq!(app.editor.extmarks().len(), 1);
    let orig_text = app.editor.text();
    assert!(orig_text.contains("[Pasted #1: ~8 lines]"));

    let up = press_key(KeyCode::Up, KeyModifiers::NONE);
    app.handle_terminal_event(up);
    assert_eq!(app.editor.text(), "historical command");

    let down = press_key(KeyCode::Down, KeyModifiers::NONE);
    app.handle_terminal_event(down);
    assert_eq!(app.editor.text(), orig_text);
    assert_eq!(app.editor.extmarks().len(), 1);

    let expanded = app.editor.take_submitted_expanded().unwrap();
    assert!(expanded.contains("line 1\nline 2\nline 3"));
}

#[test]
fn test_viewport_granular_character_selection_single_line() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(BlockKind::Assistant, "alpha bravo charlie");
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 0);
    viewport.update_selection(0, 5);

    let slice = viewport.visible_slice(&cache);
    assert!(!slice.is_empty());
    let line0 = &slice[0];

    let mut reversed_chars = String::new();
    let mut unreversed_chars = String::new();
    for span in &line0.spans {
        if span.style.add_modifier.contains(Modifier::REVERSED) {
            reversed_chars.push_str(&span.content);
        } else {
            unreversed_chars.push_str(&span.content);
        }
    }

    assert_eq!(reversed_chars, "alpha");
    assert_eq!(unreversed_chars, " bravo charlie");
}

#[test]
fn test_viewport_granular_character_selection_multiline() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "first line\n\nsecond line\n\nthird line",
    );
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 1);
    viewport.update_selection(2, 5);

    let slice = viewport.visible_slice(&cache);
    assert!(slice.len() >= 3);

    let line0 = &slice[0];
    let mut line0_rev = String::new();
    for span in &line0.spans {
        if span.style.add_modifier.contains(Modifier::REVERSED) {
            line0_rev.push_str(&span.content);
        }
    }
    assert_eq!(line0_rev, "irst line");

    let line1 = &slice[1];
    for span in &line1.spans {
        assert!(span.style.add_modifier.contains(Modifier::REVERSED));
    }

    let line2 = &slice[2];
    let mut line2_rev = String::new();
    for span in &line2.spans {
        if span.style.add_modifier.contains(Modifier::REVERSED) {
            line2_rev.push_str(&span.content);
        }
    }
    assert_eq!(line2_rev, "third");
}

#[test]
fn test_viewport_reverse_drag_selection_order() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(BlockKind::Assistant, "abcdefghijklmnopqrstuvwxyz");
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 7);
    viewport.update_selection(0, 0);

    let slice = viewport.visible_slice(&cache);
    let mut reversed = String::new();
    for span in &slice[0].spans {
        if span.style.add_modifier.contains(Modifier::REVERSED) {
            reversed.push_str(&span.content);
        }
    }
    assert_eq!(reversed, "abcdefg");

    let mut cache_multi = RenderCache::new();
    cache_multi.append_streaming_delta(
        BlockKind::Assistant,
        "first line\n\nsecond line\n\nthird line",
    );
    cache_multi.consolidate_active_turn(None);
    cache_multi.ensure_visual_lines(80);

    let mut vp_fwd = ConversationViewport::new();
    vp_fwd.sync_dimensions(cache_multi.total_lines(), 20);
    vp_fwd.start_selection(0, 1);
    vp_fwd.update_selection(2, 5);

    let mut vp_rev = ConversationViewport::new();
    vp_rev.sync_dimensions(cache_multi.total_lines(), 20);
    vp_rev.start_selection(2, 5);
    vp_rev.update_selection(0, 1);

    let fwd_slice = vp_fwd.visible_slice(&cache_multi);
    let rev_slice = vp_rev.visible_slice(&cache_multi);

    assert_eq!(fwd_slice.len(), rev_slice.len());
    for (f_line, r_line) in fwd_slice.iter().zip(rev_slice.iter()) {
        assert_eq!(f_line.spans.len(), r_line.spans.len());
        for (f_span, r_span) in f_line.spans.iter().zip(r_line.spans.iter()) {
            assert_eq!(f_span.content, r_span.content);
            assert_eq!(f_span.style, r_span.style);
        }
    }

    assert_eq!(
        vp_fwd.selected_text(&cache_multi),
        vp_rev.selected_text(&cache_multi)
    );
}

#[test]
fn test_viewport_selection_utf8_multibyte_and_out_of_bounds_safety() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(BlockKind::Assistant, "ação 🚀 teste");
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 2);
    viewport.update_selection(0, 2);

    let slice_eq = viewport.visible_slice(&cache);
    for span in &slice_eq[0].spans {
        assert!(!span.style.add_modifier.contains(Modifier::REVERSED));
    }

    viewport.start_selection(0, 1);
    viewport.update_selection(0, 9999);
    let slice_oob = viewport.visible_slice(&cache);
    let mut rev_oob = String::new();
    for span in &slice_oob[0].spans {
        if span.style.add_modifier.contains(Modifier::REVERSED) {
            rev_oob.push_str(&span.content);
        }
    }
    assert_eq!(rev_oob, "ção 🚀 teste");
}

#[test]
fn test_selection_sanitization_filters_metadata_and_headers() {
    let mut cache = RenderCache::new();
    cache.append_streaming_delta(BlockKind::Assistant, "primeira linha");
    cache.consolidate_active_turn_with_meta(
        None,
        Some(TurnMeta {
            mode: "auto".to_string(),
            model: "gemini-2.5-pro".to_string(),
            duration_seconds: Some(1.2),
            output_tokens: Some(45),
        }),
    );
    cache.append_streaming_delta(
        BlockKind::Tool {
            name: "read_file".to_string(),
            state: StepState::Done,
        },
        "file contents payload",
    );
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(0, 0);
    viewport.update_selection(cache.total_lines().saturating_sub(1), 50);

    let extracted = viewport.selected_text(&cache).expect("should extract text");
    assert!(extracted.contains("primeira linha"));
    assert!(extracted.contains("file contents payload"));
    assert!(!extracted.contains("▣ "));
    assert!(!extracted.contains("gemini-2.5-pro"));
    assert!(!extracted.contains("read_file"));
    assert!(!extracted.starts_with('\n'));
    assert!(!extracted.ends_with('\n'));
}

#[test]
fn test_prompt_editor_paste_sanitization_removes_carriage_returns_and_control_chars() {
    let mut editor = PromptEditor::new();
    editor.insert_paste("hello\r\nworld\rtest\x1b[31m\x00\x08!");
    let content = editor.text();
    assert_eq!(content, "hello\nworld\ntest!");
}

#[test]
fn test_adversarial_prompt_shortcuts_and_undo_boundaries() {
    let mut editor = PromptEditor::new();
    assert!(!editor.delete_to_line_start());
    editor.select_all();
    assert!(!editor.has_selection());

    editor.insert_str("🦀 primeira linha\n🚀 segunda linha com emojis");
    editor.select_all();
    editor.select_all();
    assert_eq!(
        editor.selected_text(),
        Some("🦀 primeira linha\n🚀 segunda linha com emojis".to_string())
    );

    editor.insert_str("substituição");
    assert_eq!(editor.text(), "substituição");
    assert!(editor.undo());
    assert_eq!(
        editor.text(),
        "🦀 primeira linha\n🚀 segunda linha com emojis"
    );

    editor.move_home();
    assert!(!editor.delete_to_line_start());
    assert_eq!(
        editor.text(),
        "🦀 primeira linha\n🚀 segunda linha com emojis"
    );

    editor.clear();
    for i in 0..120 {
        editor.insert_str(&format!("w{} ", i));
    }
    let mut undo_count = 0;
    while editor.undo() {
        undo_count += 1;
    }
    assert!(undo_count >= 99);
    assert!(!editor.undo());
}

#[test]
fn test_terminal_escape_codes_and_carriage_return_sanitization() {
    let raw = "Compiling crate...\rProgress 50%\rFinished release [optimized]";
    let wrapped = frontal::ui::cache::wrap_text_utf8(raw, 80);
    assert_eq!(wrapped.len(), 1);
    assert_eq!(wrapped[0], "Finished release [optimized]");
    assert!(!wrapped[0].contains('\r'));

    let ansi_raw = "\x1b[32mSuccess:\x1b[0m \x1b[1m3 tests passed\x1b[0m";
    let wrapped_ansi = frontal::ui::cache::wrap_text_utf8(ansi_raw, 80);
    assert_eq!(wrapped_ansi.len(), 1);
    assert_eq!(wrapped_ansi[0], "Success: 3 tests passed");

    let tab_raw = "Col1\tCol2\tCol3";
    let wrapped_tab = frontal::ui::cache::wrap_text_utf8(tab_raw, 80);
    assert_eq!(wrapped_tab.len(), 1);
    assert_eq!(wrapped_tab[0], "Col1    Col2    Col3");
}
