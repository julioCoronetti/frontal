use frontal::protocol::SessionId;

use crate::common::{TempDirGuard, PGID_TEST_LOCK};

#[tokio::test]
async fn test_respawn_applies_persisted_telemetry() {
    use frontal::infra::history::StoredMessage;
    use frontal::infra::history::TelemetrySnapshot;
    use frontal::protocol::TokenUsage;
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("respawn_telemetry");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new(manager, session_id);

    let conv_id = "conv-telemetry-1";
    app.history.append_messages(
        conv_id.to_string(),
        vec![StoredMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_name: None,
            tool_args: None,
            step_index: None,
            turn_meta: None,
        }],
    );
    app.history.record_telemetry(
        conv_id,
        TelemetrySnapshot {
            last: TokenUsage {
                total_tokens: 250_000,
                ..Default::default()
            },
            accumulated: TokenUsage {
                total_tokens: 400_000,
                ..Default::default()
            },
            steps: 4,
            turns: 2,
            duration_millis: Some(1_500),
            context_window: Some(1_000_000),
            model: Some("opencode-go/deepseek-v4.1-flash".to_string()),
        },
    );
    app.history.flush();

    let result = app
        .respawn_active_session(Some(conv_id.to_string()), None, None, None)
        .await;
    assert!(result.is_ok());
    assert_eq!(
        app.active_tab()
            .last_turn_usage
            .map(|usage| usage.total_tokens),
        Some(250_000)
    );
    assert_eq!(app.active_tab().accumulated_usage.total_tokens, 400_000);
    assert_eq!(app.active_tab().steps_seen, 4);
    assert_eq!(app.active_tab().turns_completed, 2);
    assert_eq!(app.active_tab().last_turn_duration_seconds, Some(1.5));
    assert_eq!(app.active_tab().context_window_hint, Some(1_000_000));
    assert_eq!(
        app.active_tab().served_model.as_deref(),
        Some("opencode-go/deepseek-v4.1-flash")
    );

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("250,000 tokens"));
    assert!(text.contains("25% used"));

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    }
}
