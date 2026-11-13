use frontal::infra::history::StoredMessage;
use frontal::ui::cache::RenderCache;
use frontal::ui::history::{hydrate_conversation, PromptHistory, SessionRecord};

use crate::common::TempDirGuard;

#[test]
fn test_conversation_message_store_roundtrip_and_hydration() {
    let dir = TempDirGuard::new("conversation_store");
    let db = dir.path().join("history.db");
    let history = PromptHistory::open(&db).unwrap();
    let conversation = "ses_store_1".to_string();
    history.append_messages(
        conversation.clone(),
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
                role: "thought".to_string(),
                content: "pensando".to_string(),
                tool_name: None,
                tool_args: None,
                step_index: Some(2),
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
    history.flush();

    let stored = frontal::infra::history::conversation_messages_at(&db, &conversation);
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[0].role, "user");
    assert_eq!(stored[1].step_index, Some(2));
    assert_eq!(stored[2].content, "resposta");

    let mut cache = RenderCache::new();
    assert!(hydrate_conversation(
        &conversation,
        &mut cache,
        Some(db.as_path())
    ));
    assert_eq!(cache.blocks_count(), 3);
    assert!(!hydrate_conversation(
        "ses_missing",
        &mut cache,
        Some(db.as_path())
    ));
}

#[test]
fn test_conversation_title_fallback_from_store() {
    let dir = TempDirGuard::new("conversation_title");
    let db = dir.path().join("history.db");
    let mut history = PromptHistory::open(&db).unwrap();
    history.record_session(SessionRecord {
        id: "tab-local".to_string(),
        conversation_id: Some("ses_title".to_string()),
        model: None,
        mode: None,
        effort: None,
        title: Some("Titulo persistido".to_string()),
        created_at: 1,
    });
    history.flush();

    assert_eq!(
        frontal::ui::history::conversation_title("ses_title", Some(db.as_path())).as_deref(),
        Some("Titulo persistido")
    );
}
