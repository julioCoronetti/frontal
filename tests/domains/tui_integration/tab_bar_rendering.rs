use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use frontal::protocol::{
    EngineEvent, InitEvent, SessionEnvelope, SessionId, StepUpdateData, StepUpdateEvent,
    StreamEvent,
};
use frontal::ui::app::HitAction;
use frontal::ui::theme::TAB_ACTIVE_BG;
use ratatui::style::Color;

use crate::common::{render_app, test_app};

fn row_zero_string(buf: &ratatui::buffer::Buffer) -> String {
    (0..buf.area.width)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn find_cell_x(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<u16> {
    let row: Vec<&str> = (0..buf.area.width)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    let target: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
    row.windows(target.len())
        .position(|w| w.iter().zip(target.iter()).all(|(a, b)| *a == b.as_str()))
        .map(|idx| idx as u16)
}

#[test]
fn test_tab_bar_active_pill_background_and_hover_x_layout_stability() {
    let (mut app, _) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2, Some("Secondary".to_string()));
    app.switch_tab(0);

    let (term0, _) = render_app(&mut app, 100, 24);
    let buf0 = term0.backend().buffer();
    let row0_before = row_zero_string(buf0);
    assert!(!row0_before.contains('×'));
    let sec_x_before = find_cell_x(buf0, "Secondary").expect("Secondary tab rendered");
    let plus_x_before = find_cell_x(buf0, "+").expect("New tab + rendered");

    let tab0 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(0))
        .copied()
        .expect("SwitchTab(0) hitbox");
    let tab1 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(1))
        .copied()
        .expect("SwitchTab(1) hitbox");
    let plus = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::NewTab)
        .copied()
        .expect("NewTab hitbox");

    assert_eq!(tab0.rect.width, 28);
    assert_eq!(tab1.rect.width, 28);
    assert_eq!(buf0.cell((tab0.rect.x, 0)).unwrap().bg, TAB_ACTIVE_BG);
    assert_eq!(buf0.cell((tab1.rect.x, 0)).unwrap().bg, Color::Reset);
    assert_eq!(buf0.cell((plus.rect.x, 0)).unwrap().bg, Color::Reset);

    app.handle_terminal_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: tab0.rect.x + 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));

    let (term1, _) = render_app(&mut app, 100, 24);
    let buf1 = term1.backend().buffer();
    let row0_after = row_zero_string(buf1);
    assert!(row0_after.contains('×'));
    assert_eq!(find_cell_x(buf1, "Secondary"), Some(sec_x_before));
    assert_eq!(find_cell_x(buf1, "+"), Some(plus_x_before));

    app.handle_terminal_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: plus.rect.x + 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.hovered_new_tab);
    assert_eq!(app.hovered_tab_index, None);

    let (term2, _) = render_app(&mut app, 100, 24);
    let buf2 = term2.backend().buffer();
    assert_eq!(buf2.cell((plus.rect.x, 0)).unwrap().bg, TAB_ACTIVE_BG);

    let bg_id = app.tabs[1].session_id.clone();
    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id.clone(),
        event: EngineEvent::Stream(Box::new(StreamEvent::Init(InitEvent {
            conversation_id: Some("conv-init-only".to_string()),
            init: None,
        }))),
    });
    assert!(!app.tabs[1].has_unread);

    app.handle_engine_envelope(SessionEnvelope {
        session_id: bg_id,
        event: EngineEvent::Stream(Box::new(StreamEvent::StepUpdate(StepUpdateEvent {
            conversation_id: None,
            step_update: Some(StepUpdateData {
                step_index: Some(1),
                text_delta: Some("streaming...".to_string()),
                ..Default::default()
            }),
        }))),
    });
    assert!(app.tabs[1].has_unread);
    let (term3, _) = render_app(&mut app, 100, 24);
    assert!(row_zero_string(term3.backend().buffer()).contains('⠋'));
}

#[test]
fn test_tab_bar_overflow_truncates_and_keeps_active_tab_and_plus_visible() {
    let (mut app, _) = test_app();
    for idx in 2..=6 {
        app.add_tab(
            SessionId::new(),
            Some(format!("Very Long Session Title Number {}", idx)),
        );
    }
    assert_eq!(app.active_tab_index, 5);

    let (term, _) = render_app(&mut app, 60, 20);
    let row0 = row_zero_string(term.backend().buffer());
    assert!(row0.contains('+'));
    assert!(app
        .hit_regions
        .iter()
        .any(|r| r.action == HitAction::SwitchTab(5)));
    assert!(app
        .hit_regions
        .iter()
        .any(|r| r.action == HitAction::NewTab));
    for region in &app.hit_regions {
        assert_eq!(region.rect.y, 0);
        assert!(region.rect.x >= 2);
        assert!(region.rect.right() <= 58);
    }
}

#[test]
fn test_tab_bar_wide_unicode_truncation_and_divider_render() {
    let (mut app, _) = test_app();
    app.add_tab(
        SessionId::new(),
        Some("🚀 日本語セッションタイトルテスト".to_string()),
    );
    let (term, _) = render_app(&mut app, 40, 15);
    let row0 = row_zero_string(term.backend().buffer());
    assert!(row0.contains('…'));
    let row1: String = (0..40)
        .map(|x| term.backend().buffer()[(x, 1)].symbol())
        .collect();
    assert!(row1.trim().chars().all(|c| c == '─'));
    assert!(row1.contains('─'));
}

#[test]
fn test_render_input_wraps_multibyte_media_and_selection_cleanly() {
    let (mut app, _) = test_app();
    app.cache.push_user_message("init");
    app.editor.insert_str("olá 🦀 ");
    app.editor
        .insert_media_attachment("/tmp/test.png", "image/png");
    app.editor
        .insert_str(" continuação longa para quebrar linha");
    app.editor.select_all();
    let (term, _) = render_app(&mut app, 32, 16);
    let full: String = (0..16)
        .map(|y| {
            (0..32)
                .map(|x| term.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(full.contains("Image #1"));
}
