use frontal::config::BackendKind;
use frontal::core::security::{evaluate_command, DangerAssessment, RiskCategory};
use frontal::infra::history::TitleSource;
use frontal::protocol::{EngineEvent, SessionEnvelope, SessionId};
use frontal::session::SessionManager;
use frontal::ui::app::App;
use frontal::ui::cache::BlockKind;
use frontal::ui::modal::ModalAction;

#[tokio::test]
async fn dormant_respawn_preserves_tab_backend_and_conversation_id() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    manager.set_session_metadata(
        &sid,
        Some("ses_opencode_1".to_string()),
        Some("opencode/gpt-5".to_string()),
        None,
        None,
    );
    let mut app = App::new_isolated(manager, sid.clone());
    app.tabs[0].backend = BackendKind::OpenCode;
    app.user_config.backend = BackendKind::Agy;

    let new_id = app
        .respawn_active_session(None, None, None, None)
        .await
        .unwrap();
    assert_eq!(app.tabs[0].backend, BackendKind::OpenCode);
    assert_eq!(app.user_config.backend, BackendKind::Agy);
    assert_eq!(
        app.session_manager
            .get_metadata(&new_id)
            .and_then(|m| m.conversation_id.clone())
            .as_deref(),
        Some("ses_opencode_1")
    );
}

#[test]
fn reject_danger_consolidates_active_turn_blocks() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid);
    app.cache
        .append_streaming_delta(BlockKind::Assistant, "partial response");
    assert!(app.cache.has_unconsolidated_blocks());
    app.pending_danger = Some(DangerAssessment {
        step_index: Some(1),
        tool_name: "bash".to_string(),
        command: "rm -rf /tmp/x".to_string(),
        category: RiskCategory::RecursiveDeletion,
        target: "/tmp/x".to_string(),
        warning: "destructive".to_string(),
    });
    app.dispatch_modal_action(ModalAction::RejectDanger);
    assert!(!app.cache.has_unconsolidated_blocks());
    assert!(!app.waiting_for_response);
}

#[test]
fn protocol_error_consolidates_active_turn_and_clears_telemetry() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid.clone());
    app.cache
        .append_streaming_delta(BlockKind::Assistant, "partial");
    app.waiting_for_response = true;
    app.turn_started_at = Some(std::time::Instant::now());

    app.handle_engine_envelope(SessionEnvelope {
        session_id: sid,
        event: EngineEvent::ProtocolError {
            message: "bad frame".to_string(),
            raw_preview: None,
        },
    });
    assert!(!app.cache.has_unconsolidated_blocks());
    assert!(app.turn_started_at.is_none());
    assert!(app.in_flight_usage.is_none());
    assert!(!app.waiting_for_response);
}

#[tokio::test]
async fn close_or_reset_tab_fast_path_resets_title_source() {
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid);
    app.tabs[0].title = "Custom".to_string();
    app.tabs[0].custom_title = true;
    app.tabs[0].title_source = TitleSource::Custom;

    let _ = app.close_or_reset_tab(0).await;
    assert_eq!(app.tabs[0].title, "New session");
    assert!(!app.tabs[0].custom_title);
    assert_eq!(app.tabs[0].title_source, TitleSource::Default);
}

#[test]
fn security_subshell_flags_and_sql_block_comments_detected() {
    assert!(evaluate_command(r#"bash -lc "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"sh -e -c "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"bash -o pipefail -c "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"sh -c -e "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"sh -co errexit "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"bash -oc pipefail "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"bash -O extglob -c "rm -rf /tmp/x""#).is_some());
    assert!(evaluate_command(r#"sh -- -c "rm -rf /tmp/x""#).is_none());

    assert!(evaluate_command("DROP /* comment */ TABLE users").is_some());
    assert!(evaluate_command("DROP/*x*/TABLE users").is_some());
    assert!(evaluate_command("/* /* */ DROP TABLE users").is_some());
    assert!(evaluate_command("SELECT '/* DROP TABLE users */'").is_none());
}

#[test]
fn set_opencode_models_preserves_selection_and_effort() {
    use frontal::ui::modal::{ModelEffortModal, ModelEffortStep};
    let mut modal = ModelEffortModal::new();
    modal.backend = BackendKind::OpenCode;
    modal.provider = Some("openai".to_string());
    modal.step = ModelEffortStep::SelectModel;
    modal.set_opencode_models(vec![
        ("openai/gpt-4".to_string(), "GPT-4".to_string()),
        ("openai/gpt-5".to_string(), "GPT-5".to_string()),
    ]);
    modal.selected_model_idx = 1;
    modal.selected_effort_idx = 1;
    modal.set_opencode_models(vec![
        ("anthropic/claude-4".to_string(), "Claude 4".to_string()),
        ("openai/gpt-4".to_string(), "GPT-4".to_string()),
        ("openai/gpt-5".to_string(), "GPT-5".to_string()),
    ]);
    assert_eq!(modal.models[modal.selected_model_idx].id, "openai/gpt-5");
    assert_eq!(modal.selected_effort_idx, 1);
}

#[test]
fn set_opencode_models_drops_back_to_select_model_when_active_removed() {
    use frontal::ui::modal::{ModelEffortModal, ModelEffortStep};
    let mut modal = ModelEffortModal::new();
    modal.backend = BackendKind::OpenCode;
    modal.provider = Some("openai".to_string());
    modal.step = ModelEffortStep::SelectModel;
    modal.set_opencode_models(vec![
        ("openai/gpt-4".to_string(), "GPT-4".to_string()),
        ("openai/gpt-5".to_string(), "GPT-5".to_string()),
    ]);
    modal.selected_model_idx = 1;
    modal.selected_effort_idx = 0;
    modal.step = ModelEffortStep::SelectEffort;
    modal.set_opencode_models(vec![("openai/gpt-4".to_string(), "GPT-4".to_string())]);
    assert_eq!(modal.step, ModelEffortStep::SelectModel);
    assert_eq!(modal.selected_effort_idx, 2);
}
