use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

use frontal::core::model_spec::{format_compact_tokens, ContextSemaphore, ModelSpec};
use frontal::protocol::TokenUsage;

use crate::common::test_app;

fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf.cell((x, y)).unwrap().symbol())
        .collect::<Vec<_>>()
        .join("")
}

fn find_dot_color_on_row(buf: &ratatui::buffer::Buffer, y: u16) -> Option<Color> {
    (0..buf.area.width).find_map(|x| {
        let cell = buf.cell((x, y)).unwrap();
        if cell.symbol() == "●" {
            Some(cell.fg)
        } else {
            None
        }
    })
}

#[test]
fn test_footer_context_gauge_visibility_and_sidebar_coupling() {
    let (mut app, _) = test_app();
    app.cache.push_user_message("hello");
    app.last_turn_usage = Some(TokenUsage {
        input_tokens: 300_000,
        output_tokens: 100_000,
        thinking_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 400_000,
    });

    let mut wide_terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    wide_terminal.draw(|f| app.render(f)).unwrap();
    let wide_buf = wide_terminal.backend().buffer();
    let wide_footer = row_text(wide_buf, 23);
    assert!(wide_footer.contains("Ctrl+P: Commands"));
    assert!(!wide_footer.contains("●"));
    let sidebar_gauge_row = (0..wide_buf.area.height)
        .map(|y| row_text(wide_buf, y))
        .find(|line| line.contains("● 40% used (400k/750k)"))
        .expect("expected semaphore gauge in sidebar");
    assert!(sidebar_gauge_row.contains("● 40% used (400k/750k)"));

    let mut narrow_terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    narrow_terminal.draw(|f| app.render(f)).unwrap();
    let narrow_footer = row_text(narrow_terminal.backend().buffer(), 23);
    assert!(narrow_footer.contains("Ctrl+P: Commands"));
    assert!(narrow_footer.contains("● 40% (400k/750k)"));

    app.sidebar_visible = false;
    wide_terminal.draw(|f| app.render(f)).unwrap();
    let hidden_sb_footer = row_text(wide_terminal.backend().buffer(), 23);
    assert!(hidden_sb_footer.contains("Ctrl+P: Commands"));
    assert!(hidden_sb_footer.contains("● 40% (400k/750k)"));
}

#[test]
fn test_footer_context_gauge_semaphore_green_yellow_red() {
    let spec = ModelSpec::resolve(&["gemini-3.8-flash"], None, &[]);
    assert_eq!(spec.semaphore(200_000), Some(ContextSemaphore::Green));
    assert_eq!(spec.semaphore(550_000), Some(ContextSemaphore::Yellow));
    assert_eq!(spec.semaphore(760_000), Some(ContextSemaphore::Red));

    let (mut app, _) = test_app();
    app.cache.push_user_message("hello");

    let cases = [
        (200_000, Color::Green, "20%"),
        (550_000, Color::Yellow, "55%"),
        (760_000, Color::Red, "76%"),
    ];

    for (tokens, expected_color, expected_pct) in cases {
        app.last_turn_usage = Some(TokenUsage {
            input_tokens: tokens,
            output_tokens: 0,
            thinking_tokens: 0,
            cache_read_tokens: 0,
            total_tokens: tokens,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let buf = terminal.backend().buffer();
        let footer = row_text(buf, 23);
        assert!(footer.contains(expected_pct), "footer was: {}", footer);
        assert_eq!(find_dot_color_on_row(buf, 23), Some(expected_color));
    }
}

#[test]
fn test_format_compact_tokens_boundary_rounding() {
    assert_eq!(format_compact_tokens(0), "0");
    assert_eq!(format_compact_tokens(999), "999");
    assert_eq!(format_compact_tokens(1_000), "1k");
    assert_eq!(format_compact_tokens(1_500), "1.5k");
    assert_eq!(format_compact_tokens(9_949), "9.9k");
    assert_eq!(format_compact_tokens(9_950), "10k");
    assert_eq!(format_compact_tokens(9_999), "10k");
    assert_eq!(format_compact_tokens(10_000), "10k");
    assert_eq!(format_compact_tokens(999_499), "999k");
    assert_eq!(format_compact_tokens(999_500), "1M");
    assert_eq!(format_compact_tokens(999_999), "1M");
    assert_eq!(format_compact_tokens(1_000_000), "1M");
    assert_eq!(format_compact_tokens(1_200_000), "1.2M");
}
