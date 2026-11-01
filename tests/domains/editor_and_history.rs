use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use ratatui::Terminal;

use frontal::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use frontal::ui::editor::{ExtmarkKind, PromptEditor, TriggerKind};
use frontal::ui::history::PromptHistory;

use crate::common::{press_key, render_app, test_app, TempDirGuard};

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
    let temp_guard = TempDirGuard::new("history_wal_test");
    let db_path = temp_guard.path().join("history.db");

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
fn test_background_file_search_worker_large_dataset() {
    let worker = FileSearchWorker::new();
    let temp_guard = TempDirGuard::new("search_worker_test");

    for i in 0..100 {
        let fpath = temp_guard.path().join(format!("file_{:04}.txt", i));
        let _ = std::fs::write(fpath, b"data");
    }

    worker.start_background_scan(temp_guard.path().to_path_buf(), 500);

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
}

#[test]
fn test_prompt_history_deterministic_flush() {
    let temp_guard = TempDirGuard::new("flush_test");
    let db_path = temp_guard.path().join("history.db");

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
}

#[test]
fn test_prompt_editor_visual_containment_and_shrink_no_residual_leakage() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    for i in 0..5 {
        app.editor
            .insert_str(&format!("multiline text row {}\n", i));
    }
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();

    let buf_before = terminal.backend().buffer().clone();
    let mut found_multiline = false;
    for y in 0..24 {
        let mut row_str = String::new();
        for x in 0..80 {
            row_str.push_str(buf_before.cell((x, y)).unwrap().symbol());
        }
        if row_str.contains("multiline text row 0") {
            found_multiline = true;
            break;
        }
    }
    assert!(found_multiline);

    app.editor.clear();
    app.editor.insert_str("single line prompt");
    terminal.draw(|f| app.render(f)).unwrap();

    let buf_after = terminal.backend().buffer().clone();
    for y in 0..24 {
        let mut row_str = String::new();
        for x in 0..80 {
            row_str.push_str(buf_after.cell((x, y)).unwrap().symbol());
        }
        assert!(
            !row_str.contains("multiline text row"),
            "residual character leaked on row {}: {}",
            y,
            row_str
        );
    }
}

#[test]
fn test_prompt_editor_dynamic_scroll_and_cursor_containment() {
    let (mut app, _) = test_app();
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    for i in 0..10 {
        app.editor.insert_str(&format!("line {}\n", i));
    }
    app.editor.insert_str("last line");

    terminal.draw(|f| app.render(f)).unwrap();

    let pos = terminal.get_cursor_position().unwrap();
    assert!(pos.y > 0 && pos.y < 10);
    assert!(pos.x > 0 && pos.x < 80);
}

#[test]
fn test_adversarial_paste_and_selection_sanitization() {
    let mut editor = PromptEditor::new();
    let malformed = "\x1b[31;1mLine 1\x1b[0m\r\nLine 2\rLine 3\x00\x08\r\nLine 4\rLine 5\r\nLine 6\x1b]9;notification\x07";
    editor.insert_paste(malformed);

    assert_eq!(editor.line_count(), 1);
    let pill_text = editor.text();
    assert!(pill_text.starts_with("[Pasted #1: ~6 lines]"));

    let resolved = editor.take_submitted_expanded().unwrap();
    assert!(!resolved.contains('\r'));
    assert!(!resolved.contains('\x1b'));
    assert!(!resolved.contains('\x00'));
    assert!(!resolved.contains('\x08'));
    assert!(resolved.contains("Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6"));

    let (mut app, _) = test_app();
    let backend = TestBackend::new(20, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    app.editor.insert_str(&resolved);
    terminal.draw(|f| app.render(f)).unwrap();

    let pos = terminal.get_cursor_position().unwrap();
    assert!(pos.y < 6);
    assert!(pos.x < 20);
}

#[test]
fn test_prompt_editor_undo_grouped_words_and_restoration() {
    let mut editor = PromptEditor::new();
    for c in "hello world".chars() {
        editor.insert_char(c);
    }
    assert_eq!(editor.text(), "hello world");

    assert!(editor.undo());
    assert_eq!(editor.text(), "hello ");

    assert!(editor.undo());
    assert_eq!(editor.text(), "hello");

    assert!(editor.undo());
    assert_eq!(editor.text(), "");

    assert!(!editor.undo());
}

#[test]
fn test_prompt_editor_select_all_and_atomic_replacement() {
    let mut editor = PromptEditor::new();
    editor.insert_str("conteúdo inicial");
    editor.select_all();
    assert!(editor.has_selection());
    assert_eq!(editor.selected_text(), Some("conteúdo inicial".to_string()));

    editor.insert_char('x');
    assert_eq!(editor.text(), "x");
    assert!(!editor.has_selection());

    assert!(editor.undo());
    assert_eq!(editor.text(), "conteúdo inicial");
}

#[test]
fn test_prompt_editor_delete_to_line_start_ctrl_u() {
    let mut editor = PromptEditor::new();
    editor.insert_str("linha 1\nprefixo sufixo");
    assert_eq!(editor.cursor_coordinates(), (1, 14));

    assert!(editor.delete_to_line_start());
    assert_eq!(editor.text(), "linha 1\n");
    assert_eq!(editor.cursor_coordinates(), (1, 0));

    assert!(editor.undo());
    assert_eq!(editor.text(), "linha 1\nprefixo sufixo");
}

#[test]
fn test_prompt_editor_range_deletion_preserves_extmarks_and_paste_registry() {
    let mut editor = PromptEditor::new();
    editor.insert_str("inicio ");
    let pill_data = "p1\np2\np3\np4\np5\np6";
    editor.insert_paste(pill_data);
    editor.insert_str(" fim");

    editor.select_all();
    assert_eq!(
        editor.selected_text(),
        Some(format!("inicio {} fim", pill_data))
    );

    editor.backspace();
    assert_eq!(editor.text(), "");
    assert_eq!(editor.extmarks().len(), 0);

    assert!(editor.undo());
    assert!(editor.text().contains("[Pasted #1: ~6 lines]"));
    assert_eq!(editor.extmarks().len(), 1);
    assert_eq!(
        editor.take_submitted_expanded(),
        Some(format!("inicio {} fim", pill_data))
    );
}

#[test]
fn test_app_shortcuts_ctrl_a_ctrl_c_backspace_undo_and_ctrl_u() {
    let (mut app, _) = test_app();
    app.editor.insert_str("primeira linha\nsegunda linha");

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert!(app.editor.has_selection());
    assert_eq!(
        app.editor.selected_text(),
        Some("primeira linha\nsegunda linha".to_string())
    );

    let (terminal, _) = render_app(&mut app, 80, 24);
    let buf = terminal.backend().buffer();
    let has_reversed = buf
        .content()
        .iter()
        .any(|c| c.symbol() == "p" && c.modifier.contains(Modifier::REVERSED));
    assert!(has_reversed);

    app.handle_terminal_event(press_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(
        app.clipboard.get_text(),
        Some("primeira linha\nsegunda linha".to_string())
    );
    assert!(!app.editor.has_selection());
    assert_eq!(app.editor.text(), "primeira linha\nsegunda linha");

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    app.handle_terminal_event(press_key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.editor.text(), "");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(app.editor.text(), "primeira linha\nsegunda linha");

    app.editor.move_end();
    app.handle_terminal_event(press_key(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(app.editor.text(), "primeira linha\n");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(app.editor.text(), "primeira linha\nsegunda linha");

    app.handle_terminal_event(press_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.editor.text(), "");

    app.handle_terminal_event(press_key(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(app.editor.text(), "primeira linha\nsegunda linha");

    app.handle_terminal_event(press_key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert!(app.editor.has_selection());
    app.handle_terminal_event(press_key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.editor.has_selection());
}
