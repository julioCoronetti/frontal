use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, TEXT_MUTED, TEXT_PRIMARY};

const HERO_INPUT_MAX_WIDTH: u16 = 76;

const GEM_PIXELS: [[Option<(u8, u8, u8)>; 6]; 6] = [
    [
        None,
        Some((94, 18, 35)),
        Some((230, 43, 67)),
        Some((230, 43, 67)),
        Some((198, 25, 51)),
        None,
    ],
    [
        Some((102, 18, 37)),
        Some((124, 18, 41)),
        Some((255, 59, 80)),
        Some((255, 59, 80)),
        Some((177, 12, 41)),
        Some((198, 25, 51)),
    ],
    [
        Some((79, 18, 34)),
        Some((113, 17, 40)),
        Some((86, 18, 35)),
        Some((177, 12, 41)),
        Some((177, 12, 41)),
        Some((105, 16, 37)),
    ],
    [
        Some((91, 19, 38)),
        Some((86, 18, 35)),
        Some((86, 18, 35)),
        Some((177, 12, 41)),
        Some((105, 16, 37)),
        Some((106, 16, 37)),
    ],
    [
        Some((92, 18, 37)),
        Some((105, 16, 37)),
        Some((105, 16, 37)),
        Some((102, 19, 42)),
        Some((78, 18, 34)),
        Some((78, 18, 34)),
    ],
    [
        None,
        Some((102, 19, 42)),
        Some((102, 19, 42)),
        Some((78, 18, 34)),
        Some((134, 16, 44)),
        None,
    ],
];

const LETTER_ROW_COLORS: [Color; 4] = [
    ACCENT_HIGHLIGHT,
    Color::Rgb(218, 36, 61),
    ACCENT_PRIMARY,
    ACCENT_RUBY,
];

fn push_gem_row_spans(spans: &mut Vec<Span<'static>>, row_idx: usize) {
    if row_idx == 0 {
        spans.push(Span::raw("      "));
        return;
    }
    let py = (row_idx - 1) * 2;
    for (&top, &bot) in GEM_PIXELS[py].iter().zip(GEM_PIXELS[py + 1].iter()) {
        match (top, bot) {
            (Some((tr, tg, tb)), Some((br, bg_g, bb))) => {
                spans.push(Span::styled(
                    "▀",
                    Style::default()
                        .fg(Color::Rgb(tr, tg, tb))
                        .bg(Color::Rgb(br, bg_g, bb)),
                ));
            }
            (Some((tr, tg, tb)), None) => {
                spans.push(Span::styled(
                    "▀",
                    Style::default().fg(Color::Rgb(tr, tg, tb)),
                ));
            }
            (None, Some((br, bg_g, bb))) => {
                spans.push(Span::styled(
                    "▄",
                    Style::default().fg(Color::Rgb(br, bg_g, bb)),
                ));
            }
            (None, None) => {
                spans.push(Span::raw(" "));
            }
        }
    }
}

fn build_logo_lines() -> Vec<Line<'static>> {
    let f_rows = ["▄▄▄▄▄", "█    ", "█▀▀▀ ", "█    "];
    let r_rows = ["    ", "█▀▀▄", "█   ", "█   "];
    let n_rows = ["     ", "█▀▀▀▄", "█   █", "█   █"];
    let t_rows = [" ▄ ", "▀█▀", " █ ", " █▄"];
    let a_rows = ["    ", "▀▀▀█", "█▀▀█", "█▄▄█"];
    let l_rows = ["   ", "█  ", "█  ", "█▄▄"];

    (0..4)
        .map(|row_idx| {
            let fg = Style::default().fg(LETTER_ROW_COLORS[row_idx]);
            let mut spans = Vec::with_capacity(16);

            spans.push(Span::styled(f_rows[row_idx], fg));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(r_rows[row_idx], fg));
            spans.push(Span::raw(" "));
            push_gem_row_spans(&mut spans, row_idx);
            spans.push(Span::raw(" "));
            spans.push(Span::styled(n_rows[row_idx], fg));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(t_rows[row_idx], fg));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(a_rows[row_idx], fg));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(l_rows[row_idx], fg));
            Line::from(spans)
        })
        .collect()
}

impl App {
    pub(crate) fn is_hero_welcome_active(&self, area: Rect) -> bool {
        self.cache.blocks_count() == 0
            && !self.waiting_for_response
            && area.width >= 50
            && area.height >= 16
    }

    pub(crate) fn render_welcome_hero(&self, frame: &mut Frame, body_area: Rect) -> Rect {
        let logo_lines = build_logo_lines();
        let logo_h = logo_lines.len() as u16;
        let meta_h: u16 = 1;
        let gap_h: u16 = if body_area.height >= logo_h + meta_h + 6 {
            1
        } else {
            0
        };
        let overhead_h = logo_h + gap_h + meta_h;
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
                Constraint::Length(logo_h),
                Constraint::Length(gap_h),
                Constraint::Length(input_h),
                Constraint::Length(meta_h),
            ])
            .split(stack_area);

        let logo_widget = Paragraph::new(logo_lines).alignment(Alignment::Center);
        frame.render_widget(logo_widget, stack_chunks[0]);

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

        let meta_area = Rect {
            x: input_x,
            y: stack_chunks[3].y,
            width: input_w,
            height: stack_chunks[3].height,
        };
        self.render_hero_meta_bar(frame, meta_area);

        input_area
    }

    fn render_hero_meta_bar(&self, frame: &mut Frame, meta_area: Rect) {
        if meta_area.width < 20 || meta_area.height == 0 {
            return;
        }
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(18)])
            .split(meta_area);

        let cwd_widget = Paragraph::new(Line::from(vec![Span::styled(
            &self.cwd_display,
            Style::default().fg(TEXT_MUTED),
        )]));
        frame.render_widget(cwd_widget, chunks[0]);

        let commands_widget = Paragraph::new(Line::from(vec![
            Span::styled("Ctrl+P", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(": Commands", Style::default().fg(TEXT_MUTED)),
        ]))
        .alignment(Alignment::Right);
        frame.render_widget(commands_widget, chunks[1]);
    }

    pub(crate) fn render_welcome_compact(&self, frame: &mut Frame, chat_area: Rect) {
        let use_block_logo = chat_area.width >= 35 && chat_area.height >= 6;
        let needed_with_pad = if use_block_logo { 6 } else { 3 };
        let mut lines = Vec::new();
        if chat_area.height >= needed_with_pad {
            lines.push(Line::from(""));
        }
        if use_block_logo {
            lines.extend(build_logo_lines());
        } else {
            lines.push(Line::from(vec![
                Span::styled("F", Style::default().fg(ACCENT_HIGHLIGHT)),
                Span::styled("ron", Style::default().fg(ACCENT_PRIMARY)),
                Span::styled("tal", Style::default().fg(ACCENT_RUBY)),
            ]));
        }
        lines.push(Line::from(vec![Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(TEXT_MUTED),
        )]));
        let welcome_widget = Paragraph::new(lines).alignment(Alignment::Center);
        frame.render_widget(welcome_widget, chat_area);
    }
}
