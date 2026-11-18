use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use frontal::protocol::SessionId;
use frontal::ui::app::{AppAction, HitAction};

use crate::common::{render_app, test_app};

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn row_zero_text(buf: &ratatui::buffer::Buffer) -> String {
    (0..buf.area.width)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn test_tab_bar_click_switch_and_new_tab() {
    let (mut app, _) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2, Some("Alpha".to_string()));
    assert_eq!(app.active_tab_index, 1);

    let (_term, _) = render_app(&mut app, 100, 24);
    let tab0 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(0))
        .copied()
        .expect("SwitchTab(0) hitbox");
    let new_tab = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::NewTab)
        .copied()
        .expect("NewTab hitbox");

    let act_switch = app.handle_terminal_event(mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        tab0.rect.x,
        tab0.rect.y,
    ));
    assert_eq!(act_switch, Some(AppAction::SwitchTab(0)));
    assert_eq!(app.active_tab_index, 0);

    let act_new = app.handle_terminal_event(mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        new_tab.rect.x + 1,
        new_tab.rect.y,
    ));
    assert_eq!(act_new, Some(AppAction::NewTab));
}

#[test]
fn test_tab_bar_hover_reveals_close_and_click_closes_tab() {
    let (mut app, _) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2, Some("Alpha".to_string()));
    app.switch_tab(0);

    let (term0, _) = render_app(&mut app, 100, 24);
    assert!(!row_zero_text(term0.backend().buffer()).contains('×'));
    assert!(!app
        .hit_regions
        .iter()
        .any(|r| matches!(r.action, HitAction::CloseOrResetTab(_))));

    let tab1 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(1))
        .copied()
        .expect("SwitchTab(1) hitbox");

    app.dirty = false;
    let act_hover =
        app.handle_terminal_event(mouse_event(MouseEventKind::Moved, tab1.rect.x, tab1.rect.y));
    assert_eq!(act_hover, None);
    assert_eq!(app.hovered_tab_index, Some(1));
    assert!(app.dirty);

    let (term1, _) = render_app(&mut app, 100, 24);
    assert!(row_zero_text(term1.backend().buffer()).contains('×'));

    let body1 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(1))
        .copied()
        .expect("hovered SwitchTab(1) hitbox");
    let close1 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::CloseOrResetTab(1))
        .copied()
        .expect("hovered CloseOrResetTab(1) hitbox");
    assert!(body1.rect.right() <= close1.rect.x);

    let act_close = app.handle_terminal_event(mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        close1.rect.x + 1,
        close1.rect.y,
    ));
    assert_eq!(act_close, Some(AppAction::CloseOrResetTab(1)));

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_or_reset_tab(1).await;
        assert!(closed.is_some());
    });
    assert_eq!(app.tab_count(), 1);
    assert_eq!(app.active_tab_index, 0);
}

#[test]
fn test_sole_tab_hover_close_resets_to_welcome() {
    let (mut app, old_id) = test_app();
    app.cache.push_user_message("Hello");
    app.cache.push_error("Done");
    assert!(app.cache.blocks_count() > 0);

    let (term0, _) = render_app(&mut app, 100, 24);
    assert!(!row_zero_text(term0.backend().buffer()).contains('×'));

    let tab0 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(0))
        .copied()
        .expect("sole SwitchTab(0) hitbox");

    app.handle_terminal_event(mouse_event(MouseEventKind::Moved, tab0.rect.x, tab0.rect.y));
    assert_eq!(app.hovered_tab_index, Some(0));

    let (term1, _) = render_app(&mut app, 100, 24);
    assert!(row_zero_text(term1.backend().buffer()).contains('×'));

    let close0 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::CloseOrResetTab(0))
        .copied()
        .expect("sole CloseOrResetTab(0) hitbox");
    let act = app.handle_terminal_event(mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        close0.rect.x + 1,
        close0.rect.y,
    ));
    assert_eq!(act, Some(AppAction::CloseOrResetTab(0)));

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let closed = app.close_or_reset_tab(0).await;
        assert_eq!(closed, Some(old_id.clone()));
    });

    assert_eq!(app.tab_count(), 1);
    assert_eq!(app.cache.blocks_count(), 0);
    assert_ne!(app.active_session, old_id);

    let (_term2, welcome_text) = render_app(&mut app, 100, 24);
    assert!(welcome_text.contains("▄▀▀▀▀▄"));
}

#[test]
fn test_mouse_moved_outside_tab_bar_does_not_dirty() {
    let (mut app, _) = test_app();
    let id2 = SessionId::new();
    app.add_tab(id2, Some("Second".to_string()));
    let (_term, _) = render_app(&mut app, 100, 24);

    app.dirty = false;
    app.handle_terminal_event(mouse_event(MouseEventKind::Moved, 10, 5));
    assert!(!app.dirty);
    assert_eq!(app.hovered_tab_index, None);

    let tab0 = app
        .hit_regions
        .iter()
        .find(|r| r.action == HitAction::SwitchTab(0))
        .copied()
        .expect("SwitchTab(0) hitbox");

    app.handle_terminal_event(mouse_event(MouseEventKind::Moved, tab0.rect.x, tab0.rect.y));
    assert_eq!(app.hovered_tab_index, Some(0));
    assert!(app.dirty);

    app.dirty = false;
    app.handle_terminal_event(mouse_event(
        MouseEventKind::Moved,
        tab0.rect.x + 1,
        tab0.rect.y,
    ));
    assert!(!app.dirty);
    assert_eq!(app.hovered_tab_index, Some(0));

    app.handle_terminal_event(mouse_event(MouseEventKind::Moved, 10, 5));
    assert_eq!(app.hovered_tab_index, None);
    assert!(app.dirty);
}
