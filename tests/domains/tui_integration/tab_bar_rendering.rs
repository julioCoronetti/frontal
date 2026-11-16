use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use frontal::protocol::SessionId;
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
