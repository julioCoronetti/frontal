use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use frontal::session::SessionStatus;
use frontal::ui::app::AppAction;
use frontal::ui::modal::Modal;

use crate::common::{press_key, test_app};

#[test]
fn test_local_slash_commands_execution() {
    let (mut app, _) = test_app();

    app.editor.insert_str("/quit");
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, Some(AppAction::Quit));
    assert!(app.should_quit);

    app.should_quit = false;
    app.cache.push_user_message("something");
    app.editor.insert_str("/clear");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert_eq!(app.cache.blocks_count(), 0);

    app.editor.insert_str("/help");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert!(app.cache.blocks_count() > 0);

    app.editor.insert_str("/interrupt");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, Some(AppAction::Interrupt));

    app.editor.insert_str("/model gemini");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert!(!app.waiting_for_response);

    let _ = app.cache.upsert_tool_call(
        Some(1),
        "test_tool",
        frontal::protocol::StepState::Done,
        None,
        Some("output"),
    );
    let backtab = press_key(KeyCode::BackTab, KeyModifiers::NONE);
    let act = app.handle_terminal_event(backtab);
    assert_eq!(act, None);
}

#[test]
fn test_mouse_selection_and_scroll_events_via_app() {
    let (mut app, _) = test_app();
    app.viewport_bounds = (0, 0, 80, 20);

    for i in 0..30 {
        app.cache.push_user_message(&format!("message line {}", i));
    }
    app.cache.consolidate_active_turn(None);
    app.cache.ensure_visual_lines(80);
    app.viewport.sync_dimensions(app.cache.total_lines(), 20);

    let scroll_up = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(scroll_up);
    assert!(!app.viewport.is_anchored());

    let scroll_down = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    for _ in 0..10 {
        app.handle_terminal_event(scroll_down.clone());
    }
    assert!(app.viewport.is_anchored());

    let mouse_down = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_down);

    let mouse_drag = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 15,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_drag);
    assert!(app.viewport.selected_text(&app.cache).is_some());

    let mouse_up = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 15,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_up);
    assert!(app.viewport.selected_text(&app.cache).is_some());
    assert_eq!(app.clipboard.get_text(), None);
    assert!(app.editor.is_empty());

    let ctrl_c = press_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    app.handle_terminal_event(ctrl_c);
    assert!(app.viewport.selected_text(&app.cache).is_none());
    assert!(app.clipboard.get_text().is_some());
    assert!(app.editor.is_empty());
}

#[test]
fn test_bracketed_paste_and_multiline_via_app() {
    let (mut app, _) = test_app();

    let paste_event = Event::Paste("primeira linha\nsegunda linha".to_string());
    app.handle_terminal_event(paste_event);
    assert_eq!(app.editor.text(), "primeira linha\nsegunda linha");
    assert_eq!(app.editor.line_count(), 2);

    let enter_with_shift = press_key(KeyCode::Enter, KeyModifiers::SHIFT);
    app.handle_terminal_event(enter_with_shift);
    assert_eq!(app.editor.line_count(), 3);

    let ctrl_j = press_key(KeyCode::Char('j'), KeyModifiers::CONTROL);
    app.handle_terminal_event(ctrl_j);
    assert_eq!(app.editor.line_count(), 4);

    let ctrl_v = press_key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    app.clipboard.set_text("quarta linha").unwrap();
    app.handle_terminal_event(ctrl_v);
    assert!(app.editor.text().contains("quarta linha"));
}

#[test]
fn test_contextual_ctrl_c_and_slash_commands_routing() {
    let (mut app, sid) = test_app();
    app.session_manager.set_session_metadata(
        &sid,
        Some("c1".to_string()),
        Some("m1".to_string()),
        None,
        None,
    );

    let ctrl_c = press_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let act_idle_first = app.handle_terminal_event(ctrl_c.clone());
    assert_eq!(act_idle_first, None);
    assert!(!app.should_quit);
    assert!(app.status_text.contains("Ctrl+C"));
    assert!(app.last_ctrl_c_at.is_some());

    let other_key = press_key(KeyCode::Left, KeyModifiers::NONE);
    app.handle_terminal_event(other_key);
    assert!(app.last_ctrl_c_at.is_none());

    let act_after_intervening = app.handle_terminal_event(ctrl_c.clone());
    assert_eq!(act_after_intervening, None);
    assert!(!app.should_quit);
    assert!(app.last_ctrl_c_at.is_some());

    let act_idle_second = app.handle_terminal_event(ctrl_c.clone());
    assert_eq!(act_idle_second, Some(AppAction::Quit));
    assert!(app.should_quit);

    app.should_quit = false;
    app.last_ctrl_c_at = None;
    app.editor.insert_str("draft to be cleared");
    let act_clear = app.handle_terminal_event(ctrl_c.clone());
    assert_eq!(act_clear, None);
    assert!(app.editor.is_empty());
    assert!(!app.should_quit);

    app.should_quit = false;
    app.session_manager
        .set_session_status(&sid, SessionStatus::RunningTurn);
    let act_running = app.handle_terminal_event(ctrl_c);
    assert_eq!(act_running, Some(AppAction::Interrupt));

    app.session_manager
        .set_session_status(&sid, SessionStatus::Ready);
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);

    app.editor.insert_str("/diff");
    app.handle_terminal_event(enter.clone());
    assert!(matches!(app.modals.top(), Some(Modal::Diff(_))));
    app.modals.pop();

    app.editor.insert_str("/respawn");
    app.handle_terminal_event(enter.clone());
    assert!(matches!(app.modals.top(), Some(Modal::HotRespawn(_))));
    app.modals.pop();

    app.editor.insert_str("/unknown_cmd");
    app.handle_terminal_event(enter);
    assert_eq!(app.cache.blocks_count(), 2);
    app.cache.ensure_visual_lines(80);
    let all_lines = app.cache.visible_slice(0, app.cache.total_lines());
    let full_text = all_lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(full_text.contains("is not supported locally"));
}
