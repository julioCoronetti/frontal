use super::*;

fn base_plan(app: &App, old_id: SessionId, was_replay: bool) -> RespawnPlan {
    let tab = app.active_tab();
    RespawnPlan {
        token: OpToken {
            tab_id: tab.tab_id,
            generation_id: tab.generation_id,
        },
        old_id,
        was_replay,
        target_backend: BackendKind::Agy,
        requested_backend: None,
        is_different_conv: false,
        model_changed: false,
        final_conv_id: Some("conv-dormant".to_string()),
        final_mode: Some("ask".to_string()),
        is_explicit_config_change: false,
        shares_active_conv: true,
        prev_meta: None,
        dormant_config: None,
        warnings: Vec::new(),
    }
}

fn empty_outcome() -> RespawnIoResult {
    RespawnIoResult {
        hydrated_session: None,
        final_model: None,
        final_effort: None,
        final_mode: None,
        spawned: None,
        warnings: Vec::new(),
    }
}

#[test]
fn apply_respawn_outcome_missing_spawned_rolls_back_dormant_and_sets_protocol_error() {
    let manager = SessionManager::new();
    let old_id = SessionId::new();
    let mut app = App::new_isolated(manager, old_id.clone());
    app.dirty = false;
    app.tabs[0].transition_turn(TurnEvent::Submit);
    assert!(app.tabs[0].is_waiting_for_response());

    let mut plan = base_plan(&app, old_id.clone(), false);
    plan.prev_meta = Some(SessionMetadata {
        session_id: old_id.clone(),
        conversation_id: Some("conv-dormant".to_string()),
        model: Some("gemini-3.8-flash".to_string()),
        mode: Some("ask".to_string()),
        effort: Some("low".to_string()),
        turns: 2,
        last_prompt: Some("hi".to_string()),
        status: SessionStatus::Ready,
    });
    plan.dormant_config = Some(EngineConfig {
        backend: BackendKind::Agy,
        conversation_id: Some("conv-dormant".to_string()),
        ..Default::default()
    });

    let mut outcome = empty_outcome();
    outcome.final_model = Some("gemini-3.8-pro".to_string());
    outcome.final_effort = Some("high".to_string());
    outcome.final_mode = Some("act".to_string());
    outcome.spawned = None;

    let res = app.apply_respawn_outcome(plan, outcome);
    assert!(
        matches!(res, Err(EngineError::Protocol(ref msg)) if msg.contains("missing spawned engine"))
    );
    assert!(!app.tabs[0].is_waiting_for_response());
    assert!(app.dirty);
    assert!(app.tabs[0].status_text.contains("missing spawned engine"));

    let restored_meta = app
        .session_manager
        .get_metadata(&old_id)
        .expect("restored dormant metadata");
    assert_eq!(restored_meta.status, SessionStatus::Closed);
    assert_eq!(restored_meta.model.as_deref(), Some("gemini-3.8-pro"));
    assert_eq!(restored_meta.effort.as_deref(), Some("high"));
    assert_eq!(restored_meta.mode.as_deref(), Some("act"));
    let restored_cfg = app
        .session_manager
        .get_engine_config(&old_id)
        .expect("restored dormant config");
    assert_eq!(restored_cfg.model.as_deref(), Some("gemini-3.8-pro"));
    assert_eq!(restored_cfg.effort.as_deref(), Some("high"));
    assert_eq!(restored_cfg.mode.as_deref(), Some("act"));
}

#[test]
fn apply_respawn_outcome_merges_plan_and_io_warnings() {
    let mut manager = SessionManager::new();
    let old_id = SessionId::new();
    manager.register_mock_session(old_id.clone());
    let mut app = App::new_isolated(manager, old_id.clone());

    let mut plan = base_plan(&app, old_id, true);
    plan.warnings.push("dormant warn".to_string());
    let mut outcome = empty_outcome();
    outcome.warnings.push("io warn".to_string());

    let new_id = app
        .apply_respawn_outcome(plan, outcome)
        .expect("replay respawn succeeds");
    assert!(app.tabs[0].status_text.contains(&new_id.to_string()));
    assert!(app.tabs[0].status_text.contains("dormant warn; io warn"));
}

#[test]
fn apply_respawn_outcome_err_merges_plan_io_and_dormant_rollback_warnings() {
    let mut manager = SessionManager::new();
    let old_id = SessionId::new();
    manager.register_mock_session(old_id.clone());
    let mut app = App::new_isolated(manager, old_id.clone());
    app.dirty = false;

    let mut plan = base_plan(&app, old_id.clone(), false);
    plan.warnings.push("plan warn".to_string());
    plan.prev_meta = Some(SessionMetadata {
        session_id: old_id.clone(),
        conversation_id: Some("conv-dormant".to_string()),
        model: None,
        mode: None,
        effort: None,
        turns: 0,
        last_prompt: None,
        status: SessionStatus::Ready,
    });
    plan.dormant_config = Some(EngineConfig::default());

    let mut outcome = empty_outcome();
    outcome.spawned = Some(Err(EngineError::Timeout));
    outcome.warnings.push("io warn".to_string());

    let res = app.apply_respawn_outcome(plan, outcome);
    assert!(matches!(res, Err(EngineError::Timeout)));
    assert!(app.dirty);
    assert!(app.tabs[0].status_text.contains(&format!(
        "plan warn; io warn; dormant rollback skipped for active session {}",
        old_id
    )));
}

#[test]
fn apply_respawn_outcome_discards_stale_generation_token() {
    let mut manager = SessionManager::new();
    let old_id = SessionId::new();
    manager.register_mock_session(old_id.clone());
    let mut app = App::new_isolated(manager, old_id.clone());
    app.dirty = false;

    let plan = base_plan(&app, old_id.clone(), true);
    app.tabs[0].bump_generation();
    let res = app.apply_respawn_outcome(plan, empty_outcome());
    assert!(matches!(res, Err(EngineError::ProcessNotRunning)));
    assert!(!app.dirty);
    assert_eq!(app.tabs[0].session_id, old_id);
}
