use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::style::Modifier;

use frontal::ui::app::AppAction;
use frontal::ui::editor::ExtmarkKind;
use frontal::ui::theme::ACCENT_RUBY;

use crate::common::{press_key, render_app, test_app, TempDirGuard};

#[test]
fn test_ctrl_v_isolated_clipboard_image_attaches_and_renders_accent_ruby() {
    let temp = TempDirGuard::new("tui_ctrl_v_image");
    let (mut app, _) = test_app();
    app.clipboard.set_media_cache_dir(temp.path().to_path_buf());
    app.clipboard.set_isolated_image(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    );

    let act = app.handle_terminal_event(press_key(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(act, None);
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);
    assert!(app.active_tab().editor.text().contains("[Image #1]"));
    assert!(matches!(
        app.active_tab().editor.extmarks()[0].kind,
        ExtmarkKind::Media { .. }
    ));

    let (terminal, _) = render_app(&mut app, 80, 24);
    let buf = terminal.backend().buffer();
    let has_ruby_image_pill = buf
        .content()
        .iter()
        .any(|c| c.symbol() == "I" && c.fg == ACCENT_RUBY && c.modifier.contains(Modifier::BOLD));
    assert!(has_ruby_image_pill);

    app.active_tab_mut().editor.insert_str(" explain");
    let submit = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    let Some(AppAction::SubmitPrompt(payload)) = submit else {
        panic!("expected AppAction::SubmitPrompt");
    };
    assert_eq!(payload.text, "explain");
    assert_eq!(payload.media.len(), 1);
    assert_eq!(payload.media[0].mime_type, "image/png");
    assert!(payload.media[0]
        .uri
        .starts_with(&temp.path().to_string_lossy().to_string()));
}

#[test]
fn test_bracketed_paste_drag_and_drop_image_and_non_image_fallback() {
    let temp = TempDirGuard::new("tui_dnd_image");
    let img_path = temp.path().join("dropped photo.jpg");
    let txt_path = temp.path().join("readme.md");
    std::fs::write(&img_path, b"jpeg-bytes").unwrap();
    std::fs::write(&txt_path, b"# title").unwrap();

    let (mut app, _) = test_app();
    let file_uri = format!("file://{}", img_path.to_string_lossy().replace(' ', "%20"));
    app.handle_terminal_event(Event::Paste(file_uri));
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);
    assert!(app.active_tab().editor.text().contains("[Image #1]"));

    app.handle_terminal_event(Event::Paste(txt_path.to_string_lossy().into_owned()));
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);
    assert!(app.active_tab().editor.text().contains("readme.md"));
}

#[test]
fn test_ctrl_v_image_backspace_drops_attachment_before_submit() {
    let temp = TempDirGuard::new("tui_ctrl_v_backspace");
    let (mut app, _) = test_app();
    app.clipboard.set_media_cache_dir(temp.path().to_path_buf());
    app.clipboard.set_isolated_image(1, 1, vec![0, 0, 0, 255]);

    app.handle_terminal_event(press_key(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);

    app.handle_terminal_event(press_key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.active_tab().editor.extmarks().len(), 0);
    assert!(app.active_tab().editor.is_empty());

    let submit = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(submit, None);
}

#[test]
fn test_alt_v_and_empty_bracketed_paste_trigger_image_attachment() {
    let temp = TempDirGuard::new("tui_alt_v_empty_paste");
    let (mut app, _) = test_app();
    app.clipboard.set_media_cache_dir(temp.path().to_path_buf());

    let empty_alt = app.handle_terminal_event(press_key(KeyCode::Char('v'), KeyModifiers::ALT));
    assert_eq!(empty_alt, None);
    assert!(app.active_tab().editor.is_empty());

    app.clipboard
        .set_isolated_image(1, 1, vec![12, 34, 56, 255]);
    app.handle_terminal_event(press_key(KeyCode::Char('v'), KeyModifiers::ALT));
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);
    assert!(app.active_tab().editor.text().contains("[Image #1]"));

    app.active_tab_mut().editor.clear();
    app.handle_terminal_event(Event::Paste(String::new()));
    assert_eq!(app.active_tab().editor.extmarks().len(), 1);
    assert!(app.active_tab().editor.text().contains("[Image #1]"));

    app.clipboard = frontal::ui::clipboard::SystemClipboard::new();
    let live_alt = app.handle_terminal_event(press_key(KeyCode::Char('v'), KeyModifiers::ALT));
    assert_eq!(live_alt, Some(AppAction::PasteClipboard));
    let live_empty_paste = app.handle_terminal_event(Event::Paste(String::new()));
    assert_eq!(live_empty_paste, Some(AppAction::PasteClipboard));
}
