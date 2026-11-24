#[test]
fn test_sidebar_toggle_via_command_and_commands_modal() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use frontal::ui::modal::{CommandsModal, ModalAction};

    let (mut app, _) = crate::common::test_app();
    app.active_tab_mut()
        .cache
        .push_user_message("prompt for sidebar telemetry");
    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("Context"));

    app.active_tab_mut().editor.insert_str("/sidebar");
    app.handle_terminal_event(crate::common::press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.active_tab().status_text, "Unknown command: /sidebar");
    assert!(app.sidebar_visible);

    let ctrl_b = crate::common::press_key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    app.handle_terminal_event(ctrl_b.clone());
    assert!(!app.sidebar_visible);
    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(!text.contains("tokens"));
    assert_eq!(app.viewport_bounds.2, 116);

    let mut commands = CommandsModal::new();
    for ch in ['s', 'i', 'd', 'e', 'b', 'a', 'r'] {
        commands.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let filtered = commands.filtered_indices();
    assert_eq!(commands.items[filtered[0]].id, "toggle_sidebar");
    assert_eq!(
        commands.items[filtered[0]].shortcut.as_deref(),
        Some("Ctrl+B")
    );
    let action = commands.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        action,
        ModalAction::ExecuteCommand("toggle_sidebar".to_string())
    );
    app.dispatch_modal_action(action);
    assert!(app.sidebar_visible);
    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("tokens"));
    assert_eq!(app.viewport_bounds.2, 76);

    app.handle_terminal_event(ctrl_b);
    assert!(!app.sidebar_visible);
}
