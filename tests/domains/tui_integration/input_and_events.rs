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

    app.editor.insert_str("/new");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, Some(AppAction::NewTab));

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
    assert!(!app.modals.is_empty());
    assert!(matches!(
        app.modals.top(),
        Some(frontal::ui::modal::Modal::ModelEffort(m)) if m.query == "gemini"
    ));

    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);
    app.handle_terminal_event(esc);
    assert!(app.modals.is_empty());

    let _ = app.cache.upsert_tool_call(
        Some(1),
        "test_tool",
        frontal::protocol::StepState::Done,
        None,
        Some("output line 1\noutput line 2\noutput line 3"),
    );
    let collapsed_base = app.cache.ensure_visual_lines(80);
    assert!(collapsed_base > 1);

    let backtab = press_key(KeyCode::BackTab, KeyModifiers::NONE);
    let act = app.handle_terminal_event(backtab.clone());
    assert_eq!(act, None);

    let expanded_lines = app.cache.ensure_visual_lines(80);
    assert!(expanded_lines > collapsed_base);

    let act2 = app.handle_terminal_event(backtab);
    assert_eq!(act2, None);

    let re_collapsed_lines = app.cache.ensure_visual_lines(80);
    assert_eq!(re_collapsed_lines, collapsed_base);

    let slice = app.cache.visible_slice(0, re_collapsed_lines);
    let collapsed_text: String = slice
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(collapsed_text.contains("test_tool"));
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
    assert!(!app.waiting_for_response);
}

#[test]
fn test_slash_command_autocomplete_two_columns_navigation_and_apply() {
    let (mut app, _) = test_app();

    app.handle_terminal_event(press_key(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(app.popover.is_visible());
    assert!(!app.popover.items.is_empty());

    let first = &app.popover.items[0];
    assert!(first.description.is_some());
    assert_eq!(first.label, "/help");

    app.handle_terminal_event(press_key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.popover.selected_index(), 1);

    let second = app.popover.selected_item().unwrap();
    assert_eq!(second.label, "/clear");

    app.handle_terminal_event(press_key(KeyCode::Tab, KeyModifiers::NONE));
    assert!(!app.popover.is_visible());
    assert_eq!(app.editor.text(), "/clear ");
}

#[tokio::test]
async fn test_last_turn_usage_persisted_through_result_and_subsequent_prompt() {
    let (mut app, _) = test_app();

    let usage = frontal::protocol::TokenUsage {
        input_tokens: 120,
        output_tokens: 80,
        thinking_tokens: 20,
        cache_read_tokens: 30,
        total_tokens: 250,
    };
    let result_data = frontal::protocol::ResultData {
        conversation_id: Some("conv-1".to_string()),
        status: Some(frontal::protocol::ResultStatus::Success),
        response: Some("final answer".to_string()),
        error: None,
        duration_seconds: Some(1.2),
        num_turns: Some(1),
        usage: Some(usage),
    };
    let result_ev = frontal::protocol::StreamEvent::Result(frontal::protocol::ResultEvent {
        conversation_id: Some("conv-1".to_string()),
        result: Some(result_data),
    });
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: app.active_session.clone(),
        event: frontal::protocol::EngineEvent::Stream(Box::new(result_ev)),
    });

    assert_eq!(
        app.last_turn_usage.as_ref().map(|u| u.total_tokens),
        Some(250)
    );

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let backend = TestBackend::new(120, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("250 tok"));

    app.editor.insert_str("/help");
    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.last_turn_usage.as_ref().map(|u| u.total_tokens),
        Some(250)
    );
}

#[test]
fn test_autocomplete_popover_scrolling_window_for_large_item_lists() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut app, _) = test_app();
    let files: Vec<String> = (0..15).map(|i| format!("file_{:02}.rs", i)).collect();
    app.search_worker.set_cached_files(files);

    app.handle_terminal_event(press_key(KeyCode::Char('@'), KeyModifiers::NONE));
    assert!(app.popover.is_visible());
    assert_eq!(app.popover.items.len(), 15);

    for _ in 0..10 {
        app.handle_terminal_event(press_key(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(app.popover.selected_index(), 10);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(rendered.contains("file_10.rs"));
}

#[test]
fn test_resume_command_opens_dialog_select_and_filters_query() {
    let (mut app, _) = test_app();
    app.history
        .record_session(frontal::ui::history::SessionRecord {
            id: "s1".to_string(),
            conversation_id: Some("conv-111".to_string()),
            model: Some("gemini-3.8-flash-high".to_string()),
            mode: Some("auto".to_string()),
            effort: Some("high".to_string()),
            title: Some("Fix backend parsing".to_string()),
            created_at: 0,
        });
    app.history
        .record_session(frontal::ui::history::SessionRecord {
            id: "s2".to_string(),
            conversation_id: Some("conv-222".to_string()),
            model: Some("claude-sonnet".to_string()),
            mode: Some("plan".to_string()),
            effort: Some("low".to_string()),
            title: Some("Refactor UI cache".to_string()),
            created_at: 0,
        });

    app.editor.insert_str("/resume backend");
    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.modals.stack.len(), 1);

    if let frontal::ui::modal::Modal::Select(ref select) = app.modals.top().unwrap() {
        assert_eq!(select.query, "backend");
        assert_eq!(select.filtered_indices().len(), 1);
        let selected = select.selected_item().unwrap();
        assert_eq!(selected.id, "conv-111");
    } else {
        panic!("Expected Modal::Select");
    }
}

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

    app.editor.insert_str("pare");
    let act = app.handle_terminal_event(enter);
    assert_eq!(act, None);
    assert_eq!(app.queued_prompts.len(), 1);
    assert_eq!(app.queued_prompts[0], "pare");
    assert!(app.status_text.contains("Queued (1 pending)"));
    assert_eq!(app.cache.blocks_count(), 1);

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

    assert_eq!(app.queued_prompts.len(), 0);
    assert_eq!(app.pending_engine_prompts.len(), 1);
    assert_eq!(app.pending_engine_prompts[0], (sid, "pare".to_string()));
    assert_eq!(app.cache.blocks_count(), 2);
    assert!(app.waiting_for_response);
}

#[test]
fn test_unknown_slash_command_local_rejection() {
    let (mut app, _) = test_app();
    app.editor.insert_str("/reusme");
    let act = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(act, None);
    assert!(!app.waiting_for_response);
    assert!(app.status_text.contains("Unknown command: /reusme"));
    assert_eq!(app.cache.blocks_count(), 2);
}

#[test]
fn test_mouse_click_collapsible_tool_accordion() {
    let (mut app, _) = test_app();
    app.viewport_bounds = (0, 0, 80, 20);
    let _ = app.cache.upsert_tool_call(
        Some(1),
        "run_cmd",
        frontal::protocol::StepState::Done,
        None,
        Some("output line 1\noutput line 2\noutput line 3"),
    );
    let _ = app.cache.ensure_visual_lines(80);

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

    let expanded_lines = app.cache.ensure_visual_lines(80);
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

    let collapsed_lines = app.cache.ensure_visual_lines(80);
    assert_eq!(collapsed_lines, 2);
}
