use ratatui::backend::TestBackend;
use ratatui::Terminal;

use frontal::ui::cache::BlockKind;
use frontal::ui::theme::{ACCENT_HIGHLIGHT, BG_SIDEBAR};

use crate::common::test_app;

#[test]
fn test_markdown_output_rendering_no_backticks_and_nested_indent() {
    let (mut app, _) = test_app();
    app.active_tab_mut().cache.push_user_message("check output");
    app.active_tab_mut().cache.append_streaming_delta(
        BlockKind::Assistant,
        "# Title\n\nRun `cargo test`\n\n- Outer\n  - Inner",
    );
    app.active_tab_mut().cache.consolidate_active_turn(None);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    let rendered: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(rendered.contains("cargo test") && !rendered.contains("`cargo test`"));
    assert!(rendered.contains("• Outer") && rendered.contains("  ◦ Inner"));
    assert!(buf
        .content()
        .iter()
        .any(|c| c.bg == BG_SIDEBAR && c.fg == ACCENT_HIGHLIGHT));
}
