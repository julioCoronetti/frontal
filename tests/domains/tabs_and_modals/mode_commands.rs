use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use frontal::ui::modal::CommandsModal;

use crate::common::{press_key, test_app};

#[tokio::test]
async fn test_mode_command_discovery_modal_selection_and_backend_guards() {
    use frontal::config::BackendKind;
    use frontal::ui::app::AppAction;
    use frontal::ui::autocomplete::AutocompletePopover;
    use frontal::ui::modal::Modal;

    let suggestions = AutocompletePopover::filter_commands("/mo");
    assert!(suggestions.iter().any(|item| item.label == "/mode"));

    let mut commands = CommandsModal::new();
    for c in ['m', 'o', 'd', 'e'] {
        commands.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    let filtered = commands.filtered_indices();
    assert!(filtered
        .iter()
        .any(|&idx| commands.items[idx].id == "select_mode"));

    let (mut app, _) = test_app();
    app.active_tab_mut().editor.insert_str("/help");
    assert_eq!(
        app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE)),
        None
    );
    assert!(app
        .active_tab()
        .cache
        .blocks()
        .iter()
        .any(|b| b.content.contains("/mode")));

    app.active_tab_mut().backend = BackendKind::Agy;
    app.user_config.mode = "plan".to_string();
    let sid = app.active_session_id().clone();
    app.session_manager
        .set_session_metadata(&sid, None, None, None, Some("plan".to_string()));
    app.active_tab_mut().editor.insert_str("/mode");
    assert_eq!(
        app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE)),
        None
    );
    match app.modals.top() {
        Some(Modal::Select(select)) => {
            let ids: Vec<&str> = select.items.iter().map(|it| it.id.as_str()).collect();
            assert_eq!(ids, vec!["auto", "plan", "accept-edits"]);
            assert_eq!(select.selected_idx, 1);
        }
        _ => panic!("expected mode select modal"),
    }

    app.handle_terminal_event(press_key(KeyCode::Down, KeyModifiers::NONE));
    let action = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        action,
        Some(AppAction::RespawnSession {
            conversation_id: None,
            model: None,
            effort: None,
            mode: Some("accept-edits".to_string()),
        })
    );
    assert!(app.modals.is_empty());
    assert_eq!(app.user_config.mode, "plan");
    app.respawn_active_session(None, None, None, Some("accept-edits".to_string()))
        .await
        .unwrap();
    assert_eq!(app.user_config.mode, "accept-edits");

    app.active_tab_mut().backend = BackendKind::OpenCode;
    app.active_tab_mut().editor.insert_str("/mode");
    app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    match app.modals.top() {
        Some(Modal::Select(select)) => {
            let ids: Vec<&str> = select.items.iter().map(|it| it.id.as_str()).collect();
            assert_eq!(ids, vec!["auto", "plan"]);
        }
        _ => panic!("expected opencode mode select modal"),
    }
    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));

    app.active_tab_mut().editor.insert_str("/mode accept-edits");
    let rejected = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(rejected, None);
    assert!(app
        .active_tab()
        .status_text
        .contains("Unsupported mode 'accept-edits'"));

    app.active_tab_mut().editor.insert_str("/mode plan");
    let direct = app.handle_terminal_event(press_key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        direct,
        Some(AppAction::RespawnSession {
            conversation_id: None,
            model: None,
            effort: None,
            mode: Some("plan".to_string()),
        })
    );
}

#[test]
fn test_rename_command_and_title_synchronization() {
    let (mut app, _id1) = test_app();
    app.active_tab_mut()
        .editor
        .insert_str("/rename Architectural Refactoring");
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    let action = app.handle_terminal_event(enter);
    assert_eq!(action, None);
    assert_eq!(app.tabs[0].title, "Architectural Refactoring");
    assert_eq!(
        app.active_tab().status_text,
        "Session renamed to: Architectural Refactoring"
    );

    let sessions = app.history.sessions();
    assert!(!sessions.is_empty());
    assert_eq!(
        sessions[0].title.as_deref(),
        Some("Architectural Refactoring")
    );

    app.active_tab_mut().editor.insert_str("/rename   ");
    let enter2 = press_key(KeyCode::Enter, KeyModifiers::NONE);
    let _ = app.handle_terminal_event(enter2);
    assert_eq!(app.active_tab().status_text, "Usage: /rename <title>");
    assert_eq!(app.tabs[0].title, "Architectural Refactoring");

    let clean = frontal::infra::history::clean_session_title(
        "cara seguinte estamos com problema na trc fiserv",
    );
    assert_eq!(clean, "Estamos com problema na trc fiserv");

    let clean_empty = frontal::infra::history::clean_session_title("seguinte:");
    assert_eq!(clean_empty, "New Session");
}
