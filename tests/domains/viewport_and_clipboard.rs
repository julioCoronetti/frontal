use crossterm::event::{KeyCode, KeyModifiers};

use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::clipboard::SystemClipboard;
use frontal::ui::input_stub::MinimalInputStub;
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
    cache.append_streaming_delta(BlockKind::Thought, "internal thought reasoning");
    cache.consolidate_active_turn(None);
    cache.ensure_visual_lines(80);

    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(cache.total_lines(), 20);

    viewport.start_selection(1, 3);
    viewport.update_selection(1, 19);
    let user_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(user_selected, "user prompt text");
    assert!(!user_selected.starts_with(" ┃ "));
    assert!(!user_selected.ends_with(' '));

    viewport.start_selection(4, 3);
    viewport.update_selection(4, 26);
    let assistant_selected = viewport.selected_text(&cache).unwrap();
    assert_eq!(assistant_selected, "assistant markdown text");
    assert!(!assistant_selected.starts_with("   "));

    let total = cache.total_lines();
    assert!(total >= 7);
    viewport.start_selection(total - 1, 5);
    viewport.update_selection(total - 1, 31);
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
fn test_minimal_input_stub_editing_and_submit() {
    let mut input = MinimalInputStub::new();
    assert_eq!(input.take_submitted(), None);

    input.insert_char('h');
    input.insert_char('i');
    assert_eq!(input.content(), "hi");
    assert_eq!(input.cursor_char_idx(), 2);

    input.backspace();
    assert_eq!(input.content(), "h");
    assert_eq!(input.cursor_char_idx(), 1);

    input.insert_str("ello world");
    assert_eq!(input.content(), "hello world");
    assert_eq!(input.cursor_char_idx(), 11);

    let submitted = input.take_submitted();
    assert_eq!(submitted.as_deref(), Some("hello world"));
    assert_eq!(input.content(), "");
    assert_eq!(input.cursor_char_idx(), 0);
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
