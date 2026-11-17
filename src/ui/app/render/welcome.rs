use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, TAB_ACTIVE_BG, TEXT_MUTED, TEXT_PRIMARY};

const HERO_INPUT_MAX_WIDTH: u16 = 76;
const HERO_SHORTCUTS: [(&str, &str); 6] = [
    ("/resume", "Sessions"),
    ("/model", "Model"),
    ("/new", "New tab"),
    ("/quota", "Usage"),
    ("/diff", "Diff"),
    ("Ctrl+P", "Palette"),
];

fn build_pill_spans(cmd: &'static str, desc: Option<&'static str>) -> Vec<Span<'static>> {
    let pill_style = Style::default()
        .bg(TAB_ACTIVE_BG)
        .fg(ACCENT_HIGHLIGHT)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::styled(format!(" {} ", cmd), pill_style)];
    if let Some(label) = desc {
        spans.push(Span::styled(
            format!(" {}", label),
            Style::default().fg(TEXT_MUTED),
        ));
    }
    spans
}

fn spans_display_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}

fn build_pill_lines(max_width: u16) -> Vec<Line<'static>> {
    let width_budget = max_width as usize;
    let include_desc = width_budget >= 56;
    let gap = "   ";
    let gap_w = gap.len();

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_row: Vec<Span<'static>> = Vec::new();
    let mut current_w = 0usize;

    for &(cmd, desc) in &HERO_SHORTCUTS {
        let mut pill = build_pill_spans(cmd, if include_desc { Some(desc) } else { None });
        let pill_w = spans_display_width(&pill);
        let needed = if current_row.is_empty() {
            pill_w
        } else {
            gap_w + pill_w
        };
        if !current_row.is_empty() && current_w + needed > width_budget {
            rows.push(std::mem::take(&mut current_row));
            current_w = 0;
        }
        if !current_row.is_empty() {
            current_row.push(Span::raw(gap));
            current_w += gap_w;
        }
        current_w += pill_w;
        current_row.append(&mut pill);
    }

    if !current_row.is_empty() {
        rows.push(current_row);
    }

    rows.into_iter().map(Line::from).collect()
}

impl App {
    pub(crate) fn is_hero_welcome_active(&self, area: Rect) -> bool {
        self.cache.blocks_count() == 0
            && !self.waiting_for_response
            && area.width >= 50
            && area.height >= 16
    }

    pub(crate) fn render_welcome_hero(&self, frame: &mut Frame, body_area: Rect) -> Rect {
        let header_lines = vec![
            Line::from(vec![
                Span::styled(
                    "frontal",
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  v{} ", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(TEXT_MUTED),
                ),
                Span::styled(
                    format!(" {} ", self.backend.as_str()),
                    Style::default()
                        .bg(TAB_ACTIVE_BG)
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![Span::styled(
                "Async TUI for Antigravity & stream-json engines",
                Style::default().fg(TEXT_MUTED),
            )]),
        ];

        let pill_lines = build_pill_lines(body_area.width);
        let header_h = header_lines.len() as u16;
        let pills_h = pill_lines.len() as u16;
        let gap_h: u16 = if body_area.height >= header_h + pills_h + 7 {
            1
        } else {
            0
        };
        let overhead_h = header_h + pills_h + gap_h.saturating_mul(2);
        let max_input_lines = body_area.height.saturating_sub(overhead_h + 2).clamp(1, 8);
        let input_lines = (self.editor.line_count() as u16).clamp(1, max_input_lines);
        let input_h = (input_lines + 2).min(body_area.height.saturating_sub(overhead_h).max(3));
        let total_stack_h = (overhead_h + input_h).min(body_area.height);

        let top_pad = body_area.height.saturating_sub(total_stack_h) / 2;
        let stack_area = Rect {
            x: body_area.x,
            y: body_area.y.saturating_add(top_pad),
            width: body_area.width,
            height: total_stack_h,
        };

        let stack_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_h),
                Constraint::Length(gap_h),
                Constraint::Length(input_h),
                Constraint::Length(gap_h),
                Constraint::Length(pills_h),
            ])
            .split(stack_area);

        let header_widget = Paragraph::new(header_lines).alignment(Alignment::Center);
        frame.render_widget(header_widget, stack_chunks[0]);

        let input_w = body_area.width.min(HERO_INPUT_MAX_WIDTH);
        let input_x = body_area
            .x
            .saturating_add(body_area.width.saturating_sub(input_w) / 2);
        let input_area = Rect {
            x: input_x,
            y: stack_chunks[2].y,
            width: input_w,
            height: stack_chunks[2].height,
        };

        if let Some((cursor_x, cursor_y)) = self.render_input(frame, input_area) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        let pills_widget = Paragraph::new(pill_lines).alignment(Alignment::Center);
        frame.render_widget(pills_widget, stack_chunks[4]);

        input_area
    }

    pub(crate) fn render_welcome_compact(&self, frame: &mut Frame, chat_area: Rect) {
        let bold_hl = Style::default()
            .fg(ACCENT_HIGHLIGHT)
            .add_modifier(Modifier::BOLD);
        let welcome_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "frontal",
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        " v{} · {}",
                        env!("CARGO_PKG_VERSION"),
                        self.backend.as_str()
                    ),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]),
            Line::from(vec![Span::styled(
                "Async TUI for Antigravity & stream-json engines",
                Style::default().fg(TEXT_MUTED),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("/resume", bold_hl),
                Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                Span::styled("/model", bold_hl),
                Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                Span::styled("/new", bold_hl),
                Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                Span::styled("Ctrl+P", bold_hl),
            ]),
        ];
        let welcome_widget = Paragraph::new(welcome_lines).alignment(Alignment::Center);
        frame.render_widget(welcome_widget, chat_area);
    }
}
