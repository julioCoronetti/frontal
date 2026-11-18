use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

const HERO_INPUT_MAX_WIDTH: u16 = 76;
const LOGO_GLYPHS: [&[&str]; 7] = [
    &[
        "████████",
        "██      ",
        "██      ",
        "██████  ",
        "██      ",
        "██      ",
        "██      ",
    ],
    &[
        "        ",
        "        ",
        "██████  ",
        "██   ██ ",
        "██      ",
        "██      ",
        "██      ",
    ],
    &[
        "        ",
        "        ",
        " █████  ",
        "██   ██ ",
        "██   ██ ",
        "██   ██ ",
        " █████  ",
    ],
    &[
        "        ",
        "        ",
        "██████  ",
        "██   ██ ",
        "██   ██ ",
        "██   ██ ",
        "██   ██ ",
    ],
    &[
        "  ██    ",
        "  ██    ",
        "██████  ",
        "  ██    ",
        "  ██    ",
        "  ██    ",
        "   ████ ",
    ],
    &[
        "        ",
        "        ",
        " █████  ",
        "     ██ ",
        " ██████ ",
        "██   ██ ",
        " ██████ ",
    ],
    &[
        "██      ",
        "██      ",
        "██      ",
        "██      ",
        "██      ",
        "██      ",
        " ███    ",
    ],
];

fn quadrant_char(tl: bool, tr: bool, bl: bool, br: bool) -> char {
    match (tl, tr, bl, br) {
        (false, false, false, false) => ' ',
        (true, false, false, false) => '▘',
        (false, true, false, false) => '▝',
        (false, false, true, false) => '▖',
        (false, false, false, true) => '▗',
        (true, true, false, false) => '▀',
        (false, false, true, true) => '▄',
        (true, false, true, false) => '▌',
        (false, true, false, true) => '▐',
        (true, false, false, true) => '▚',
        (false, true, true, false) => '▞',
        (true, true, true, false) => '▛',
        (true, true, false, true) => '▜',
        (true, false, true, true) => '▙',
        (false, true, true, true) => '▟',
        (true, true, true, true) => '█',
    }
}

fn glyph_cell(bmp: &[Vec<u8>], cy: usize, cx: usize) -> (char, Style) {
    let face = Style::default().fg(ACCENT_PRIMARY);
    let face_on_shadow = Style::default().fg(ACCENT_PRIMARY).bg(BORDER_SUBTLE);
    let shadow = Style::default().fg(BORDER_SUBTLE);

    let vals = [
        bmp[cy][cx],
        bmp[cy][cx + 1],
        bmp[cy + 1][cx],
        bmp[cy + 1][cx + 1],
    ];
    let has_face = vals.contains(&1);
    let has_shadow = vals.contains(&2);
    let has_empty = vals.contains(&0);

    if has_face {
        let ch = quadrant_char(vals[0] == 1, vals[1] == 1, vals[2] == 1, vals[3] == 1);
        let style = if has_shadow && !has_empty {
            face_on_shadow
        } else {
            face
        };
        (ch, style)
    } else if has_shadow {
        let ch = quadrant_char(vals[0] == 2, vals[1] == 2, vals[2] == 2, vals[3] == 2);
        (ch, shadow)
    } else {
        (' ', Style::default())
    }
}

fn build_glyph_cells(glyph: &[&str]) -> Vec<Vec<(char, Style)>> {
    let max_x = glyph
        .iter()
        .filter_map(|row| {
            row.chars()
                .enumerate()
                .filter(|&(_, ch)| ch == '█')
                .map(|(char_idx, _)| char_idx + 1)
                .last()
        })
        .max()
        .unwrap_or(2);
    let mut width = max_x + 1;
    if width % 2 == 1 {
        width += 1;
    }

    let mut bmp = vec![vec![0u8; width]; 8];
    for (y, row) in glyph.iter().enumerate().take(7) {
        for (x, ch) in row.chars().enumerate().take(width) {
            if ch == '█' {
                bmp[y][x] = 1;
            }
        }
    }
    for y in 0..7 {
        for x in 0..width.saturating_sub(1) {
            if bmp[y][x] == 1 && bmp[y + 1][x + 1] == 0 {
                bmp[y + 1][x + 1] = 2;
            }
        }
    }

    (0..8)
        .step_by(2)
        .map(|cy| {
            (0..width)
                .step_by(2)
                .map(|cx| glyph_cell(&bmp, cy, cx))
                .collect()
        })
        .collect()
}

fn build_logo_lines() -> Vec<Line<'static>> {
    let blocks: Vec<Vec<Vec<(char, Style)>>> =
        LOGO_GLYPHS.iter().map(|g| build_glyph_cells(g)).collect();

    (0..4)
        .map(|row_idx| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut active_style: Option<Style> = None;
            let mut buf = String::new();

            for (g_idx, block) in blocks.iter().enumerate() {
                if g_idx > 0 {
                    if active_style == Some(Style::default()) {
                        buf.push(' ');
                    } else {
                        if let Some(st) = active_style.take() {
                            spans.push(Span::styled(std::mem::take(&mut buf), st));
                        }
                        buf.push(' ');
                        active_style = Some(Style::default());
                    }
                }
                for &(ch, style) in &block[row_idx] {
                    if active_style == Some(style) {
                        buf.push(ch);
                    } else {
                        if let Some(st) = active_style.take() {
                            spans.push(Span::styled(std::mem::take(&mut buf), st));
                        }
                        buf.push(ch);
                        active_style = Some(style);
                    }
                }
            }
            if let Some(st) = active_style {
                if !buf.is_empty() {
                    spans.push(Span::styled(buf, st));
                }
            }
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

        let palette_widget = Paragraph::new(Line::from(vec![
            Span::styled("Ctrl+P", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(": Palette", Style::default().fg(TEXT_MUTED)),
        ]))
        .alignment(Alignment::Right);
        frame.render_widget(palette_widget, chunks[1]);
    }

    pub(crate) fn render_welcome_compact(&self, frame: &mut Frame, chat_area: Rect) {
        let use_block_logo = chat_area.width >= 34 && chat_area.height >= 6;
        let needed_with_pad = if use_block_logo { 6 } else { 3 };
        let mut lines = Vec::new();
        if chat_area.height >= needed_with_pad {
            lines.push(Line::from(""));
        }
        if use_block_logo {
            lines.extend(build_logo_lines());
        } else {
            lines.push(Line::from(vec![Span::styled(
                "Frontal",
                Style::default().fg(ACCENT_PRIMARY),
            )]));
        }
        lines.push(Line::from(vec![Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(TEXT_MUTED),
        )]));
        let welcome_widget = Paragraph::new(lines).alignment(Alignment::Center);
        frame.render_widget(welcome_widget, chat_area);
    }
}
