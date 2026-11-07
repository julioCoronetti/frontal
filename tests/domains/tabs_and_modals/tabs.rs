use crossterm::event::{KeyCode, KeyModifiers};

use frontal::protocol::{
    EngineEvent, ResultData, ResultEvent, ResultStatus, SessionEnvelope, SessionId, StepState,
    StepType, StepUpdateData, StepUpdateEvent, StreamEvent, TokenUsage,
};
use frontal::ui::app::AppAction;

use crate::common::{press_key, test_app};

#[test]
fn test_tab_creation_and_switching_alt_keys() {
    let (mut app, _id1) = test_app();

    assert_eq!(app.tab_count(), 1);
    assert_eq!(app.active_tab_index, 0);

    let id2 = SessionId::new();
    let id3 = SessionId::new();
    app.add_tab(id2.clone(), Some("Second Tab".to_string()));
    app.add_tab(id3.clone(), Some("Third Tab".to_string()));

    assert_eq!(app.tab_count(), 3);
    assert_eq!(app.active_tab_index, 2);

    let alt1 = press_key(KeyCode::Char('1'), KeyModifiers::ALT);
    let act = app.handle_terminal_event(alt1);
    assert_eq!(act, Some(AppAction::SwitchTab(0)));
    assert_eq!(app.active_tab_index, 0);

    let alt2 = press_key(KeyCode::Char('2'), KeyModifiers::ALT);
    let act = app.handle_terminal_event(alt2);
    assert_eq!(act, Some(AppAction::SwitchTab(1)));
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.active_session, id2);

    let alt3 = press_key(KeyCode::Char('3'), KeyModifiers::ALT);
    let act = app.handle_terminal_event(alt3);
    assert_eq!(act, Some(AppAction::SwitchTab(2)));
    assert_eq!(app.active_tab_index, 2);
    assert_eq!(app.active_session, id3);

    let alt9 = press_key(KeyCode::Char('9'), KeyModifiers::ALT);
    let act = app.handle_terminal_event(alt9);
    assert_eq!(act, None);
    assert_eq!(app.active_tab_index, 2);
}

#[test]
fn test_background_tab_streaming_isolation_and_unread() {
    let (mut app, id1) = test_app();

    let id2 = SessionId::new();
    app.add_tab(id2.clone(), Some("Background Session".to_string()));

    app.switch_tab(0);
    assert_eq!(app.active_tab_index, 0);
    assert_eq!(app.active_session, id1);

    let bg_update = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            text_delta: Some("Background text content".to_string()),
            ..Default::default()
        }),
    });

    let env = SessionEnvelope {
        session_id: id2.clone(),
        event: EngineEvent::Stream(Box::new(bg_update)),
    };
    app.handle_engine_envelope(env);

    assert_eq!(app.cache.blocks_count(), 0);
    assert!(app.tabs[1].has_unread);
    assert_eq!(app.tabs[1].cache.blocks_count(), 1);

    app.switch_tab(1);
    assert_eq!(app.active_tab_index, 1);
    assert!(!app.tabs[1].has_unread);
    assert_eq!(app.cache.blocks_count(), 1);
}

#[test]
fn test_background_tab_token_usage_swap_and_isolation() {
    let (mut app, id1) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2.clone(), Some("Second Session".to_string()));

    app.switch_tab(0);
    assert_eq!(app.active_tab_index, 0);
    assert_eq!(app.active_session, id1);

    let usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 42,
        thinking_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 52,
    };
    let bg_update = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            text_delta: Some("Delta with usage".to_string()),
            usage: Some(usage),
            ..Default::default()
        }),
    });

    let env = SessionEnvelope {
        session_id: id2.clone(),
        event: EngineEvent::Stream(Box::new(bg_update)),
    };
    app.handle_engine_envelope(env);

    let bg_result = StreamEvent::Result(ResultEvent {
        conversation_id: None,
        result: Some(ResultData {
            response: Some("Done".to_string()),
            usage: Some(usage),
            ..Default::default()
        }),
    });
    let env_res = SessionEnvelope {
        session_id: id2.clone(),
        event: EngineEvent::Stream(Box::new(bg_result)),
    };
    app.handle_engine_envelope(env_res);

    assert_eq!(app.last_turn_usage, None);
    assert_eq!(app.tabs[1].last_turn_usage, Some(usage));

    app.switch_tab(1);
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.last_turn_usage, Some(usage));

    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    app.editor.insert_str("new prompt");
    app.handle_terminal_event(enter);
    assert_eq!(app.last_turn_usage, None);
}

#[test]
fn test_close_tab_and_tab_lifecycle() {
    let (mut app, id1) = test_app();

    let id2 = SessionId::new();
    let id3 = SessionId::new();
    app.add_tab(id2.clone(), Some("Tab 2".to_string()));
    app.add_tab(id3.clone(), Some("Tab 3".to_string()));
    assert_eq!(app.tab_count(), 3);
    assert_eq!(app.active_tab_index, 2);

    let ctrl_w = press_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
    let act = app.handle_terminal_event(ctrl_w);
    assert_eq!(act, Some(AppAction::CloseTab(2)));

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_tab(2).await;
        assert_eq!(closed, Some(id3));
    });

    assert_eq!(app.tab_count(), 2);
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.active_session, id2);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_tab(0).await;
        assert_eq!(closed, Some(id1));
    });

    assert_eq!(app.tab_count(), 1);
    assert_eq!(app.active_tab_index, 0);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_tab(0).await;
        assert_eq!(closed, None);
    });
    assert_eq!(app.tab_count(), 1);
}

#[test]
fn test_tab_switching_under_rapid_background_events() {
    let (mut app, _id1) = test_app();

    let id2 = SessionId::new();
    let id3 = SessionId::new();
    let id4 = SessionId::new();

    app.add_tab(id2.clone(), Some("Tab 2".to_string()));
    app.add_tab(id3.clone(), Some("Tab 3".to_string()));
    app.add_tab(id4.clone(), Some("Tab 4".to_string()));
    assert_eq!(app.tab_count(), 4);

    app.switch_tab(0);

    for i in 0..5 {
        let update = StreamEvent::StepUpdate(StepUpdateEvent {
            conversation_id: None,
            step_update: Some(StepUpdateData {
                step_index: Some(i),
                text_delta: Some(format!("token {}", i)),
                ..Default::default()
            }),
        });
        app.handle_engine_envelope(SessionEnvelope {
            session_id: id2.clone(),
            event: EngineEvent::Stream(Box::new(update.clone())),
        });
        app.handle_engine_envelope(SessionEnvelope {
            session_id: id3.clone(),
            event: EngineEvent::Stream(Box::new(update)),
        });
    }

    assert_eq!(app.cache.blocks_count(), 0);
    assert!(app.tabs[1].has_unread);
    assert!(app.tabs[2].has_unread);
    assert!(!app.tabs[3].has_unread);

    app.switch_tab(1);
    assert_eq!(app.active_session, id2);
    assert!(!app.tabs[1].has_unread);
    assert_eq!(app.cache.blocks_count(), 1);

    app.switch_tab(2);
    assert_eq!(app.active_session, id3);
    assert!(!app.tabs[2].has_unread);
    assert_eq!(app.cache.blocks_count(), 1);

    app.switch_tab(3);
    assert_eq!(app.active_session, id4);
    assert_eq!(app.cache.blocks_count(), 0);
}

#[test]
fn test_close_active_tab_state_isolation_and_no_contamination() {
    let (mut app, _) = test_app();

    app.cache.push_user_message("msg from tab 1");
    app.editor.insert_str("editor tab 1");

    let id2 = SessionId::new();
    app.add_tab(id2, Some("Tab 2".to_string()));
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.cache.blocks_count(), 0);
    assert_eq!(app.editor.text(), "");

    app.cache.push_user_message("msg from tab 2");
    app.editor.insert_str("editor tab 2");
    assert_eq!(app.cache.blocks_count(), 1);
    assert_eq!(app.editor.text(), "editor tab 2");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_tab(1).await;
        assert!(closed.is_some());
    });

    assert_eq!(app.active_tab_index, 0);
    assert_eq!(app.cache.blocks_count(), 1);
    assert_eq!(app.editor.text(), "editor tab 1");

    let id3 = SessionId::new();
    app.add_tab(id3, Some("Tab 3".to_string()));
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.cache.blocks_count(), 0);
    assert_eq!(app.editor.text(), "");
}

#[test]
fn test_background_tab_protocol_error_and_stderr() {
    let (mut app, _) = test_app();

    let id2 = SessionId::new();
    app.add_tab(id2.clone(), Some("Background Session".to_string()));
    app.switch_tab(0);
    app.tabs[1].waiting_for_response = true;

    app.handle_engine_envelope(SessionEnvelope {
        session_id: id2.clone(),
        event: EngineEvent::ProtocolError {
            message: "Malformed input stream".to_string(),
            raw_preview: None,
        },
    });

    assert!(!app.tabs[1].waiting_for_response);
    assert!(app.tabs[1].has_unread);
    assert_eq!(app.tabs[1].cache.blocks_count(), 1);
    assert_eq!(app.tabs[1].status_text, "Protocol Error");

    app.handle_engine_envelope(SessionEnvelope {
        session_id: id2,
        event: EngineEvent::Stderr("warning: high memory usage".to_string()),
    });

    assert_eq!(
        app.tabs[1].status_text,
        "stderr: warning: high memory usage"
    );
}

#[test]
fn test_popover_closed_on_tab_switch_and_management() {
    let (mut app, _) = test_app();

    let id2 = SessionId::new();
    app.add_tab(id2, Some("Tab 2".to_string()));
    app.switch_tab(0);

    let slash_key = press_key(KeyCode::Char('/'), KeyModifiers::NONE);
    app.handle_terminal_event(slash_key);
    assert!(app.popover.is_visible());

    app.switch_tab(1);
    assert!(!app.popover.is_visible());

    let slash_key2 = press_key(KeyCode::Char('/'), KeyModifiers::NONE);
    app.handle_terminal_event(slash_key2);
    assert!(app.popover.is_visible());

    let id3 = SessionId::new();
    app.add_tab(id3, None);
    assert!(!app.popover.is_visible());
}

#[test]
fn test_background_tab_tool_calls_and_error_lifecycle() {
    let (mut app, _) = test_app();
    let bg_id = SessionId::new();
    app.add_tab(bg_id.clone(), Some("BG Worker".to_string()));
    app.switch_tab(0);

    let tool_active = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            step_type: Some(StepType::Tool),
            state: Some(StepState::Active),
            tool_name: Some("fetch_data".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id.clone(),
        event: EngineEvent::Stream(Box::new(tool_active)),
    });
    assert!(app.tabs[1].has_unread);
    assert_eq!(app.tabs[1].cache.blocks_count(), 1);

    let tool_done = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            step_type: Some(StepType::Tool),
            state: Some(StepState::Done),
            tool_name: Some("fetch_data".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id.clone(),
        event: EngineEvent::Stream(Box::new(tool_done)),
    });
    assert_eq!(app.tabs[1].cache.blocks_count(), 1);

    let thought_ev = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(2),
            step_type: Some(StepType::Thought),
            state: Some(StepState::Active),
            thought: Some("evaluating background payload".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id.clone(),
        event: EngineEvent::Stream(Box::new(thought_ev)),
    });
    assert_eq!(app.tabs[1].cache.blocks_count(), 2);

    let res_ev = StreamEvent::Result(frontal::protocol::ResultEvent {
        conversation_id: None,
        result: Some(ResultData {
            conversation_id: None,
            status: Some(ResultStatus::Success),
            response: Some("done processing".to_string()),
            duration_seconds: Some(0.8),
            num_turns: Some(1),
            usage: None,
            error: None,
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id,
        event: EngineEvent::Stream(Box::new(res_ev)),
    });
    assert_eq!(app.tabs[1].cache.blocks_count(), 3);
}

#[test]
fn test_close_tab_before_active_index_adjusts_active_tab() {
    let (mut app, id1) = test_app();
    let id2 = SessionId::new();
    let id3 = SessionId::new();
    app.add_tab(id2.clone(), Some("Tab 2".to_string()));
    app.add_tab(id3.clone(), Some("Tab 3".to_string()));
    assert_eq!(app.active_tab_index, 2);
    assert_eq!(app.active_session, id3);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_tab(0).await;
        assert_eq!(closed, Some(id1));
    });

    assert_eq!(app.tab_count(), 2);
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.active_session, id3);
}

#[test]
fn test_resume_modal_current_sessions_and_tab_switching() {
    let (mut app, _id1) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2.clone(), Some("Second Session".to_string()));
    assert_eq!(app.active_tab_index, 1);

    app.open_resume_modal(None);
    assert_eq!(app.modals.stack.len(), 1);

    let modal = match app.modals.stack.last().unwrap() {
        frontal::ui::modal::Modal::Select(s) => s,
        _ => panic!("Expected Select modal"),
    };

    assert_eq!(modal.items.len(), 2);
    assert_eq!(modal.items[0].title, "Session 1");
    assert_eq!(modal.items[0].shortcut.as_deref(), Some("ctrl + t"));
    assert_eq!(modal.items[0].category.as_deref(), Some("Current Sessions"));
    assert_eq!(modal.items[1].title, "Second Session");
    assert_eq!(modal.items[1].shortcut.as_deref(), Some("ctrl + t"));

    let enter_key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let modal_action = app.modals.handle_key_event(enter_key);
    assert_eq!(modal_action, frontal::ui::modal::ModalAction::SwitchTab(0));

    let dispatched = app.dispatch_modal_action(modal_action);
    assert_eq!(dispatched, Some(Some(AppAction::SwitchTab(0))));
    assert_eq!(app.active_tab_index, 0);
    assert!(app.modals.is_empty());
}

#[test]
fn test_rename_command_and_title_synchronization() {
    let (mut app, _id1) = test_app();
    app.editor.insert_str("/rename Architectural Refactoring");
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    let action = app.handle_terminal_event(enter);
    assert_eq!(action, None);
    assert_eq!(app.tabs[0].title, "Architectural Refactoring");
    assert_eq!(
        app.status_text,
        "Session renamed to: Architectural Refactoring"
    );

    let sessions = app.history.sessions();
    assert!(!sessions.is_empty());
    assert_eq!(
        sessions[0].title.as_deref(),
        Some("Architectural Refactoring")
    );

    app.editor.insert_str("/rename   ");
    let enter2 = press_key(KeyCode::Enter, KeyModifiers::NONE);
    let _ = app.handle_terminal_event(enter2);
    assert_eq!(app.status_text, "Usage: /rename <title>");
    assert_eq!(app.tabs[0].title, "Architectural Refactoring");

    let clean = frontal::ui::history::clean_session_title(
        "cara seguinte estamos com problema na trc fiserv",
    );
    assert_eq!(clean, "Estamos com problema na trc fiserv");

    let clean_empty = frontal::ui::history::clean_session_title("seguinte:");
    assert_eq!(clean_empty, "New Session");
}
