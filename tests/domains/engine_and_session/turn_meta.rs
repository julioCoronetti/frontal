use rusqlite::Connection;

use frontal::infra::history::{
    conversation_messages_at, PromptHistory, StoredMessage, TelemetrySnapshot,
};
use frontal::protocol::{SessionId, TokenUsage};
use frontal::ui::cache::{BlockKind, RenderCache, TurnMeta};
use frontal::ui::history::hydrate_conversation;

use crate::common::{TempDirGuard, PGID_TEST_LOCK};

#[test]
fn test_messages_schema_migrates_and_round_trips_turn_meta() {
    let guard = TempDirGuard::new("turn_meta_migration");
    let db_path = guard.path().join("history.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY, conversation_id TEXT, model TEXT, mode TEXT,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE prompt_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                prompt TEXT NOT NULL, created_at INTEGER NOT NULL
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id TEXT NOT NULL,
                role TEXT NOT NULL, content TEXT NOT NULL, tool_name TEXT,
                tool_args TEXT, step_index INTEGER, created_at INTEGER NOT NULL
             );",
        )
        .unwrap();
    drop(connection);

    let history = PromptHistory::open(&db_path).unwrap();
    drop(history);
    let history = PromptHistory::open(&db_path).unwrap();

    let meta = TurnMeta {
        mode: "auto".to_string(),
        model: "opencode-go/deepseek-v4.1-flash".to_string(),
        duration_seconds: Some(2.5),
        output_tokens: Some(120),
    };
    let encoded = serde_json::to_string(&meta).unwrap();
    history.append_messages(
        "conv-1".to_string(),
        vec![
            StoredMessage {
                role: "assistant".to_string(),
                content: "with meta".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: Some(encoded.clone()),
            },
            StoredMessage {
                role: "assistant".to_string(),
                content: "legacy".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: None,
            },
        ],
    );
    history.flush();
    drop(history);

    let messages = conversation_messages_at(&db_path, "conv-1");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].turn_meta.as_deref(), Some(encoded.as_str()));
    assert_eq!(messages[1].turn_meta, None);

    let mut cache = RenderCache::new();
    assert!(hydrate_conversation("conv-1", &mut cache, Some(&db_path)));
    let assistant = cache
        .blocks()
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Assistant))
        .expect("assistant block expected");
    assert_eq!(assistant.turn_meta, Some(meta));
    assert!(serde_json::from_str::<TurnMeta>("not-json").is_err());
}

#[tokio::test]
async fn test_resume_synthesizes_turn_meta_from_telemetry() {
    use frontal::session::SessionManager;
    use frontal::ui::app::App;

    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("turn_meta_resume");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", guard.0.clone());

    let mut manager = SessionManager::new();
    let session_id = SessionId::new();
    manager.register_mock_session(session_id.clone());
    let mut app = App::new(manager, session_id);

    let conv_id = "conv-meta-1";
    app.history.append_messages(
        conv_id.to_string(),
        vec![
            StoredMessage {
                role: "user".to_string(),
                content: "oi".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: None,
            },
            StoredMessage {
                role: "assistant".to_string(),
                content: "resposta".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: None,
                turn_meta: None,
            },
        ],
    );
    app.history.record_telemetry(
        conv_id,
        TelemetrySnapshot {
            last: TokenUsage {
                output_tokens: 120,
                total_tokens: 500,
                ..Default::default()
            },
            turns: 1,
            duration_millis: Some(2_500),
            model: Some("opencode-go/deepseek-v4.1-flash".to_string()),
            ..Default::default()
        },
    );
    app.history.flush();

    let result = app
        .respawn_active_session(Some(conv_id.to_string()), None, None, None)
        .await;
    assert!(result.is_ok());

    let (_, text) = crate::common::render_app(&mut app, 120, 30);
    assert!(text.contains("DeepSeek V4.1 Flash"));
    assert!(text.contains("2.5s"));
    assert!(text.contains("48.0 tok/s"));

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    }
}
