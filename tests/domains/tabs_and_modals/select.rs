use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use frontal::ui::modal::{DialogSelect, Modal, ModalAction, ModalStack, SelectItem, SelectResult};
use frontal::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, TEXT_PRIMARY};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn test_dialog_select_filtering_across_fields() {
    let items = vec![
        SelectItem::new("item_1", "Checkout Branch", "git checkout")
            .with_category("Git")
            .with_description("Switch current branch")
            .with_shortcut("Ctrl+B")
            .with_badge("VCS"),
        SelectItem::new("item_2", "Search Symbols", "find symbols")
            .with_category("Editor")
            .with_description("Global code search")
            .with_shortcut("Ctrl+S"),
        SelectItem::new("item_3", "Format File", "cargo fmt")
            .with_category("Build")
            .with_shortcut("Alt+F"),
    ];

    let mut dialog =
        DialogSelect::new("Select Action", items).with_placeholder("Type to search...");
    assert_eq!(dialog.filtered_indices().len(), 3);

    dialog.query = "branch".to_string();
    assert_eq!(dialog.filtered_indices(), vec![0]);

    dialog.query = "git".to_string();
    assert_eq!(dialog.filtered_indices(), vec![0]);

    dialog.query = "global".to_string();
    assert_eq!(dialog.filtered_indices(), vec![1]);

    dialog.query = "alt+f".to_string();
    assert_eq!(dialog.filtered_indices(), vec![2]);

    dialog.query = "nonexistent".to_string();
    assert!(dialog.filtered_indices().is_empty());
    assert!(dialog.selected_item().is_none());
}

#[test]
fn test_dialog_select_navigation_keyboard_keys() {
    let items: Vec<SelectItem<usize>> = (0..20)
        .map(|i| SelectItem::new(format!("id_{}", i), format!("Title {}", i), i))
        .collect();

    let mut dialog = DialogSelect::new("Numbers", items);

    dialog.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 1);

    dialog.handle_key(key(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert_eq!(dialog.selected_idx, 2);

    dialog.handle_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert_eq!(dialog.selected_idx, 3);

    dialog.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 2);

    dialog.handle_key(key(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(dialog.selected_idx, 1);

    dialog.handle_key(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert_eq!(dialog.selected_idx, 0);

    dialog.handle_key(key(KeyCode::PageDown, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 5);

    dialog.handle_key(key(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 0);

    dialog.handle_key(key(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 19);

    dialog.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 0);

    let res_enter = dialog.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        res_enter,
        SelectResult::Selected(SelectItem::new("id_0", "Title 0", 0))
    );

    let res_esc = dialog.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(res_esc, SelectResult::Cancel);

    dialog.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(dialog.query.is_empty());

    dialog.handle_key(key(KeyCode::Char('1'), KeyModifiers::NONE));
    assert_eq!(dialog.query, "1");
    assert_eq!(dialog.selected_idx, 0);

    dialog.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(dialog.query.is_empty());
}

#[test]
fn test_dialog_select_modal_stack_integration() {
    let mut stack = ModalStack::new();
    let items = vec![
        SelectItem::new(
            "opt_1",
            "First Action",
            ModalAction::ExecuteCommand("run_1".into()),
        ),
        SelectItem::new(
            "opt_2",
            "Second Action",
            ModalAction::ExecuteCommand("run_2".into()),
        ),
    ];

    let dialog = DialogSelect::new("Stack Select", items);
    stack.push(Modal::Select(dialog));
    assert!(!stack.is_empty());

    let area = stack.top().unwrap().popup_area(Rect::new(0, 0, 100, 40));
    assert!(area.width >= 42 && area.height >= 12);

    let act = stack.handle_key_event(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(act, ModalAction::ExecuteCommand("run_1".into()));

    let act_close = stack.handle_key_event(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(act_close, ModalAction::Close);
    assert!(stack.is_empty());
}

#[test]
fn test_dialog_select_rendering_and_viewport_clipping() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let items: Vec<SelectItem<usize>> = (0..25)
        .map(|i| {
            let cat = if i < 10 { "Group A" } else { "Group B" };
            SelectItem::new(format!("item_{:02}", i), format!("Entry {:02}", i), i)
                .with_category(cat)
                .with_badge("TAG")
        })
        .collect();

    let mut dialog = DialogSelect::new("Categorized List", items);
    terminal
        .draw(|f| dialog.render(f, Rect::new(5, 2, 70, 12)))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut found_prompt = false;
    let mut found_selection = false;

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            if cell.symbol() == ">" && cell.fg == ACCENT_HIGHLIGHT {
                found_prompt = true;
            }
            if cell.bg == ACCENT_RUBY && cell.fg == TEXT_PRIMARY {
                found_selection = true;
            }
        }
    }

    assert!(found_prompt);
    assert!(found_selection);

    dialog.selected_idx = 24;
    terminal
        .draw(|f| dialog.render(f, Rect::new(5, 2, 70, 12)))
        .unwrap();

    let buffer_scrolled = terminal.backend().buffer();
    let mut found_scrolled_entry = false;
    for y in 0..buffer_scrolled.area.height {
        let mut row_text = String::new();
        for x in 0..buffer_scrolled.area.width {
            row_text.push_str(buffer_scrolled[(x, y)].symbol());
        }
        if row_text.contains("Entry 24") {
            found_scrolled_entry = true;
            break;
        }
    }
    assert!(found_scrolled_entry);

    dialog.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 23);
    terminal
        .draw(|f| dialog.render(f, Rect::new(5, 2, 70, 12)))
        .unwrap();

    let buffer_up = terminal.backend().buffer();
    let mut found_entry_23 = false;
    for y in 0..buffer_up.area.height {
        let mut row_text = String::new();
        for x in 0..buffer_up.area.width {
            row_text.push_str(buffer_up[(x, y)].symbol());
        }
        if row_text.contains("Entry 23") {
            found_entry_23 = true;
            break;
        }
    }
    assert!(found_entry_23);

    dialog.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(dialog.selected_idx, 0);
    terminal
        .draw(|f| dialog.render(f, Rect::new(5, 2, 70, 12)))
        .unwrap();

    let buffer_home = terminal.backend().buffer();
    let mut found_group_a = false;
    for y in 0..buffer_home.area.height {
        let mut row_text = String::new();
        for x in 0..buffer_home.area.width {
            row_text.push_str(buffer_home[(x, y)].symbol());
        }
        if row_text.contains("[Group A]") {
            found_group_a = true;
            break;
        }
    }
    assert!(found_group_a);
}

#[test]
fn test_dialog_select_adversarial_dimensions_and_multibyte_utf8() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let items = vec![SelectItem::new(
        "utf_1",
        "Opção de Configuração com Caracteres Especiais ●",
        "val1",
    )
    .with_category("Acentuação")
    .with_description("Descrição longa com símbolos Unicode ▸ █")];
    let dialog = DialogSelect::new("Unicode Dialog", items);

    terminal
        .draw(|f| dialog.render(f, Rect::new(0, 0, 1, 1)))
        .unwrap();

    terminal
        .draw(|f| dialog.render(f, Rect::new(0, 0, 10, 4)))
        .unwrap();

    terminal
        .draw(|f| dialog.render(f, Rect::new(0, 0, 80, 24)))
        .unwrap();
}
