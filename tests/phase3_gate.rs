use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::protocol::SessionId;
use frontal::session::SessionManager;
use frontal::ui::app::{App, AppAction};
use frontal::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use frontal::ui::cache::{BlockKind, RenderCache};
use frontal::ui::clipboard::SystemClipboard;
use frontal::ui::editor::{ExtmarkKind, PromptEditor, TriggerKind};
use frontal::ui::history::PromptHistory;

#[test]
fn test_prompt_editor_multiline_and_navigation() {
    let mut editor = PromptEditor::new();
    assert!(editor.is_empty());

    editor.insert_str("primeira linha\n");
    editor.insert_str("segunda linha com acentuação: maçã e café 🦀\n");
    editor.insert_str("terceira linha");

    assert_eq!(editor.line_count(), 3);
    let (l, c) = editor.cursor_coordinates();
    assert_eq!(l, 2);
    assert_eq!(c, 14);

    editor.move_home();
    let (l, c) = editor.cursor_coordinates();
    assert_eq!(l, 2);
    assert_eq!(c, 0);

    editor.move_end();
    let (l, c) = editor.cursor_coordinates();
    assert_eq!(l, 2);
    assert_eq!(c, 14);

    assert!(editor.move_up());
    let (l, _) = editor.cursor_coordinates();
    assert_eq!(l, 1);

    assert!(editor.move_up());
    let (l, _) = editor.cursor_coordinates();
    assert_eq!(l, 0);

    assert!(!editor.move_up());

    assert!(editor.move_down());
    let (l, _) = editor.cursor_coordinates();
    assert_eq!(l, 1);
}

#[test]
fn test_atomic_extmarks_insertion_and_deletion() {
    let mut editor = PromptEditor::new();
    editor.insert_str("abra ");
    let id1 = editor.insert_extmark(
        ExtmarkKind::FileMention {
            path: "src/main.rs".to_string(),
        },
        "@src/main.rs ",
    );
    editor.insert_str("e execute ");
    let id2 = editor.insert_extmark(
        ExtmarkKind::SlashCommand {
            cmd: "build".to_string(),
        },
        "/build ",
    );

    assert_eq!(editor.extmarks().len(), 2);
    assert_eq!(editor.extmarks()[0].id, id1);
    assert_eq!(editor.extmarks()[1].id, id2);

    editor.backspace();
    assert_eq!(editor.extmarks().len(), 1);
    assert_eq!(editor.extmarks()[0].id, id1);
    assert!(!editor.text().contains("/build"));

    editor.set_cursor_char(0);
    editor.insert_str("por favor ");
    assert_eq!(editor.extmarks()[0].id, id1);
    assert!(editor.extmarks()[0].start_char > 0);

    editor.set_cursor_char(editor.extmarks()[0].start_char);
    editor.delete_forward();
    assert_eq!(editor.extmarks().len(), 0);
    assert!(!editor.text().contains("@src/main.rs"));
}

#[test]
fn test_paste_pill_collapse_and_reverse_expansion() {
    let mut editor = PromptEditor::new();
    let short_paste = "linha 1\nlinha 2\nlinha 3";
    editor.insert_paste(short_paste);
    assert_eq!(editor.extmarks().len(), 0);
    assert_eq!(editor.text(), short_paste);

    editor.clear();

    let long_paste_1 = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8";
    editor.insert_str("inicio: ");
    editor.insert_paste(long_paste_1);
    editor.insert_str(" meio: ");
    let long_paste_2 = "a\nb\nc\nd\ne\nf\ng";
    editor.insert_paste(long_paste_2);
    editor.insert_str(" fim.");

    assert_eq!(editor.extmarks().len(), 2);
    assert!(editor.text().contains("[Pasted #1: ~8 lines]"));
    assert!(editor.text().contains("[Pasted #2: ~7 lines]"));

    let expanded = editor.take_submitted_expanded().unwrap();
    assert!(expanded.starts_with("inicio: l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8 meio: "));
    assert!(expanded.ends_with(" fim."));
    assert!(expanded.contains("a\nb\nc\nd\ne\nf\ng"));
    assert!(editor.is_empty());
}

#[test]
fn test_autocomplete_popover_and_file_search() {
    let mut editor = PromptEditor::new();
    editor.insert_str("verifique @sr");
    let trigger = editor.active_trigger();
    assert!(trigger.is_some());
    let (kind, start, query) = trigger.unwrap();
    assert_eq!(kind, TriggerKind::File);
    assert_eq!(query, "sr");

    let mut searcher = FileSearchWorker::new();
    searcher.set_cached_files(vec![
        "src/lib.rs".to_string(),
        "src/main.rs".to_string(),
        "tests/phase1_gate.rs".to_string(),
    ]);
    let items = searcher.search(&query, 10);
    assert_eq!(items.len(), 2);

    let mut popover = AutocompletePopover::new();
    popover.show(kind, start, &query, items);
    assert!(popover.is_visible());
    assert_eq!(popover.selected_index(), 0);

    popover.select_next();
    assert_eq!(popover.selected_index(), 1);
    let selected = popover.selected_item().unwrap();
    assert_eq!(selected.label, "src/main.rs");

    editor.replace_range_with_extmark(
        popover.trigger_start_char,
        editor.cursor_char(),
        ExtmarkKind::FileMention {
            path: selected.label.clone(),
        },
        &selected.insert_text,
    );
    popover.close();
    assert!(!popover.is_visible());
    assert_eq!(editor.text(), "verifique @src/main.rs ");
}

#[test]
fn test_prompt_history_sqlite_wal_and_navigation() {
    let temp_dir = std::env::temp_dir().join(format!("frontal_test_{}", SessionId::new()));
    let db_path = temp_dir.join("history.db");

    {
        let mut history = PromptHistory::open(&db_path).unwrap();
        history.record_prompt("primeiro prompt");
        history.record_prompt("segundo prompt");
        history.record_prompt("segundo prompt");
        history.record_prompt("terceiro prompt");

        let p1 = history.prev_history("rascunho").unwrap();
        assert_eq!(p1, "terceiro prompt");
        let p2 = history.prev_history("rascunho").unwrap();
        assert_eq!(p2, "segundo prompt");
        let p3 = history.prev_history("rascunho").unwrap();
        assert_eq!(p3, "primeiro prompt");

        let n1 = history.next_history().unwrap();
        assert_eq!(n1, "segundo prompt");
        let n2 = history.next_history().unwrap();
        assert_eq!(n2, "terceiro prompt");
        let draft = history.next_history().unwrap();
        assert_eq!(draft, "rascunho");
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

    {
        let reopened = PromptHistory::open(&db_path).unwrap();
        assert!(reopened.entries().contains(&"primeiro prompt".to_string()));
        assert!(reopened.entries().contains(&"terceiro prompt".to_string()));
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_phase3_ui_refinements_on_test_backend() {
    let session_manager = SessionManager::new();
    let session_id = SessionId::new();
    let mut app = App::new(session_manager, session_id);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut cache = RenderCache::new();
    cache.append_streaming_delta(
        BlockKind::Assistant,
        "# Titulo Primario\n## Subtitulo Secundario\nTexto normal aqui.",
    );
    cache.consolidate_active_turn(None);
    app.cache = cache;

    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let rendered_text = buffer
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(!rendered_text.contains("# Titulo"));
    assert!(!rendered_text.contains("## Subtitulo"));
    assert!(rendered_text.contains("Titulo Primario"));
    assert!(rendered_text.contains("Subtitulo Secundario"));

    let initial_blocks = app.cache.blocks_count();
    app.waiting_for_response = true;
    app.spinner_tick = 8;
    terminal.draw(|f| app.render(f)).unwrap();
    let buffer2 = terminal.backend().buffer();
    let text2 = buffer2
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    assert!(text2.contains("Thinking / Waiting for response..."));
    assert_eq!(app.cache.blocks_count(), initial_blocks);

    let down_event = Event::Key(KeyEvent {
        code: KeyCode::Down,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.handle_terminal_event(down_event);
    assert_eq!(app.cache.blocks_count(), initial_blocks);
}

#[test]
fn test_extmark_cursor_boundary_invariance() {
    let mut editor = PromptEditor::new();
    editor.insert_str("prefix ");
    editor.insert_extmark(
        ExtmarkKind::FileMention {
            path: "test.rs".to_string(),
        },
        "@test.rs",
    );
    editor.insert_str(" suffix\nline 2 middle");

    let ext = editor.extmarks()[0].clone();

    editor.move_up();
    let c = editor.cursor_char();
    assert!(c <= ext.start_char || c >= ext.end_char);

    editor.move_home();
    let c = editor.cursor_char();
    assert!(c <= ext.start_char || c >= ext.end_char);

    editor.move_end();
    let c = editor.cursor_char();
    assert!(c <= ext.start_char || c >= ext.end_char);

    editor.move_down();
    let c = editor.cursor_char();
    assert!(c <= ext.start_char || c >= ext.end_char);
}

#[test]
fn test_paste_pill_preservation_across_history_navigation() {
    let session_manager = SessionManager::new();
    let session_id = SessionId::new();
    let mut app = App::new(session_manager, session_id);
    app.history.record_prompt("historical command");

    let big_paste = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8";
    app.editor.insert_str("prefix: ");
    app.editor.insert_paste(big_paste);
    app.editor.insert_str(" :suffix");

    assert_eq!(app.editor.extmarks().len(), 1);
    let orig_text = app.editor.text();
    assert!(orig_text.contains("[Pasted #1: ~8 lines]"));

    let up = Event::Key(KeyEvent {
        code: KeyCode::Up,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.handle_terminal_event(up);
    assert_eq!(app.editor.text(), "historical command");

    let down = Event::Key(KeyEvent {
        code: KeyCode::Down,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.handle_terminal_event(down);
    assert_eq!(app.editor.text(), orig_text);
    assert_eq!(app.editor.extmarks().len(), 1);

    let expanded = app.editor.take_submitted_expanded().unwrap();
    assert!(expanded.contains("line 1\nline 2\nline 3"));
}

#[test]
fn test_local_slash_commands_execution() {
    let session_manager = SessionManager::new();
    let session_id = SessionId::new();
    let mut app = App::new(session_manager, session_id);

    app.editor.insert_str("/quit");
    let enter = Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, Some(AppAction::Quit));
    assert!(app.should_quit);

    app.should_quit = false;
    app.cache.push_user_message("something");
    app.editor.insert_str("/clear");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert_eq!(app.cache.blocks_count(), 0);

    app.editor.insert_str("/help");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert!(app.cache.blocks_count() > 0);

    app.editor.insert_str("/interrupt");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, Some(AppAction::Interrupt));

    app.editor.insert_str("/model gemini");
    let act = app.handle_terminal_event(enter.clone());
    assert_eq!(act, None);
    assert!(!app.waiting_for_response);

    let _ = app.cache.upsert_tool_call(
        Some(1),
        "test_tool",
        frontal::protocol::StepState::Done,
        None,
        Some("output"),
    );
    let backtab = Event::Key(KeyEvent {
        code: KeyCode::BackTab,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    let act = app.handle_terminal_event(backtab);
    assert_eq!(act, None);
}

#[test]
fn test_background_file_search_worker_large_dataset() {
    let worker = FileSearchWorker::new();
    let temp_dir = std::env::temp_dir().join(format!("search_test_{}", SessionId::new()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    for i in 0..100 {
        let fpath = temp_dir.join(format!("file_{:04}.txt", i));
        let _ = std::fs::write(fpath, b"data");
    }

    worker.start_background_scan(temp_dir.clone(), 500);

    let mut found = false;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let files = worker.cached_files();
        if files.len() >= 100 {
            found = true;
            break;
        }
    }
    assert!(found);

    let items = worker.search("0042", 5);
    assert_eq!(items.len(), 1);
    assert!(items[0].label.contains("file_0042.txt"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_prompt_history_deterministic_flush() {
    let temp_dir = std::env::temp_dir().join(format!("flush_test_{}", SessionId::new()));
    let db_path = temp_dir.join("history.db");

    {
        let mut history = PromptHistory::open(&db_path).unwrap();
        history.record_prompt("unflushed line 1");
        history.record_prompt("unflushed line 2");
        history.flush();
    }

    {
        let reopened = PromptHistory::open(&db_path).unwrap();
        assert!(reopened.entries().contains(&"unflushed line 1".to_string()));
        assert!(reopened.entries().contains(&"unflushed line 2".to_string()));
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_system_clipboard_osc52_generation() {
    let payload = "hello world";
    let base64 = SystemClipboard::encode_base64(payload.as_bytes());
    assert_eq!(base64, "aGVsbG8gd29ybGQ=");

    let osc = SystemClipboard::format_osc52(payload);
    assert_eq!(osc, "\x1b]52;c;aGVsbG8gd29ybGQ=\x07");
}
