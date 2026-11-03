use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use frontal::ui::diff::{DiffLineKind, DiffViewer};
use frontal::ui::modal::{CommandPaletteModal, HotRespawnModal, ModalAction, ModelEffortModal};
use frontal::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_PRIMARY};

use crate::common::{press_key, test_app};

#[test]
fn test_modal_stack_push_pop_and_exclusive_focus() {
    let (mut app, _) = test_app();

    assert!(app.modals.is_empty());

    let ctrl_p = press_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
    let act = app.handle_terminal_event(ctrl_p);
    assert_eq!(act, None);
    assert!(!app.modals.is_empty());

    let key_a = press_key(KeyCode::Char('a'), KeyModifiers::NONE);
    app.handle_terminal_event(key_a);
    assert!(app.editor.is_empty());

    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);
    app.handle_terminal_event(esc);
    assert!(app.modals.is_empty());

    let key_b = press_key(KeyCode::Char('b'), KeyModifiers::NONE);
    app.handle_terminal_event(key_b);
    assert_eq!(app.editor.text(), "b");
}

#[test]
fn test_hot_respawn_modal_navigation_and_submission() {
    let mut modal = HotRespawnModal::new(
        Some("conv-123".to_string()),
        Some("gemini-3.8-flash-high".to_string()),
        Some("high".to_string()),
        Some("auto".to_string()),
    );

    assert_eq!(modal.active_field, 0);

    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    modal.handle_key(tab);
    assert_eq!(modal.active_field, 1);

    let char_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    modal.handle_key(char_x);
    assert_eq!(modal.model, "gemini-3.8-flash-highx");

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act = modal.handle_key(enter);
    assert_eq!(
        act,
        ModalAction::Respawn {
            conversation_id: Some("conv-123".to_string()),
            model: Some("gemini-3.8-flash-highx".to_string()),
            effort: Some("high".to_string()),
            mode: Some("auto".to_string()),
        }
    );
}

#[test]
fn test_command_palette_filtering_and_action_dispatch() {
    let mut palette = CommandPaletteModal::new();

    let key_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
    let key_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
    let key_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
    palette.handle_key(key_d);
    palette.handle_key(key_i);
    palette.handle_key(key_f);

    let filtered = palette.filtered_indices();
    assert_eq!(filtered.len(), 1);
    assert_eq!(palette.items[filtered[0]].id, "view_diff");

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act = palette.handle_key(enter);
    assert_eq!(act, ModalAction::ExecuteCommand("view_diff".to_string()));
}

#[test]
fn test_integrated_diff_viewer_parsing_and_scrolling() {
    let sample = r#"diff --git a/src/main.rs b/src/main.rs
index 1234..5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn test() {
-    let x = 1;
+    let x = 2;
+    let y = 3;
 }
"#;

    let mut viewer = DiffViewer::new(sample);
    assert_eq!(viewer.line_count(), 10);
    assert_eq!(viewer.lines[0].kind, DiffLineKind::Header);
    assert_eq!(viewer.lines[4].kind, DiffLineKind::HunkHeader);
    assert_eq!(viewer.lines[6].kind, DiffLineKind::Deletion);
    assert_eq!(viewer.lines[7].kind, DiffLineKind::Addition);
    assert_eq!(viewer.lines[8].kind, DiffLineKind::Addition);

    let rendered = viewer.render_lines(5);
    assert_eq!(rendered.len(), 5);

    viewer.scroll_down(3);
    assert_eq!(viewer.scroll_offset, 3);
    viewer.scroll_up(2);
    assert_eq!(viewer.scroll_offset, 1);
}

#[test]
fn test_adversarial_modal_and_palette_inputs() {
    let mut palette = CommandPaletteModal::new();
    for _ in 0..10 {
        let bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        palette.handle_key(bs);
    }
    assert!(palette.query.is_empty());
    assert_eq!(palette.filtered_indices().len(), palette.items.len());

    let special = "non_existent_command_xyz_123!@#";
    for c in special.chars() {
        let ch = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        palette.handle_key(ch);
    }
    assert_eq!(palette.filtered_indices().len(), 0);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act = palette.handle_key(enter);
    assert_eq!(act, ModalAction::None);

    let mut empty_diff = DiffViewer::new("");
    assert_eq!(empty_diff.line_count(), 0);
    empty_diff.scroll_down(10);
    assert_eq!(empty_diff.scroll_offset, 0);
    empty_diff.scroll_up(10);
    assert_eq!(empty_diff.scroll_offset, 0);
    assert!(empty_diff.render_lines(10).is_empty());

    let mut modal = ModelEffortModal::new();
    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(esc), ModalAction::Close);

    let mut modal2 = ModelEffortModal::new();
    let key_c = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
    modal2.handle_key(key_c);
    assert_eq!(modal2.query, "f");
    assert_eq!(modal2.handle_key(esc), ModalAction::Close);
}

#[test]
fn test_modal_shortcut_keys_do_not_leak_chars() {
    let mut respawn = HotRespawnModal::new(None, None, None, None);
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let alt_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
    respawn.handle_key(ctrl_c);
    respawn.handle_key(alt_x);
    assert!(respawn.conversation_id.is_empty());

    let mut palette = CommandPaletteModal::new();
    palette.handle_key(ctrl_c);
    palette.handle_key(alt_x);
    assert!(palette.query.is_empty());
}

#[test]
fn test_respawn_active_session_metadata_inheritance() {
    let (mut app, initial_id) = test_app();
    app.session_manager.set_session_metadata(
        &initial_id,
        Some("conv-inherited".to_string()),
        Some("gemini-4-argon-high".to_string()),
        Some("high".to_string()),
        Some("architect".to_string()),
    );
    assert_eq!(app.active_model_name(), "gemini-4-argon-high");
    assert_eq!(app.active_mode_name(), "architect");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let new_session_id = app
            .respawn_active_session(
                None,
                Some("gemini-3.8-flash-high".to_string()),
                Some("low".to_string()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(app.active_session, new_session_id);
        assert_eq!(app.tabs[0].session_id, new_session_id);
        assert_eq!(app.active_model_name(), "gemini-3.8-flash-high");
        assert_eq!(app.active_mode_name(), "architect");
        let meta = app.session_manager.get_metadata(&new_session_id).unwrap();
        assert_eq!(meta.conversation_id.as_deref(), Some("conv-inherited"));
        assert_eq!(meta.effort.as_deref(), Some("low"));
        assert_eq!(
            app.session_manager.active_session_id(),
            Some(&new_session_id)
        );
        assert!(app.session_manager.interrupt_active().is_ok());
    });
}

#[test]
fn test_modal_backdrop_dimming_overlay() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    app.cache.push_user_message("Testing backdrop overlay text");
    app.cache.consolidate_active_turn(None);
    terminal.draw(|f| app.render(f)).unwrap();

    let ctrl_p = press_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
    app.handle_terminal_event(ctrl_p);
    assert!(!app.modals.is_empty());

    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer().clone();

    let cell_outside = &buf[(1, 1)];
    assert!(cell_outside.modifier.contains(Modifier::DIM));
    assert_eq!(cell_outside.bg, Color::Black);

    let cell_inside = &buf[(50, 15)];
    assert_ne!(cell_inside.symbol(), "");

    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);
    app.handle_terminal_event(esc);
    assert!(app.modals.is_empty());

    terminal.draw(|f| app.render(f)).unwrap();
    let buf_closed = terminal.backend().buffer().clone();
    let cell_restored = &buf_closed[(1, 1)];
    assert!(!cell_restored.modifier.contains(Modifier::DIM));
}

#[test]
fn test_modal_palette_and_respawn_atlas_crimson_styling() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let palette = CommandPaletteModal::new();
    terminal
        .draw(|f| palette.render(f, ratatui::layout::Rect::new(10, 2, 60, 20)))
        .unwrap();
    let buf = terminal.backend().buffer();

    let mut found_prompt_highlight = false;
    let mut found_selected_ruby = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).unwrap();
            if cell.symbol() == ">" && cell.fg == ACCENT_HIGHLIGHT {
                found_prompt_highlight = true;
            }
            if cell.bg == ACCENT_RUBY && cell.fg == TEXT_PRIMARY {
                found_selected_ruby = true;
            }
        }
    }
    let border_cell = buf.cell((10, 2)).unwrap();
    assert_eq!(border_cell.fg, BORDER_SUBTLE);
    assert!(found_prompt_highlight);
    assert!(found_selected_ruby);

    let respawn = HotRespawnModal::new(
        Some("c-1".to_string()),
        Some("m-1".to_string()),
        Some("high".to_string()),
        Some("auto".to_string()),
    );
    terminal
        .draw(|f| respawn.render(f, ratatui::layout::Rect::new(10, 2, 60, 20)))
        .unwrap();
    let buf2 = terminal.backend().buffer();

    let mut found_active_field_highlight = false;
    let mut found_active_val_ruby = false;
    for y in 0..buf2.area.height {
        for x in 0..buf2.area.width {
            let cell = buf2.cell((x, y)).unwrap();
            if cell.symbol() == "C" && cell.fg == ACCENT_HIGHLIGHT {
                found_active_field_highlight = true;
            }
            if cell.bg == ACCENT_RUBY && cell.fg == TEXT_PRIMARY {
                found_active_val_ruby = true;
            }
        }
    }
    assert!(found_active_field_highlight);
    assert!(found_active_val_ruby);
}
