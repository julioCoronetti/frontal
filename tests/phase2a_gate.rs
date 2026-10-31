use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::engine::{kill_registered_subprocesses, register_child_pgid, unregister_child_pgid};
use frontal::protocol::{EngineEvent, StreamEvent};
use frontal::session::SessionManager;
use frontal::transport::ReplayMode;
use frontal::ui::app::{App, AppAction};
use frontal::ui::cache::{wrap_text_utf8, BlockKind, RenderCache};
use frontal::ui::input_stub::MinimalInputStub;
use frontal::ui::viewport::{ConversationViewport, ScrollMode};

#[test]
fn test_render_cache_l1_and_l2_o1_invariance() {
    let mut cache = RenderCache::new();

    cache.push_user_message("user prompt 1");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant response 1");
    cache.consolidate_active_turn(None);

    cache.push_user_message("user prompt 2");
    cache.append_streaming_delta(BlockKind::Assistant, "assistant response 2");
    cache.consolidate_active_turn(None);

    let initial_lines = cache.ensure_visual_lines(80);
    assert!(initial_lines > 0);
    let misses_after_initial = cache.cache_misses();
    assert_eq!(misses_after_initial, 4);

    let lines_second = cache.ensure_visual_lines(80);
    assert_eq!(lines_second, initial_lines);
    assert_eq!(cache.cache_misses(), misses_after_initial);
    assert_eq!(cache.cache_hits(), 4);

    for i in 0..10 {
        cache.append_streaming_delta(BlockKind::Assistant, &format!(" token{}", i));
        cache.ensure_visual_lines(80);
    }

    assert_eq!(cache.cache_misses(), misses_after_initial + 10);
    assert_eq!(cache.cache_hits(), 4 + (4 * 10));
}

#[test]
fn test_render_cache_resize_invalidation_and_subsequent_hits() {
    let mut cache = RenderCache::new();
    cache.push_user_message("First long line that wraps differently across widths");
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "Second long line that will also wrap differently",
    );
    cache.consolidate_active_turn(None);

    let lines_80 = cache.ensure_visual_lines(80);
    let misses_80 = cache.cache_misses();
    assert_eq!(misses_80, 2);

    let lines_40 = cache.ensure_visual_lines(40);
    assert!(lines_40 >= lines_80);
    assert_eq!(cache.cache_misses(), 4);

    let hits_before = cache.cache_hits();
    let lines_40_again = cache.ensure_visual_lines(40);
    assert_eq!(lines_40_again, lines_40);
    assert_eq!(cache.cache_hits(), hits_before + 2);
    assert_eq!(cache.cache_misses(), 4);
}

#[test]
fn test_sticky_scroll_state_machine() {
    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(10, 5);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 5);

    viewport.sync_dimensions(20, 5);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 15);

    viewport.scroll_up(3);
    assert_eq!(viewport.mode(), ScrollMode::Unanchored { offset: 12 });
    assert_eq!(viewport.effective_offset(), 12);

    viewport.sync_dimensions(25, 5);
    assert_eq!(viewport.effective_offset(), 12);

    viewport.scroll_down(3);
    assert_eq!(viewport.effective_offset(), 15);

    viewport.scroll_down(10);
    assert!(viewport.is_anchored());
    assert_eq!(viewport.effective_offset(), 20);
}

#[test]
fn test_utf8_word_wrap_safety() {
    let text = "Olá, Senhor! Testando acentuação e caracteres especiais: coração, ação, 🦀, café.";
    let wrapped_5 = wrap_text_utf8(text, 5);
    for line in &wrapped_5 {
        assert!(line.chars().count() <= 5);
    }

    let wrapped_1 = wrap_text_utf8("ação", 1);
    assert_eq!(wrapped_1.len(), 4);
    assert_eq!(wrapped_1[0], "a");
    assert_eq!(wrapped_1[1], "ç");
    assert_eq!(wrapped_1[2], "ã");
    assert_eq!(wrapped_1[3], "o");
}

#[test]
fn test_minimal_input_stub_editing_and_submit() {
    let mut input = MinimalInputStub::new();
    assert_eq!(input.take_submitted(), None);

    input.insert_char('h');
    input.insert_char('i');
    assert_eq!(input.content(), "hi");
    assert_eq!(input.cursor_char_idx(), 2);

    input.backspace();
    assert_eq!(input.content(), "h");
    assert_eq!(input.cursor_char_idx(), 1);

    input.insert_str("ello world");
    assert_eq!(input.content(), "hello world");
    assert_eq!(input.cursor_char_idx(), 11);

    let submitted = input.take_submitted();
    assert_eq!(submitted.as_deref(), Some("hello world"));
    assert_eq!(input.content(), "");
    assert_eq!(input.cursor_char_idx(), 0);
}

#[tokio::test]
async fn test_app_ui_rendering_and_fixture_flow_with_test_backend() {
    let fixtures = vec![
        r#"{"event":"init","conversation_id":"c-tui"}"#.to_string(),
        r#"{"event":"step_update","step_update":{"conversation_id":"c-tui","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"streamed token "}}"#.to_string(),
        r#"{"event":"result","result":{"conversation_id":"c-tui","status":"SUCCESS","response":"streamed token response"}}"#.to_string(),
    ];

    let mut session_manager = SessionManager::new();
    let session_id = session_manager
        .create_replay_session_from_lines(fixtures, ReplayMode::TurnGated)
        .await
        .unwrap();

    let mut app = App::new(session_manager, session_id);
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
fn test_child_pgid_registry_and_kill_safeguard() {
    register_child_pgid(999999);
    unregister_child_pgid(999999);
    kill_registered_subprocesses();
}

#[test]
fn test_kill_registered_subprocesses_real_termination() {
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("Failed to spawn sleep");
    let pid = child.id() as i32;

    register_child_pgid(pid);
    kill_registered_subprocesses();

    let _ = child.wait();
    unsafe {
        let res = libc::kill(pid, 0);
        assert_eq!(res, -1);
    }
}

#[test]
fn test_viewport_resize_reanchoring() {
    let mut viewport = ConversationViewport::new();
    viewport.sync_dimensions(100, 20);
    assert_eq!(viewport.mode(), ScrollMode::Anchored);

    viewport.scroll_up(30);
    assert!(matches!(viewport.mode(), ScrollMode::Unanchored { .. }));

    viewport.sync_dimensions(25, 20);
    assert_eq!(viewport.mode(), ScrollMode::Anchored);
}

#[test]
fn test_consolidate_active_turn_fallback_response() {
    let mut cache = RenderCache::new();
    cache.push_user_message("hello");
    cache.consolidate_active_turn(Some("fallback text"));
    assert_eq!(cache.blocks_count(), 2);
    let lines = cache.ensure_visual_lines(80);
    assert!(lines > 0);

    let mut cache2 = RenderCache::new();
    cache2.push_user_message("hello");
    cache2.append_streaming_delta(BlockKind::Assistant, "actual response");
    cache2.consolidate_active_turn(Some("fallback text"));
    assert_eq!(cache2.blocks_count(), 2);
}

#[test]
fn test_spinner_visibility_when_total_lines_exceed_height() {
    use frontal::protocol::SessionId;
    let session_manager = SessionManager::new();
    let session_id = SessionId::new();
    let mut app = App::new(session_manager, session_id);

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
