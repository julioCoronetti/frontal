use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::ui::modal::{ModalAction, ModelEffortModal};

use crate::common::{press_key, test_app};

#[test]
fn test_model_effort_selector_two_step_flow() {
    let mut modal = ModelEffortModal::new();
    assert_eq!(modal.selected_model_idx, 0);

    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    modal.handle_key(down);
    assert_eq!(modal.selected_model_idx, 1);

    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
    modal.handle_key(left);
    assert_eq!(modal.selected_effort_idx, 1);

    let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    modal.handle_key(right);
    assert_eq!(modal.selected_effort_idx, 2);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act = modal.handle_key(enter);
    assert_eq!(
        act,
        ModalAction::SelectModelEffort {
            model: "gemini-4-argon".to_string(),
            effort: "high".to_string(),
        }
    );
}

#[test]
fn test_model_effort_query_filtering_and_effort_cycling() {
    let mut modal = ModelEffortModal::new();
    assert_eq!(modal.filtered_indices().len(), modal.models.len());

    let char_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    let char_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE);
    modal.handle_key(char_s);
    modal.handle_key(char_o);
    assert_eq!(modal.query, "so");

    let filtered = modal.filtered_indices();
    for &idx in &filtered {
        let m = &modal.models[idx];
        assert!(m.id.contains("so") || m.name.to_lowercase().contains("so"));
    }

    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    modal.handle_key(tab);
    assert_eq!(modal.selected_effort_idx, 0);
    let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
    modal.handle_key(backtab);
    assert_eq!(modal.selected_effort_idx, 0);

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let action = modal.handle_key(enter);
    assert!(matches!(
        action,
        ModalAction::SelectModelEffort {
            ref effort,
            ..
        } if effort == "high"
    ));
}

#[test]
fn test_model_catalog_normalization_and_effort_decoupling() {
    let raw_models = vec![
        (
            "gemini-3.8-flash-high".to_string(),
            "Gemini 3.8 Flash (High)".to_string(),
        ),
        (
            "gemini-3.8-flash-medium".to_string(),
            "Gemini 3.8 Flash (Medium)".to_string(),
        ),
        (
            "gemini-3.8-flash-low".to_string(),
            "Gemini 3.8 Flash (Low)".to_string(),
        ),
        (
            "gpt-oss-120b-medium".to_string(),
            "GPT-OSS 120B (Medium)".to_string(),
        ),
        ("custom-model".to_string(), "Custom Model".to_string()),
    ];

    let mut modal = ModelEffortModal::with_models(raw_models);
    assert_eq!(modal.models.len(), 3);
    assert_eq!(modal.models[0].id, "gemini-3.8-flash");
    assert_eq!(modal.models[0].name, "Gemini 3.8 Flash");
    assert_eq!(
        modal.models[0].supported_efforts,
        vec!["low".to_string(), "medium".to_string(), "high".to_string()]
    );

    assert_eq!(modal.models[1].id, "gpt-oss-120b");
    assert_eq!(modal.models[1].name, "GPT-OSS 120B");
    assert_eq!(
        modal.models[1].supported_efforts,
        vec!["medium".to_string()]
    );

    assert_eq!(modal.models[2].id, "custom-model");
    assert_eq!(modal.models[2].name, "Custom Model");

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let act = modal.handle_key(enter);
    assert_eq!(
        act,
        ModalAction::SelectModelEffort {
            model: "gemini-3.8-flash".to_string(),
            effort: "high".to_string(),
        }
    );

    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    modal.handle_key(down);
    assert_eq!(modal.selected_model_idx, 1);
    assert_eq!(modal.current_supported_efforts(), &["medium".to_string()]);

    let act2 = modal.handle_key(enter);
    assert_eq!(
        act2,
        ModalAction::SelectModelEffort {
            model: "gpt-oss-120b".to_string(),
            effort: "medium".to_string(),
        }
    );
}

#[test]
fn test_model_effort_modal_rendering_layout_and_slider() {
    let mut modal = ModelEffortModal::new();
    let backend = TestBackend::new(50, 18);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| modal.render(f, f.area())).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut content = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            content.push_str(buffer[(x, y)].symbol());
        }
        content.push('\n');
    }

    assert!(content.contains("Select Model:"));
    assert!(content.contains("Search:"));
    assert!(content.contains('●'));
    assert!(content.contains('◎'));
    assert!(content.contains('◀'));
    assert!(content.contains('▶'));

    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
    modal.handle_key(left);
    assert_eq!(modal.selected_effort_idx, 1);

    terminal.draw(|f| modal.render(f, f.area())).unwrap();
    let buffer2 = terminal.backend().buffer().clone();
    let mut content2 = String::new();
    for y in 0..buffer2.area.height {
        for x in 0..buffer2.area.width {
            content2.push_str(buffer2[(x, y)].symbol());
        }
        content2.push('\n');
    }

    assert!(content2.contains("medium"));
}

#[test]
fn test_model_effort_modal_restores_last_selected_effort_and_model() {
    let raw_models = vec![
        (
            "gemini-3.8-flash".to_string(),
            "Gemini 3.8 Flash".to_string(),
        ),
        ("gemini-4-argon".to_string(), "Gemini 4 Argon".to_string()),
    ];
    let modal = ModelEffortModal::with_models(raw_models)
        .with_initial_selection(Some("gemini-4-argon".to_string()), Some("low".to_string()));

    assert_eq!(modal.selected_model_idx, 1);
    assert_eq!(modal.selected_effort_idx, 0);
    assert_eq!(
        modal.current_supported_efforts()[modal.selected_effort_idx],
        "low"
    );

    let (mut app, session_id) = test_app();
    app.session_manager.set_session_metadata(
        &session_id,
        None,
        None,
        Some("medium".to_string()),
        None,
    );
    app.editor.insert_str("/model");
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_terminal_event(enter);

    assert_eq!(app.modals.stack.len(), 1);
    if let Some(frontal::ui::modal::Modal::ModelEffort(ref m)) = app.modals.top() {
        assert_eq!(m.current_effort.as_deref(), Some("medium"));
        assert_eq!(
            m.current_supported_efforts()[m.selected_effort_idx],
            "medium"
        );
    } else {
        panic!("expected ModelEffort modal");
    }
}
