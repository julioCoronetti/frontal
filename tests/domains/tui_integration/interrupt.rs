use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use frontal::protocol::{EngineEvent, PromptPayload, ResultEvent, StreamEvent};
use frontal::ui::app::AppAction;
use frontal::ui::cache::BlockKind;
use frontal::ui::tab::INTERRUPT_ARM_TIMEOUT;
use frontal::ui::theme::{TEXT_INTERRUPT_HINT, TEXT_MUTED};

use crate::common::{press_key, render_app, test_app};

#[test]
fn test_double_esc_arms_confirms_and_clears_active_turn() {
    let (mut app, _) = test_app();
    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);

    app.active_tab_mut()
        .cache
        .push_user_message("run long task");
    app.active_tab_mut().waiting_for_response = true;
    app.active_tab_mut().status_text = String::from("Thinking...");
    app.active_tab_mut()
        .queued_prompts
        .push_back(PromptPayload::from_text("queued follow-up"));

    let first = app.handle_terminal_event(esc.clone());
    assert_eq!(first, None);
    assert!(app.active_tab().is_interrupt_armed());
    assert!(app.active_tab().waiting_for_response);
    assert_eq!(app.active_tab().status_text, "Thinking...");
    assert_eq!(app.active_tab().queued_prompts.len(), 1);

    let second = app.handle_terminal_event(esc);
    assert_eq!(second, Some(AppAction::Interrupt));
    assert!(!app.active_tab().is_interrupt_armed());
    assert!(!app.active_tab().waiting_for_response);
    assert!(app.active_tab().queued_prompts.is_empty());
    assert!(!app.is_turn_active());
    assert_eq!(app.active_tab().status_text, "Interrupted by user");

    let (term, rendered) = render_app(&mut app, 80, 24);
    assert!(rendered.contains("  Prompt interrupted"));
    let buf = term.backend().buffer();
    let found_muted = (0..buf.area.height).any(|y| {
        let row: String = (0..buf.area.width)
            .map(|x| buf.cell((x, y)).unwrap().symbol())
            .collect();
        row.contains("  Prompt interrupted")
            && (0..buf.area.width).any(|x| {
                let cell = buf.cell((x, y)).unwrap();
                cell.symbol() == "P" && cell.fg == TEXT_MUTED
            })
    });
    assert!(found_muted);

    app.active_tab_mut()
        .cache
        .append_streaming_delta(BlockKind::Assistant, "partial streamed answer");
    assert_eq!(
        app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE)),
        Some(AppAction::Interrupt)
    );
    let blocks = app.active_tab().cache.blocks();
    assert_eq!(
        blocks.last().map(|b| &b.kind),
        Some(&BlockKind::Interrupted)
    );
    assert!(blocks.iter().all(|b| b.consolidated));
    let (_, rendered_mid) = render_app(&mut app, 80, 24);
    assert!(rendered_mid.contains("partial streamed answer"));
    assert!(rendered_mid.contains("  Prompt interrupted"));
}

#[test]
fn test_double_esc_disarmed_by_intervening_key_and_timeout_expiry() {
    let (mut app, _) = test_app();
    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);

    app.active_tab_mut().waiting_for_response = true;
    assert_eq!(app.handle_terminal_event(esc.clone()), None);
    assert!(app.active_tab().is_interrupt_armed());

    app.handle_terminal_event(press_key(KeyCode::Left, KeyModifiers::NONE));
    assert!(!app.active_tab().is_interrupt_armed());

    assert_eq!(app.handle_terminal_event(esc.clone()), None);
    assert!(app.active_tab().is_interrupt_armed());

    app.active_tab_mut().esc_armed_at =
        Some(Instant::now() - INTERRUPT_ARM_TIMEOUT - Duration::from_millis(50));
    assert!(!app.active_tab().is_interrupt_armed());
    assert!(app.expire_interrupt_timers());
    assert!(app.active_tab().esc_armed_at.is_none());

    assert_eq!(app.handle_terminal_event(esc), None);
    assert!(app.active_tab().is_interrupt_armed());
}

#[test]
fn test_esc_precedence_and_natural_turn_completion_clear_arm() {
    let (mut app, _) = test_app();
    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);

    app.active_tab_mut().waiting_for_response = true;
    app.handle_terminal_event(press_key(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(app.popover.is_visible());

    assert_eq!(app.handle_terminal_event(esc.clone()), None);
    assert!(!app.popover.is_visible());
    assert!(!app.active_tab().is_interrupt_armed());

    app.active_tab_mut().editor.select_all();
    assert!(app.active_tab().editor.has_selection());
    assert_eq!(app.handle_terminal_event(esc.clone()), None);
    assert!(!app.active_tab().editor.has_selection());
    assert!(!app.active_tab().is_interrupt_armed());

    assert_eq!(app.handle_terminal_event(esc), None);
    assert!(app.active_tab().is_interrupt_armed());

    let res_event = StreamEvent::Result(ResultEvent {
        conversation_id: None,
        result: None,
    });
    let sid = app.active_session_id().clone();
    app.handle_engine_envelope(frontal::protocol::SessionEnvelope {
        session_id: sid,
        event: EngineEvent::Stream(Box::new(res_event)),
    });
    assert!(!app.active_tab().is_interrupt_armed());
}

#[test]
fn test_thinking_line_renders_interrupt_hint_and_width_fallbacks() {
    let (mut app, _) = test_app();
    app.active_tab_mut().cache.push_user_message("hello");
    app.active_tab_mut().cache.consolidate_active_turn(None);
    app.active_tab_mut().waiting_for_response = true;

    let (_, text_unarmed) = render_app(&mut app, 80, 24);
    assert!(text_unarmed.contains("Thinking..."));
    assert!(!text_unarmed.contains("esc again to interrupt"));

    app.active_tab_mut().esc_armed_at = Some(Instant::now());
    let (term_armed, text_armed) = render_app(&mut app, 80, 24);
    assert!(text_armed.contains("Thinking..."));
    assert!(text_armed.contains("esc again to interrupt"));

    let buf = term_armed.backend().buffer();
    let found_hint_color = (0..buf.area.height).any(|y| {
        (0..buf.area.width).any(|x| {
            let cell = buf.cell((x, y)).unwrap();
            cell.symbol() == "e" && cell.fg == TEXT_INTERRUPT_HINT
        })
    });
    assert!(found_hint_color);

    app.active_tab_mut().waiting_for_response = false;
    app.active_tab_mut()
        .cache
        .append_streaming_delta(BlockKind::Assistant, "streaming delta");
    let (_, text_streaming_armed) = render_app(&mut app, 80, 24);
    assert!(text_streaming_armed.contains("Thinking..."));
    assert!(text_streaming_armed.contains("esc again to interrupt"));

    let (_, text_medium) = render_app(&mut app, 34, 24);
    assert!(text_medium.contains("esc again to interrupt"));
    assert!(!text_medium.contains("Thinking..."));

    let (_, text_narrow) = render_app(&mut app, 22, 24);
    assert!(text_narrow.contains("esc×2"));
}
