use crossterm::event::{KeyCode, KeyModifiers};
use frontal::config::BackendKind;
use frontal::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, TEXT_PRIMARY};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;

use crate::common::{press_key, render_app, test_app};

fn row_text(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf.cell((x, y)).unwrap().symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn find_row_containing(buf: &Buffer, needle: &str) -> Option<u16> {
    (0..buf.area.height).find(|&y| row_text(buf, y).contains(needle))
}

#[test]
fn test_hero_welcome_centered_layout_and_version_footer() {
    let (mut app, _) = test_app();
    let (term, text) = render_app(&mut app, 100, 30);
    let buf = term.backend().buffer();
    let version_badge = format!("v{}", env!("CARGO_PKG_VERSION"));

    assert!(text.contains("▄▀▀▀▀▄"));
    assert!(text.contains(&version_badge));
    assert!(!text.contains("Async TUI for Antigravity"));
    assert!(!text.contains("/resume"));
    assert!(text.contains(&app.cwd_display));
    assert!(text.contains("Ctrl+P: Commands"));

    let gem_y = find_row_containing(buf, "▄▀▀▀▀▄").expect("gem top row");
    let logo_y = gem_y.saturating_sub(1);
    let input_y = find_row_containing(buf, "Gemini 3.8 Flash").expect("input row");
    let meta_y = find_row_containing(buf, "Ctrl+P: Commands").expect("meta bar row");
    let footer_y = find_row_containing(buf, &version_badge).expect("version footer row");

    assert!(logo_y > 2 && logo_y < input_y);
    assert!(input_y > 6 && input_y < 22);
    assert!(meta_y > input_y && meta_y < footer_y);
    assert_eq!(footer_y, 29);

    let logo_x = (0..buf.area.width)
        .find(|&x| {
            let cell = buf.cell((x, logo_y)).unwrap();
            cell.symbol() == "▄" && cell.fg == ACCENT_HIGHLIGHT
        })
        .expect("logo top accent x");
    assert!(logo_x > 25 && logo_x < 55);

    let mut found_colors = Vec::new();
    for y in logo_y..=logo_y + 3 {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).unwrap();
            if cell.symbol() != " " {
                found_colors.push(cell.fg);
            }
        }
    }
    assert!(found_colors.contains(&ACCENT_HIGHLIGHT));
    assert!(found_colors.contains(&ACCENT_PRIMARY));
    assert!(found_colors.contains(&ACCENT_RUBY));
    assert!(found_colors.contains(&Color::Rgb(230, 43, 67)));
}

#[test]
fn test_hero_welcome_popover_anchored_below_and_clamped() {
    let (mut app, _) = test_app();
    app.active_tab_mut().editor.insert_str("/");
    app.update_autocomplete();
    assert!(app.popover.is_visible());

    let (term, _) = render_app(&mut app, 100, 26);
    let buf = term.backend().buffer();
    let logo_y = find_row_containing(buf, "▄▀▀▀▀▄").expect("logo row");
    let input_y = find_row_containing(buf, "Gemini 3.8 Flash").expect("input row");
    let popover_y = find_row_containing(buf, " Commands ").expect("popover title row");

    assert!(logo_y < input_y);
    assert!(popover_y > input_y);

    let mut tight_term = Terminal::new(TestBackend::new(80, 16)).unwrap();
    for _ in 0..8 {
        app.handle_terminal_event(press_key(KeyCode::Down, KeyModifiers::NONE));
    }
    tight_term.draw(|f| app.render(f)).unwrap();
    let tight_buf = tight_term.backend().buffer();
    let has_selection = (0..tight_buf.area.height).any(|y| {
        (0..tight_buf.area.width).any(|x| {
            let cell = tight_buf.cell((x, y)).unwrap();
            cell.bg == ACCENT_RUBY && cell.fg == TEXT_PRIMARY
        })
    });
    assert!(has_selection);
}

#[test]
fn test_hero_welcome_transition_on_first_prompt_both_backends() {
    for backend_kind in [BackendKind::Agy, BackendKind::OpenCode] {
        let (mut app, _) = test_app();
        app.active_tab_mut().backend = backend_kind;

        let (_term_before, text_before) = render_app(&mut app, 120, 30);
        assert!(text_before.contains("▄▀▀▀▀▄"));
        assert!(text_before.contains(&app.cwd_display));

        app.active_tab_mut().cache.push_user_message("first prompt");
        app.active_tab_mut()
            .transition_turn(frontal::ui::tab::TurnEvent::Submit);

        let (term_after, text_after) = render_app(&mut app, 120, 30);
        let buf_after = term_after.backend().buffer();
        assert!(!text_after.contains("▄▀▀▀▀▄"));
        assert!(text_after.contains("first prompt"));
        assert!(text_after.contains("Context"));

        let input_y = find_row_containing(buf_after, "Gemini 3.8 Flash").expect("docked input");
        assert!(input_y >= 24);
    }
}

#[test]
fn test_hero_welcome_small_terminal_fallback_and_adversarial_sizes() {
    for (w, h) in [(49, 20), (80, 15)] {
        let (mut app, _) = test_app();
        let (_term_plain, text_plain) = render_app(&mut app, w, h);
        assert!(text_plain.contains("▄▀▀▀▀▄"));

        app.active_tab_mut().editor.insert_str("/");
        app.update_autocomplete();

        let (term, _) = render_app(&mut app, w, h);
        let buf = term.backend().buffer();
        let input_y = find_row_containing(buf, "Gemini 3.8 Flash").expect("fallback input row");
        assert!(input_y >= h.saturating_sub(5));

        if let Some(popover_y) = find_row_containing(buf, "Commands") {
            assert!(popover_y < input_y);
        }
    }

    let (mut app, _) = test_app();
    for _ in 0..10 {
        app.active_tab_mut().editor.insert_str("line\n");
    }
    let version_badge = format!("v{}", env!("CARGO_PKG_VERSION"));
    let (tight_hero, tight_text) = render_app(&mut app, 80, 16);
    assert!(tight_text.contains("▄▀▀▀▀▄"));
    assert!(tight_text.contains(&version_badge));
    assert!(find_row_containing(tight_hero.backend().buffer(), &version_badge).is_some());

    let (mut narrow_app, _) = test_app();
    let (_narrow_term, narrow_text) = render_app(&mut narrow_app, 34, 12);
    assert!(narrow_text.contains("Frontal"));
    assert!(narrow_text.contains(&version_badge));

    for (w, h) in [(50, 16), (40, 14), (20, 8), (1, 1)] {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        assert!(terminal.draw(|f| app.render(f)).is_ok());
    }
}

#[test]
fn test_welcome_hero_and_chat_input_visual_wrapping_and_cursor() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    app.active_tab_mut().editor.insert_str(&"b".repeat(72));
    terminal.draw(|f| app.render(f)).unwrap();
    let row_b = find_row_containing(terminal.backend().buffer(), &"b".repeat(72))
        .expect("first wrapped input row");
    let cursor_exact = terminal.get_cursor_position().unwrap();
    assert_eq!(cursor_exact.y, row_b + 1);

    app.active_tab_mut().editor.clear();
    app.active_tab_mut()
        .editor
        .insert_str(&format!("{}XYZ", "a".repeat(72)));
    terminal.draw(|f| app.render(f)).unwrap();
    let row_1 = find_row_containing(terminal.backend().buffer(), &"a".repeat(72))
        .expect("first row with 72 chars");
    let row_2 = find_row_containing(terminal.backend().buffer(), "XYZ")
        .expect("wrapped second row with XYZ");
    assert_eq!(row_2, row_1 + 1);
    let cursor_pos = terminal.get_cursor_position().unwrap();
    assert_eq!(cursor_pos.y, row_2);
    assert!(cursor_pos.x > 0 && cursor_pos.x < 120);

    app.active_tab_mut().cache.push_user_message("hello");
    app.active_tab_mut().editor.clear();
    app.active_tab_mut()
        .editor
        .insert_str(&format!("{}TAIL", "c".repeat(76)));
    let chat_backend = TestBackend::new(80, 24);
    let mut chat_terminal = Terminal::new(chat_backend).unwrap();
    chat_terminal.draw(|f| app.render(f)).unwrap();
    let chat_row_1 = find_row_containing(chat_terminal.backend().buffer(), &"c".repeat(72))
        .expect("chat wrapped first row");
    let chat_row_2 = find_row_containing(chat_terminal.backend().buffer(), "TAIL")
        .expect("chat wrapped tail row");
    assert_eq!(chat_row_2, chat_row_1 + 1);
    let chat_cursor = chat_terminal.get_cursor_position().unwrap();
    assert_eq!(chat_cursor.y, chat_row_2);
}

#[test]
fn test_welcome_scroll_extmark_wrap_and_clear_parity() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    app.active_tab_mut().editor.insert_str("line1 ");
    app.active_tab_mut().editor.insert_extmark(
        frontal::ui::editor::ExtmarkKind::Media {
            mime_type: "image/png".to_string(),
            uri: "file:///tmp/x.png".to_string(),
        },
        "[Image #1]",
    );
    app.active_tab_mut()
        .editor
        .insert_str(&format!(" {}", "w".repeat(600)));
    terminal.draw(|f| app.render(f)).unwrap();

    let cursor = terminal.get_cursor_position().unwrap();
    assert!(cursor.y < 24);
    assert!(cursor.x < 80);
    assert!(find_row_containing(terminal.backend().buffer(), "line1 [Image #1]").is_none());

    app.active_tab_mut().steps_seen = 4;
    app.active_tab_mut().turns_completed = 2;
    app.active_tab_mut().last_turn_duration_seconds = Some(3.5);
    app.dispatch_modal_action(frontal::ui::modal::ModalAction::ExecuteCommand(
        "clear".to_string(),
    ));
    assert_eq!(app.active_tab().steps_seen, 0);
    assert_eq!(app.active_tab().turns_completed, 0);
    assert!(app.active_tab().last_turn_duration_seconds.is_none());
    assert!(app.active_tab().in_flight_usage.is_none());
}
