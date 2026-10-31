use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::protocol::{EngineEvent, SessionId, StepState, StreamEvent};
use frontal::session::{SessionManager, SessionStatus};
use frontal::ui::app::{App, AppAction};
use frontal::ui::cache::{BlockKind, RenderCache, TurnMeta};
use frontal::ui::modal::{
    CommandPaletteModal, DiffModal, HotRespawnModal, Modal, ModelEffortModal,
};
use frontal::ui::theme::BG_USER_CONTAINER;

use crate::common::{press_key, test_app};

#[tokio::test]
async fn test_app_ui_rendering_and_fixture_flow_with_test_backend() {
    let fixtures = vec![
        r#"{"event":"init","conversation_id":"c-tui"}"#.to_string(),
        r#"{"event":"step_update","step_update":{"conversation_id":"c-tui","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"streamed token "}}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"c-tui","status":"SUCCESS","response":"streamed token response"}}"#.to_string(),
    ];

    let mut session_manager = SessionManager::new();
    let session_id = session_manager
        .create_replay_session_from_lines(fixtures, frontal::transport::ReplayMode::TurnGated)
        .await
        .unwrap();

    let mut app = App::new_isolated(session_manager, session_id);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| app.render(f)).unwrap();

    app.input.insert_str("ola");
    let enter_event = Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    let action = app.handle_terminal_event(enter_event);
    assert_eq!(action, Some(AppAction::SubmitPrompt("ola".to_string())));

    app.session_manager
        .send_prompt(&app.active_session, "ola")
        .await
        .unwrap();

    while let Some(env) = app.session_manager.recv().await {
        let is_res = matches!(env.event, EngineEvent::Stream(ref s) if matches!(**s, StreamEvent::Result(_)));
        app.handle_engine_envelope(env);
        if is_res {
            break;
        }
    }

    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let buffer_str = format!("{:?}", buffer);
    assert!(buffer_str.contains("ola"));
    assert!(buffer_str.contains("streamed token"));
}

#[test]
fn test_spinner_visibility_when_total_lines_exceed_height() {
    let (mut app, _) = test_app();

    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    for i in 0..15 {
        app.cache.push_user_message(&format!("message line {}", i));
    }
    app.cache.consolidate_active_turn(None);
    app.waiting_for_response = true;
    app.spinner_tick = 4;

    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered.contains("Thinking / Waiting for response..."));
}

#[tokio::test]
async fn test_phase3_ui_refinements_on_test_backend() {
    let (mut app, _) = test_app();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut cache = RenderCache::new();
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "# Titulo Primario\n## Subtitulo Secundario\nTexto normal aqui.",
    );
    cache.consolidate_active_turn(None);
    app.cache = cache;

    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let rendered_text = buffer
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(!rendered_text.contains("# Titulo"));
    assert!(!rendered_text.contains("## Subtitulo"));
    assert!(rendered_text.contains("Titulo Primario"));
    assert!(rendered_text.contains("Subtitulo Secundario"));

    let initial_blocks = app.cache.blocks_count();
    app.waiting_for_response = true;
    app.spinner_tick = 8;
    terminal.draw(|f| app.render(f)).unwrap();
    let buffer2 = terminal.backend().buffer();
    let text2 = buffer2
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(text2.contains("Thinking / Waiting for response..."));
    assert_eq!(app.cache.blocks_count(), initial_blocks);

    let down_event = press_key(KeyCode::Down, KeyModifiers::NONE);
    app.handle_terminal_event(down_event);
    assert_eq!(app.cache.blocks_count(), initial_blocks);
}

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
fn test_phase4_ui_rendering_on_test_backend() {
    let (mut app, _) = test_app();

    let id2 = SessionId::new();
    app.add_tab(id2, Some("Secondary".to_string()));
    app.tabs[1].has_unread = true;
    app.switch_tab(0);

    let backend = TestBackend::new(100, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    let rendered = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered.contains("Session 1"));
    assert!(rendered.contains("Secondary*"));
    assert!(rendered.contains("Ctrl+P: Palette"));

    app.modals
        .push(Modal::CommandPalette(CommandPaletteModal::new()));
    terminal.draw(|f| app.render(f)).unwrap();
    let buf_modal = terminal.backend().buffer();
    let rendered_modal = buf_modal
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered_modal.contains("Command Palette"));
    assert!(rendered_modal.contains("New Session Tab"));

    app.modals.pop();
    app.modals.push(Modal::Diff(DiffModal::new(
        "diff --git a/test b/test\n+new line\n-old line\n",
    )));
    terminal.draw(|f| app.render(f)).unwrap();
    let buf_diff = terminal.backend().buffer();
    let rendered_diff = buf_diff
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered_diff.contains("Integrated Diff Viewer"));
    assert!(rendered_diff.contains("+new line"));
}

#[test]
fn test_footer_cwd_and_active_model_header() {
    let (mut app, _) = test_app();

    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf = terminal.backend().buffer();
    let rendered = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered.contains(&app.cwd_display));
    assert!(rendered.contains("gemini-4-argon-high"));
    assert!(rendered.contains("Ctrl+P: Palette"));
}

#[test]
fn test_opencode_visual_layout_and_symmetry() {
    let (mut app, _) = test_app();

    app.cache.push_user_message("Hello from user prompt");
    app.cache.append_streaming_delta(
        BlockKind::Assistant,
        "Hello from assistant response\nSecond line",
    );
    let meta = TurnMeta {
        mode: "auto".to_string(),
        model: "gemini-4-argon-high".to_string(),
        duration_seconds: Some(1.5),
        output_tokens: Some(300),
    };
    app.cache
        .consolidate_active_turn_with_meta(None, Some(meta));

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf = terminal.backend().buffer();
    let rendered_text = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(rendered_text.contains('●'));
    assert!(rendered_text.contains("Session 1"));
    assert!(rendered_text.contains(&app.cwd_display));
    assert!(rendered_text.contains("Ctrl+P: Palette"));
    assert!(!rendered_text.contains("#hex"));
    assert!(!rendered_text.contains("/model"));
    assert!(rendered_text.contains("▣ auto · gemini-4-argon-high · 1.5s · 200.0 tok/s"));

    let mut found_user_bg = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).unwrap();
            if cell.bg == BG_USER_CONTAINER {
                found_user_bg = true;
                break;
            }
        }
        if found_user_bg {
            break;
        }
    }
    assert!(found_user_bg);

    let visual_lines = app.cache.visible_slice(0, 20);
    assert!(visual_lines.len() >= 3);
    let assistant_first_line = &visual_lines[2];
    assert_eq!(assistant_first_line.spans[0].content.as_ref(), "   ");
}

#[test]
fn test_adversarial_terminal_dimensions_no_panic() {
    let (mut app, _) = test_app();

    app.cache
        .push_user_message("Long user message to test wrapping in narrow dimensions");
    app.cache.append_streaming_delta(
        BlockKind::Assistant,
        "Assistant multi-line response\n```rust\nlet x = 10;\n```\n| col1 | col2 |\n|---|---|\n| a | b |",
    );
    app.cache.consolidate_active_turn(None);
    app.cache
        .append_streaming_delta(BlockKind::Thought, "Thought text for extreme dimensions");
    app.cache.consolidate_active_turn(None);
    app.cache.upsert_tool_call(
        Some(1),
        "write_file",
        StepState::Active,
        Some("{\"path\":\"src/lib.rs\"}"),
        None,
    );

    let adversarial_sizes = [(100, 30), (39, 12), (20, 8), (10, 5), (1, 1)];

    for &(w, h) in &adversarial_sizes {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        let draw_res = terminal.draw(|f| app.render(f));
        assert!(draw_res.is_ok());
    }

    let model_modal = ModelEffortModal::new();
    let cmd_modal = CommandPaletteModal::new();
    let respawn_modal = HotRespawnModal::new(None, None, None, None);
    let diff_modal = DiffModal::new("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new");

    for &(w, h) in &[(20, 8), (1, 1)] {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| model_modal.render(f, f.area())).unwrap();
        terminal.draw(|f| cmd_modal.render(f, f.area())).unwrap();
        terminal
            .draw(|f| respawn_modal.render(f, f.area()))
            .unwrap();
        terminal.draw(|f| diff_modal.render(f, f.area())).unwrap();
    }
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
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_down);

    let mouse_drag = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 15,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_drag);
    assert!(app.viewport.selected_text(&app.cache).is_some());

    let mouse_up = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 15,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_terminal_event(mouse_up);
    assert!(app.viewport.selected_text(&app.cache).is_none());
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
    let act_idle = app.handle_terminal_event(ctrl_c.clone());
    assert_eq!(act_idle, Some(AppAction::Quit));
    assert!(app.should_quit);

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
