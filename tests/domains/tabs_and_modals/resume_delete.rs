use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use frontal::config::BackendKind;
use frontal::infra::history::{
    telemetry, PromptHistory, SessionRecord, StoredMessage, TelemetrySnapshot, TitleSource,
};
use frontal::protocol::{SessionId, TokenUsage};
use frontal::session::SessionManager;
use frontal::ui::app::App;
use frontal::ui::modal::{DialogSelect, Modal, SelectItem, SelectResult};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use rusqlite::Connection;

use crate::common::{press_key, TempDirGuard, PGID_TEST_LOCK};

struct EnvRestore {
    key: &'static str,
    prev: Option<String>,
}

impl EnvRestore {
    fn set(key: &'static str, val: &std::path::Path) -> Self {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, val);
        Self { key, prev }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn render_dialog_text<T: Clone>(dialog: &DialogSelect<T>) -> String {
    let backend = TestBackend::new(64, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| dialog.render(f, Rect::new(0, 0, 64, 18)))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn test_dialog_select_two_step_delete_arm_disarm_and_confirm() {
    let items = vec![
        SelectItem::new("tab-0", "Active Tab", 0usize).with_category("Current Sessions"),
        SelectItem::new("ses_1", "First Past Session", 1usize)
            .with_category("Past Sessions")
            .with_shortcut("1m ago")
            .with_deletable(true),
        SelectItem::new("ses_2", "Second Past Session", 2usize)
            .with_category("Past Sessions")
            .with_shortcut("5m ago")
            .with_deletable(true),
    ];
    let mut dialog = DialogSelect::new("Resume Session", items);

    assert_eq!(
        dialog.handle_key(key(KeyCode::Delete, KeyModifiers::NONE)),
        SelectResult::None
    );
    assert_eq!(dialog.pending_delete, None);
    assert_eq!(dialog.items.len(), 3);

    dialog.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        dialog.handle_key(key(KeyCode::Delete, KeyModifiers::NONE)),
        SelectResult::None
    );
    assert_eq!(dialog.pending_delete.as_deref(), Some("ses_1"));
    assert_eq!(dialog.items.len(), 3);

    let armed_text = render_dialog_text(&dialog);
    assert!(armed_text.contains("Confirm delete?"));

    assert_eq!(
        dialog.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
        SelectResult::None
    );
    assert_eq!(dialog.pending_delete, None);
    assert_eq!(dialog.items.len(), 3);

    dialog.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(dialog.pending_delete.as_deref(), Some("ses_1"));
    dialog.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(dialog.pending_delete, None);

    dialog.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(dialog.pending_delete.as_deref(), Some("ses_2"));
    let res = dialog.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    match res {
        SelectResult::Deleted(item) => assert_eq!(item.id, "ses_2"),
        other => panic!("expected Deleted(ses_2), got {:?}", other),
    }
    assert_eq!(dialog.items.len(), 2);
    assert_eq!(dialog.selected_idx, 1);
    assert_eq!(dialog.pending_delete, None);

    assert!(frontal::transport::is_valid_opencode_session_id(
        "ses_abc123"
    ));
    assert!(!frontal::transport::is_valid_opencode_session_id("ses_"));
    assert!(!frontal::transport::is_valid_opencode_session_id(
        "ses_../evil"
    ));
    assert!(!frontal::transport::is_valid_opencode_session_id("ses_a b"));
    assert!(!frontal::transport::is_valid_opencode_session_id("-flag"));

    let mut payload = frontal::protocol::PromptPayload::from_text("hi");
    payload.media = vec![frontal::protocol::MediaItem {
        uri: "file://-flag.png".to_string(),
        mime_type: "image/png".to_string(),
    }];
    let args = frontal::transport::build_opencode_payload_args(
        &frontal::transport::EngineConfig::default(),
        Some("ses_invalid id"),
        &payload,
    );
    assert!(!args.iter().any(|a| a == "--session"));
    assert!(args.windows(2).any(|w| w == ["--file", "./-flag.png"]));
}

#[tokio::test]
async fn test_resume_delete_persists_tombstone_and_invokes_opencode_delete() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let guard = TempDirGuard::new("resume_delete_full");

    let cli_log = guard.path().join("opencode_calls.log");
    let script_path = guard.path().join("opencode");
    let script = format!(
        "#!/usr/bin/env bash\necho \"$*\" >> \"{}\"\nexit 0\n",
        cli_log.display()
    );
    std::fs::write(&script_path, script).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();
    let _env_guard = EnvRestore::set("FRONTAL_OPENCODE_BIN", &script_path);

    let agy_db = guard.path().join("conversation_summaries.db");
    {
        let conn = Connection::open(&agy_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT,
                preview TEXT,
                last_modified_time DATETIME
            );
            INSERT INTO conversation_summaries (conversation_id, title, preview, last_modified_time)
            VALUES ('conv-agy-del', 'Agy To Delete', 'p', '2026-11-18T12:00:00Z');",
        )
        .unwrap();
    }

    let oc_db = guard.path().join("opencode.db");
    {
        let conn = Connection::open(&oc_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                parent_id TEXT,
                title TEXT,
                time_created INTEGER,
                time_updated INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            INSERT INTO session (id, project_id, parent_id, title, time_created, time_updated)
            VALUES ('ses_del_123', 'proj', NULL, 'OpenCode To Delete', 1731931200000, 1731931300000);",
        )
        .unwrap();
    }

    let history_db = guard.path().join("history.db");
    let mut manager = SessionManager::new();
    let sid = SessionId::new();
    manager.register_mock_session(sid.clone());
    let mut app = App::new_isolated(manager, sid);
    app.history = PromptHistory::open_with_external_dbs(
        &history_db,
        Some(agy_db.clone()),
        Some(oc_db.clone()),
    )
    .unwrap();
    app.history.record_session(SessionRecord {
        id: "local-agy-1".to_string(),
        conversation_id: Some("conv-agy-del".to_string()),
        backend: Some(BackendKind::Agy),
        model: None,
        mode: None,
        effort: None,
        title: Some("Agy To Delete".to_string()),
        title_source: TitleSource::Semantic,
        created_at: 1_731_931_200,
    });
    app.history.record_session(SessionRecord {
        id: "local-oc-1".to_string(),
        conversation_id: Some("ses_del_123".to_string()),
        backend: Some(BackendKind::OpenCode),
        model: None,
        mode: None,
        effort: None,
        title: Some("OpenCode To Delete".to_string()),
        title_source: TitleSource::Semantic,
        created_at: 1_731_931_300,
    });
    app.history.append_messages(
        "conv-agy-del".to_string(),
        vec![StoredMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_name: None,
            tool_args: None,
            step_index: None,
            turn_meta: None,
        }],
    );
    app.history.flush();
    {
        let conn = Connection::open(&history_db).unwrap();
        telemetry::upsert(
            &conn,
            "conv-agy-del",
            &TelemetrySnapshot {
                accumulated: TokenUsage {
                    input_tokens: 100,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
    }

    app.open_resume_modal(None);
    app.handle_terminal_event(press_key(KeyCode::Down, KeyModifiers::NONE));
    app.handle_terminal_event(press_key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    app.handle_terminal_event(press_key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(!app.modals.is_empty());

    app.handle_terminal_event(press_key(KeyCode::Delete, KeyModifiers::NONE));
    app.handle_terminal_event(press_key(KeyCode::Delete, KeyModifiers::NONE));
    assert!(!app.modals.is_empty());

    if let Some(Modal::Select(dialog)) = app.modals.top() {
        assert_eq!(dialog.items.len(), 1);
        assert_eq!(dialog.items[0].id, "tab-0");
    } else {
        panic!("expected resume select modal");
    }

    app.history.flush();

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(content) = std::fs::read_to_string(&cli_log) {
            if content.contains("session delete ses_del_123") {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for opencode session delete"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    {
        let conn = Connection::open(&history_db).unwrap();
        let sess_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        let msg_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        let tel_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversation_telemetry", [], |r| {
                r.get(0)
            })
            .unwrap();
        let tomb_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM deleted_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sess_count, 0);
        assert_eq!(msg_count, 0);
        assert_eq!(tel_count, 0);
        assert!(tomb_count >= 2);
    }

    app.history.request_agent_refresh(None);
    app.history.flush();
    assert!(app.history.resumable_sessions().is_empty());
    assert!(app.history.cached_agent_conversations().is_empty());

    let reopened =
        PromptHistory::open_with_external_dbs(&history_db, Some(agy_db), Some(oc_db)).unwrap();
    reopened.flush();
    assert!(reopened.resumable_sessions().is_empty());
    assert!(reopened.cached_agent_conversations().is_empty());
}

#[test]
fn test_dialog_select_truncates_narrow_rows_within_width() {
    let items = vec![
        SelectItem::new(
            "1",
            "VeryLongSessionTitleWithoutSpaces",
            frontal::ui::modal::ModalAction::Close,
        )
        .with_description("Extremely long description that should truncate cleanly")
        .with_badge("OpenCode")
        .with_shortcut("2h ago")
        .with_deletable(true),
        SelectItem::new(
            "2",
            "AnotherLongItemTitleHere",
            frontal::ui::modal::ModalAction::Close,
        )
        .with_badge("Agy")
        .with_shortcut("Ctrl+Shift+X"),
    ];
    let mut dialog = DialogSelect::new("Resume", items);
    dialog.pending_delete = Some("1".to_string());

    for width in [12u16, 18u16] {
        let backend = TestBackend::new(width, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| Modal::Select(dialog.clone()).render(f, Rect::new(0, 0, width, 8)))
            .unwrap();
        let row: String = (0..width)
            .map(|x| terminal.backend().buffer()[(x, 3)].symbol())
            .collect();
        assert_eq!(row.chars().count(), width as usize);
    }
}
