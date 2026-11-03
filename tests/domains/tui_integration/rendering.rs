use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::protocol::{EngineEvent, SessionId, StepState, StreamEvent};
use frontal::session::SessionManager;
use frontal::ui::app::{App, AppAction};
use frontal::ui::cache::{BlockKind, RenderCache, TurnMeta};
use frontal::ui::modal::{
    CommandPaletteModal, DiffModal, HotRespawnModal, Modal, ModelEffortModal,
};
use frontal::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BG_USER_CONTAINER, BORDER_SUBTLE,
    TAB_ACTIVE_DOT, TEXT_MUTED, TEXT_PRIMARY,
};
use ratatui::style::Color;

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

    app.editor.insert_str("ola");
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

    for i in 0..20 {
        app.cache.push_user_message(&format!("user line {}", i));
    }
    app.cache.consolidate_active_turn(None);
    app.waiting_for_response = true;

    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    let rendered = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    let spinner_symbols = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let contains_spinner = spinner_symbols.iter().any(|&s| rendered.contains(s));
    assert!(contains_spinner);

    let initial_blocks = app.cache.blocks_count();
    let slash_not_cmd = Event::Key(KeyEvent {
        code: KeyCode::Char('a'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    let act = app.handle_terminal_event(slash_not_cmd);
    assert_eq!(act, None);
    assert_eq!(app.cache.blocks_count(), initial_blocks);
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
    assert!(rendered.contains("gemini-3.8-flash-high"));
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
    assert!(visual_lines.len() >= 5);
    let assistant_first_line = &visual_lines[4];
    let assistant_text: String = assistant_first_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(assistant_text.contains("Hello from assistant response"));
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
fn test_welcome_state_sidebar_and_scroll_down_indicator() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(120, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    let text = buf
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("frontal"));
    assert!(text.contains("Async TUI for Antigravity"));

    for i in 0..30 {
        app.cache.push_user_message(&format!("user prompt {}", i));
    }
    app.last_turn_usage = Some(frontal::protocol::TokenUsage {
        input_tokens: 500,
        output_tokens: 200,
        thinking_tokens: 50,
        cache_read_tokens: 100,
        total_tokens: 700,
    });
    terminal.draw(|f| app.render(f)).unwrap();
    let buf2 = terminal.backend().buffer();
    let text2 = buf2
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(text2.contains("Context Window"));
    assert!(text2.contains("700 tok"));
    assert!(text2.contains("Session Active"));

    app.viewport.scroll_up(10);
    terminal.draw(|f| app.render(f)).unwrap();
    let buf3 = terminal.backend().buffer();
    let text3 = buf3
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(text3.contains("Scroll to bottom"));
}

#[test]
fn test_atlas_crimson_theme_constants_and_rendering() {
    assert_eq!(BG_USER_CONTAINER, Color::Rgb(24, 15, 28));
    assert_eq!(ACCENT_PRIMARY, Color::Rgb(177, 12, 41));
    assert_eq!(ACCENT_HIGHLIGHT, Color::Rgb(255, 59, 80));
    assert_eq!(ACCENT_RUBY, Color::Rgb(134, 16, 44));
    assert_eq!(BORDER_SUBTLE, Color::Rgb(72, 11, 32));
    assert_eq!(TEXT_PRIMARY, Color::Rgb(255, 255, 255));
    assert_eq!(TEXT_MUTED, Color::Rgb(140, 125, 136));
    assert_eq!(TAB_ACTIVE_DOT, Color::Rgb(177, 12, 41));

    let (mut app, _) = test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();

    let mut found_logo_accent = false;
    let mut found_help_highlight = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).unwrap();
            if cell.symbol() == "f" && cell.fg == ACCENT_PRIMARY {
                found_logo_accent = true;
            }
            if cell.symbol() == "/" && cell.fg == ACCENT_HIGHLIGHT {
                found_help_highlight = true;
            }
        }
    }
    assert!(found_logo_accent);
    assert!(found_help_highlight);

    for i in 0..20 {
        app.cache.push_user_message(&format!("message {}", i));
    }
    terminal.draw(|f| app.render(f)).unwrap();
    app.viewport.scroll_up(5);
    terminal.draw(|f| app.render(f)).unwrap();
    let buf2 = terminal.backend().buffer();

    let mut found_indicator = false;
    for y in 0..buf2.area.height {
        for x in 0..buf2.area.width {
            let cell = buf2.cell((x, y)).unwrap();
            if cell.symbol() == "▼" && cell.bg == ACCENT_PRIMARY && cell.fg == TEXT_PRIMARY {
                found_indicator = true;
                break;
            }
        }
        if found_indicator {
            break;
        }
    }
    assert!(found_indicator);
}
