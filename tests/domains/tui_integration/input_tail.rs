use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::common::{press_key, test_app};

#[test]
fn test_staging_queue_during_active_turn_and_auto_drain() {
    let (mut app, sid) = test_app();
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);

    let step_ev = frontal::protocol::StreamEvent::StepUpdate(frontal::protocol::StepUpdateEvent {
        conversation_id: None,
        step_update: Some(frontal::protocol::StepUpdateData {
            step_index: Some(1),
            text_delta: Some("Streaming response...".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid.clone(),
        event: frontal::protocol::EngineEvent::Stream(Box::new(step_ev)),
    });
    assert!(app.is_turn_active());

    app.active_tab_mut().editor.insert_str("pare");
    let act = app.handle_terminal_event(enter);
    assert_eq!(act, None);
    assert_eq!(app.active_tab().queued_prompts.len(), 1);
    assert_eq!(
        app.active_tab().queued_prompts[0],
        frontal::protocol::PromptPayload::from("pare")
    );
    assert!(app.active_tab().status_text.contains("Queued (1 pending)"));
    assert_eq!(app.active_tab().cache.blocks_count(), 1);

    let res_ev = frontal::protocol::StreamEvent::Result(frontal::protocol::ResultEvent {
        conversation_id: None,
        result: Some(frontal::protocol::ResultData {
            response: Some("Finished turn".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid.clone(),
        event: frontal::protocol::EngineEvent::Stream(Box::new(res_ev)),
    });

    assert_eq!(app.active_tab().queued_prompts.len(), 0);
    assert_eq!(app.pending_engine_prompts.len(), 1);
    assert_eq!(
        app.pending_engine_prompts[0],
        (sid, frontal::protocol::PromptPayload::from("pare"))
    );
    assert_eq!(app.active_tab().cache.blocks_count(), 2);
    assert!(app.active_tab().waiting_for_response);
}

#[test]
fn test_unknown_slash_command_local_rejection() {
    let (mut app, _) = test_app();
    app.active_tab_mut().editor.insert_str("/reusme");
    let act = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(act, None);
    assert!(!app.active_tab().waiting_for_response);
    assert!(app
        .active_tab()
        .status_text
        .contains("Unknown command: /reusme"));
    assert_eq!(app.active_tab().cache.blocks_count(), 2);
}

#[test]
fn test_mouse_click_collapsible_tool_accordion() {
    let (mut app, _) = test_app();
    app.viewport_bounds = (0, 0, 80, 20);
    let _ = app.active_tab_mut().cache.upsert_tool_call(
        Some(1),
        "run_cmd",
        frontal::protocol::StepState::Done,
        None,
        Some("output line 1\noutput line 2\noutput line 3"),
    );
    let _ = app.active_tab_mut().cache.ensure_visual_lines(80);

    let click_down = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    let click_up = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(click_down);
    app.handle_terminal_event(click_up);

    let expanded_lines = app.active_tab_mut().cache.ensure_visual_lines(80);
    assert!(expanded_lines > 2);

    let click_down2 = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    let click_up2 = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(click_down2);
    app.handle_terminal_event(click_up2);

    let collapsed_lines = app.active_tab_mut().cache.ensure_visual_lines(80);
    assert_eq!(collapsed_lines, 2);
}
