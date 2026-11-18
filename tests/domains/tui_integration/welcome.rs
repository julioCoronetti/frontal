use crossterm::event::{KeyCode, KeyModifiers};
use frontal::config::BackendKind;
use frontal::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BORDER_SUBTLE, TEXT_PRIMARY,
};
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
    assert!(text.contains("Ctrl+P: Palette"));

    let gem_y = find_row_containing(buf, "▄▀▀▀▀▄").expect("gem top row");
    let logo_y = gem_y.saturating_sub(1);
    let input_y = find_row_containing(buf, "Gemini 3.8 Flash").expect("input row");
    let meta_y = find_row_containing(buf, "Ctrl+P: Palette").expect("meta bar row");
    let footer_y = find_row_containing(buf, &version_badge).expect("version footer row");

    assert!(logo_y > 2 && logo_y < input_y);
    assert!(input_y > 6 && input_y < 22);
    assert!(meta_y > input_y && meta_y < footer_y);
    assert_eq!(footer_y, 29);

    let logo_x = (0..buf.area.width)
        .find(|&x| {
            let cell = buf.cell((x, logo_y)).unwrap();
            cell.symbol() == "█" && cell.fg == ACCENT_HIGHLIGHT
        })
        .expect("logo top accent x");
    assert!(logo_x > 25 && logo_x < 55);

    let mut found_colors = Vec::new();
    for y in logo_y..=logo_y + 4 {
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
    assert!(found_colors.contains(&BORDER_SUBTLE));
    assert!(found_colors.contains(&Color::Rgb(230, 43, 67)));
}

#[test]
fn test_hero_welcome_popover_anchored_below_and_clamped() {
    let (mut app, _) = test_app();
    app.editor.insert_str("/");
    app.update_autocomplete();
    assert!(app.popover.is_visible());

    let (term, _) = render_app(&mut app, 100, 26);
    let buf = term.backend().buffer();
    let logo_y = find_row_containing(buf, "▄▀▀▀▀▄").expect("logo row");
    let input_y = find_row_containing(buf, "Gemini 3.8 Flash").expect("input row");
    let popover_y = find_row_containing(buf, "Commands (Tab/Enter: apply, Esc: close)")
        .expect("popover title row");

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
        app.backend = backend_kind;

        let (_term_before, text_before) = render_app(&mut app, 120, 30);
        assert!(text_before.contains("▄▀▀▀▀▄"));
        assert!(text_before.contains(&app.cwd_display));

        app.cache.push_user_message("first prompt");
        app.waiting_for_response = true;

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

        app.editor.insert_str("/");
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
        app.editor.insert_str("line\n");
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
