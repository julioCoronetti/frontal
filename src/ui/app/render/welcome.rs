use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_RUBY, BG_USER_CONTAINER, TAB_ACTIVE_BG, TEXT_MUTED, TEXT_PRIMARY,
};

const HERO_INPUT_MAX_WIDTH: u16 = 76;

fn build_logo_lines() -> Vec<Line<'static>> {
    let left = Style::default().fg(ACCENT_RUBY);
    let right = Style::default().fg(ACCENT_HIGHLIGHT);
    let gap = Span::raw("  ");
    let shade = Style::default().fg(TAB_ACTIVE_BG);
    let left_on_shade = Style::default().fg(ACCENT_RUBY).bg(TAB_ACTIVE_BG);
    let right_on_shade = Style::default().fg(ACCENT_HIGHLIGHT).bg(TAB_ACTIVE_BG);
    let right_on_deep = Style::default().fg(ACCENT_HIGHLIGHT).bg(BG_USER_CONTAINER);

    vec![
        Line::from(vec![
            Span::styled("▄▀▀", left),
            gap.clone(),
            Span::styled("▄▄▄", left),
            gap.clone(),
            Span::styled("▄▄▄▄", left),
            gap.clone(),
            Span::styled("▄▄▄ ", left),
            gap.clone(),
            Span::styled("▄█▄", right),
            gap.clone(),
            Span::styled("▄▄▄ ", right),
            gap.clone(),
            Span::styled("█ ", right),
        ]),
        Line::from(vec![
            Span::styled("█▀ ", left),
            gap.clone(),
            Span::styled("█", left),
            Span::styled("▄", shade),
            Span::styled("▀", left_on_shade),
            gap.clone(),
            Span::styled("█", left),
            Span::styled("▄▄", shade),
            Span::styled("█", left),
            gap.clone(),
            Span::styled("█", left),
            Span::styled("▄▄", shade),
            Span::styled("█", left),
            gap.clone(),
            Span::styled(" █ ", right),
            gap.clone(),
            Span::styled("▄▄▄", right_on_deep),
            Span::styled("█", right),
            gap.clone(),
            Span::styled("█ ", right),
        ]),
        Line::from(vec![
            Span::styled("█  ", left),
            gap.clone(),
            Span::styled("█  ", left),
            gap.clone(),
            Span::styled("█", left),
            Span::styled("▄▄", left_on_shade),
            Span::styled("█", left),
            gap.clone(),
            Span::styled("█", left),
            Span::styled("▀▀", shade),
            Span::styled("█", left),
            gap.clone(),
            Span::styled(" █▄", right),
            gap.clone(),
            Span::styled("█", right),
            Span::styled("▄▄", right_on_shade),
            Span::styled("█", right),
            gap.clone(),
            Span::styled("█▄", right),
        ]),
    ]
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

        let palette_widget = Paragraph::new(Line::from(vec![
            Span::styled("Ctrl+P", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(": Palette", Style::default().fg(TEXT_MUTED)),
        ]))
        .alignment(Alignment::Right);
        frame.render_widget(palette_widget, chunks[1]);
    }

    pub(crate) fn render_welcome_compact(&self, frame: &mut Frame, chat_area: Rect) {
        let use_block_logo = chat_area.width >= 35 && chat_area.height >= 5;
        let needed_with_pad = if use_block_logo { 5 } else { 3 };
        let mut lines = Vec::new();
        if chat_area.height >= needed_with_pad {
            lines.push(Line::from(""));
        }
        if use_block_logo {
            lines.extend(build_logo_lines());
        } else {
            lines.push(Line::from(vec![
                Span::styled("fron", Style::default().fg(ACCENT_RUBY)),
                Span::styled("tal", Style::default().fg(ACCENT_HIGHLIGHT)),
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
