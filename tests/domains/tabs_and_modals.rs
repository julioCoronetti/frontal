use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use frontal::protocol::{
    EngineEvent, ResultData, ResultStatus, SessionEnvelope, SessionId, StepState, StepType,
    StepUpdateData, StepUpdateEvent, StreamEvent, TokenUsage,
};
use frontal::ui::app::AppAction;
use frontal::ui::diff::{DiffLineKind, DiffViewer};
use frontal::ui::modal::{
    CommandPaletteModal, HotRespawnModal, ModalAction, ModelEffortModal, ModelEffortStep,
};

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
fn test_model_effort_selector_two_step_flow() {
    let mut modal = ModelEffortModal::new();
    assert_eq!(modal.selected_model_idx, 0);

    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    modal.handle_key(down);
    assert_eq!(modal.selected_model_idx, 1);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act1 = modal.handle_key(enter);
    assert_eq!(act1, ModalAction::None);
    assert_eq!(modal.step, ModelEffortStep::SelectEffort);
    assert_eq!(modal.chosen_model.as_deref(), Some("gemini-4-argon"));

    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    modal.handle_key(tab);
    assert_eq!(modal.selected_effort_idx, 0);

    let enter2 = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act2 = modal.handle_key(enter2);
    assert_eq!(
        act2,
        ModalAction::SelectModelEffort {
            model: "gemini-4-argon".to_string(),
            effort: "low".to_string(),
        }
    );
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
fn test_model_effort_query_filtering_and_effort_cycling() {
    let mut modal = ModelEffortModal::new();
    assert_eq!(modal.filtered_indices().len(), modal.models.len());

    let char_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    let char_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE);
    modal.handle_key(char_s);
    modal.handle_key(char_o);
    assert_eq!(modal.query, "so");

    let filtered = modal.filtered_indices();
    for &idx in &filtered {
        let m = &modal.models[idx];
        assert!(m.id.contains("so") || m.name.to_lowercase().contains("so"));
    }

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    modal.handle_key(enter);
    assert_eq!(modal.step, ModelEffortStep::SelectEffort);
    let tab2 = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    modal.handle_key(tab2);
    assert_eq!(modal.selected_effort_idx, 0);
    let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
    modal.handle_key(backtab);
    assert_eq!(modal.selected_effort_idx, 0);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let action = modal.handle_key(enter);
    assert!(matches!(
        action,
        ModalAction::SelectModelEffort {
            ref effort,
            ..
        } if effort == "high"
    ));
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
fn test_model_catalog_normalization_and_effort_decoupling() {
    let raw_models = vec![
        (
            "gemini-3.8-flash-high".to_string(),
            "Gemini 3.8 Flash (High)".to_string(),
        ),
        (
            "gemini-3.8-flash-medium".to_string(),
            "Gemini 3.8 Flash (Medium)".to_string(),
        ),
        (
            "gemini-3.8-flash-low".to_string(),
            "Gemini 3.8 Flash (Low)".to_string(),
        ),
        (
            "gpt-oss-120b-medium".to_string(),
            "GPT-OSS 120B (Medium)".to_string(),
        ),
        ("custom-model".to_string(), "Custom Model".to_string()),
    ];

    let mut modal = ModelEffortModal::with_models(raw_models);
    assert_eq!(modal.models.len(), 3);
    assert_eq!(modal.models[0].id, "gemini-3.8-flash");
    assert_eq!(modal.models[0].name, "Gemini 3.8 Flash");
    assert_eq!(
        modal.models[0].supported_efforts,
        vec!["low".to_string(), "medium".to_string(), "high".to_string()]
    );

    assert_eq!(modal.models[1].id, "gpt-oss-120b");
    assert_eq!(modal.models[1].name, "GPT-OSS 120B");
    assert_eq!(
        modal.models[1].supported_efforts,
        vec!["medium".to_string()]
    );

    assert_eq!(modal.models[2].id, "custom-model");
    assert_eq!(modal.models[2].name, "Custom Model");

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act1 = modal.handle_key(enter);
    assert_eq!(act1, ModalAction::None);
    assert_eq!(modal.step, ModelEffortStep::SelectEffort);
    assert_eq!(modal.chosen_model.as_deref(), Some("gemini-3.8-flash"));

    let act_confirm = modal.handle_key(enter);
    assert_eq!(
        act_confirm,
        ModalAction::SelectModelEffort {
            model: "gemini-3.8-flash".to_string(),
            effort: "high".to_string(),
        }
    );

    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let act_back = modal.handle_key(esc);
    assert_eq!(act_back, ModalAction::None);
    assert_eq!(modal.step, ModelEffortStep::SelectModel);

    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    modal.handle_key(down);
    assert_eq!(modal.selected_model_idx, 1);

    modal.handle_key(enter);
    assert_eq!(modal.step, ModelEffortStep::SelectEffort);
    assert_eq!(modal.current_supported_efforts(), &["medium".to_string()]);

    let act2 = modal.handle_key(enter);
    assert_eq!(
        act2,
        ModalAction::SelectModelEffort {
            model: "gpt-oss-120b".to_string(),
            effort: "medium".to_string(),
        }
    );
}
