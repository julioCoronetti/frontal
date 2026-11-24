use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Modifier;

use crate::common::{press_key, render_app, test_app};

#[test]
fn test_app_shortcuts_ctrl_a_ctrl_c_backspace_undo_and_ctrl_u() {
    let (mut app, _) = test_app();
    app.active_tab_mut()
        .editor
        .insert_str("primeira linha\nsegunda linha");

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert!(app.active_tab().editor.has_selection());
    assert_eq!(
        app.active_tab().editor.selected_text(),
        Some("primeira linha\nsegunda linha".to_string())
    );

    let (terminal, _) = render_app(&mut app, 80, 24);
    let buf = terminal.backend().buffer();
    let has_reversed = buf
        .content()
        .iter()
        .any(|c| c.symbol() == "p" && c.modifier.contains(Modifier::REVERSED));
    assert!(has_reversed);

    app.handle_terminal_event(press_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(
        app.clipboard.get_text(),
        Some("primeira linha\nsegunda linha".to_string())
    );
    assert!(!app.active_tab().editor.has_selection());
    assert_eq!(
        app.active_tab().editor.text(),
        "primeira linha\nsegunda linha"
    );

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    app.handle_terminal_event(press_key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.active_tab().editor.text(), "");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(
        app.active_tab().editor.text(),
        "primeira linha\nsegunda linha"
    );

    app.active_tab_mut().editor.move_end();
    app.handle_terminal_event(press_key(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(app.active_tab().editor.text(), "primeira linha\n");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(
        app.active_tab().editor.text(),
        "primeira linha\nsegunda linha"
    );

    app.handle_terminal_event(press_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.active_tab().editor.text(), "");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(
        app.active_tab().editor.text(),
        "primeira linha\nsegunda linha"
    );

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert!(app.active_tab().editor.has_selection());
    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.active_tab().editor.has_selection());
}
