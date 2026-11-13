use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use frontal::protocol::TokenUsage;
use frontal::ui::diff::{DiffLineKind, DiffViewer};
use frontal::ui::modal::{CommandPaletteModal, HotRespawnModal, ModalAction, ModelEffortModal};
use frontal::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_PRIMARY};

use crate::common::{press_key, test_app};

#[test]
fn test_modal_stack_push_pop_and_exclusive_focus() {
    let (mut app, _) = test_app();
    assert!(app.modals.is_empty());

    let act = app.handle_terminal_event(press_key(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(act, None);
    assert!(!app.modals.is_empty());

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(app.editor.is_empty());

    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.modals.is_empty());

    app.handle_terminal_event(press_key(KeyCode::Char('b'), KeyModifiers::NONE));
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
    for c in ['d', 'i', 'f'] {
        palette.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
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
    app.context_window_hint = Some(1_000_000);
    app.served_model = Some("gemini-4-argon-high".to_string());
    app.in_flight_usage = Some(TokenUsage::default());

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
        assert_eq!(app.context_window_hint, None);
        assert_eq!(app.served_model, None);
        assert_eq!(app.in_flight_usage, None);
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
    assert_eq!(cell_outside.bg, Color::Reset);

    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            assert_ne!(buf[(x, y)].bg, Color::Black);
        }
    }

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

    let mut effort_modal =
        ModelEffortModal::with_models(vec![("m-1".to_string(), "Model One".to_string())]);
    effort_modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    terminal
        .draw(|f| effort_modal.render(f, ratatui::layout::Rect::new(10, 2, 60, 20)))
        .unwrap();
    let buf3 = terminal.backend().buffer();
    let mut found_cursor = false;
    let mut found_node = false;
    for y in 0..buf3.area.height {
        for x in 0..buf3.area.width {
            let cell = buf3.cell((x, y)).unwrap();
            if cell.symbol() == "█" && cell.fg == ACCENT_HIGHLIGHT {
                found_cursor = true;
            }
            if cell.symbol() == "◎" && cell.fg == ACCENT_HIGHLIGHT {
                found_node = true;
            }
        }
    }
    assert!(found_cursor);
    assert!(found_node);
}

fn make_tool_event(cmd: &str) -> frontal::protocol::StreamEvent {
    use frontal::protocol::{
        StepState, StepType, StepUpdateData, StepUpdateEvent, StreamEvent, ToolInfo,
    };
    StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            step_type: Some(StepType::Tool),
            state: Some(StepState::Active),
            tool_name: Some("run_command".to_string()),
            tool_info: Some(ToolInfo {
                name: Some("run_command".to_string()),
                parameters: Some(serde_json::json!({ "CommandLine": cmd })),
                output: None,
            }),
            ..Default::default()
        }),
    })
}

#[test]
fn test_security_risk_matrix_classification_and_zero_false_positives() {
    use frontal::security::{evaluate_command, RiskCategory};

    let cases = [
        ("rm -rf /tmp/test", RiskCategory::RecursiveDeletion),
        ("rm -r src/", RiskCategory::RecursiveDeletion),
        ("rm -fr target/", RiskCategory::RecursiveDeletion),
        ("rmdir old_dir", RiskCategory::RecursiveDeletion),
        ("bash -c \"rm -rf /build\"", RiskCategory::RecursiveDeletion),
        ("sudo rm -rf /var/cache", RiskCategory::RecursiveDeletion),
        ("git reset --hard HEAD~1", RiskCategory::DestructiveVcs),
        ("git clean -fd", RiskCategory::DestructiveVcs),
        ("git push origin main -f", RiskCategory::DestructiveVcs),
        ("git branch -D feat-broken", RiskCategory::DestructiveVcs),
        (
            "git -C /dir reset --hard HEAD",
            RiskCategory::DestructiveVcs,
        ),
        ("git -c k=v clean -fd", RiskCategory::DestructiveVcs),
        ("psql -c 'DROP TABLE u;'", RiskCategory::DestructiveSql),
        ("sqlite3 db 'DROP TABLE u;'", RiskCategory::DestructiveSql),
        (
            "dd if=/dev/zero of=/dev/sda",
            RiskCategory::DiskManipulation,
        ),
        ("mkfs.ext4 /dev/sdb1", RiskCategory::DiskManipulation),
        ("shred -u /dev/sdb", RiskCategory::DiskManipulation),
        ("DROP TABLE users;", RiskCategory::DestructiveSql),
        ("DROP DATABASE production;", RiskCategory::DestructiveSql),
        ("TRUNCATE TABLE session_logs;", RiskCategory::DestructiveSql),
    ];
    for (cmd, cat) in cases {
        assert_eq!(evaluate_command(cmd).unwrap().category, cat);
    }

    let safe = [
        "cat rm -rf.txt",
        "grep -rn \"rm -rf\" .",
        "git status",
        "git diff",
        "git log -n 5",
        "echo \"DROP TABLE users\"",
        "echo DROP TABLE users",
        "psql -c \"SELECT * FROM users;\"",
        "SELECT * FROM users WHERE drop_count > 0",
        "padding hidden add",
    ];
    for cmd in safe {
        assert!(evaluate_command(cmd).is_none());
    }

    assert!(frontal::security::evaluate_tool_call(
        "write_file",
        Some(1),
        Some(&serde_json::json!({"Path": "a.sh", "Content": "rm -rf /tmp/x"})),
        None,
    )
    .is_none());
}

#[test]
fn test_danger_tool_call_interception_opens_modal_and_suspends_stream() {
    use frontal::protocol::{
        EngineEvent, SessionEnvelope, StepUpdateData, StepUpdateEvent, StreamEvent,
    };
    let (mut app, _) = test_app();

    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(make_tool_event("rm -rf /tmp/dangerous"))),
    });

    assert!(app.pending_danger.is_some());
    assert!(!app.modals.is_empty());
    assert_eq!(app.status_text, "Security confirmation required");

    let text_ev = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            text_delta: Some("Streaming after danger".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(text_ev)),
    });

    assert_eq!(app.suspended_events.len(), 1);
    assert_eq!(app.cache.blocks_count(), 1);

    app.add_tab(frontal::protocol::SessionId::new(), None);
    assert!(app.modals.is_empty());
    assert!(app.pending_danger.is_none());
    app.switch_tab(0);
    assert!(!app.modals.is_empty());
    assert!(app.pending_danger.is_some());
}

#[test]
fn test_danger_modal_approve_resumes_suspended_events() {
    use frontal::protocol::{
        EngineEvent, SessionEnvelope, StepUpdateData, StepUpdateEvent, StreamEvent,
    };
    let (mut app, _) = test_app();

    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(make_tool_event("git reset --hard HEAD"))),
    });

    let next_ev = StreamEvent::StepUpdate(StepUpdateEvent {
        conversation_id: None,
        step_update: Some(StepUpdateData {
            step_index: Some(1),
            text_delta: Some("Execution resumed".to_string()),
            ..Default::default()
        }),
    });
    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(next_ev)),
    });
    assert_eq!(app.suspended_events.len(), 1);

    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.modals.is_empty());
    assert!(app.pending_danger.is_none());
    assert!(app.suspended_events.is_empty());
    assert_eq!(app.status_text, "Streaming...");
}

#[test]
fn test_danger_modal_reject_aborts_command_and_records_security_block() {
    use frontal::protocol::{EngineEvent, SessionEnvelope};
    let (mut app, _) = test_app();

    app.handle_engine_envelope(SessionEnvelope {
        session_id: app.active_session.clone(),
        event: EngineEvent::Stream(Box::new(make_tool_event("DROP TABLE users;"))),
    });

    app.handle_terminal_event(press_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.modals.is_empty());
    assert!(app.pending_danger.is_none());
    assert_eq!(app.status_text, "Blocked destructive command");
    assert!(!app.waiting_for_response);
}
