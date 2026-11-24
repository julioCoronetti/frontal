use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::editor::{Extmark, ExtmarkKind};
use crate::ui::theme::{ACCENT_PRIMARY, ACCENT_RUBY};

fn build_media_ranges(extmarks: &[Extmark]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = extmarks
        .iter()
        .filter_map(|ext| match ext.kind {
            ExtmarkKind::Media { .. } if ext.start_char < ext.end_char => {
                Some((ext.start_char, ext.end_char))
            }
            _ => None,
        })
        .collect();
    ranges.sort_unstable_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn style_at_char(
    char_idx: usize,
    media_ranges: &[(usize, usize)],
    media_cursor: &mut usize,
    selection: Option<(usize, usize)>,
) -> Style {
    while *media_cursor < media_ranges.len() && media_ranges[*media_cursor].1 <= char_idx {
        *media_cursor += 1;
    }
    let is_media = *media_cursor < media_ranges.len()
        && char_idx >= media_ranges[*media_cursor].0
        && char_idx < media_ranges[*media_cursor].1;
    let mut style = if is_media {
        Style::default()
            .fg(ACCENT_RUBY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    if let Some((sel_start, sel_end)) = selection {
        if char_idx >= sel_start && char_idx < sel_end {
            style = style.add_modifier(Modifier::REVERSED);
        }
    }
    style
}

pub(crate) fn visual_line_count(text: &str, inner_width: usize) -> usize {
    if inner_width == 0 {
        return text.split('\n').count().max(1);
    }
    text.split('\n')
        .map(|line| (line.chars().count() / inner_width) + 1)
        .sum()
}

pub(crate) fn visual_cursor_position(
    text: &str,
    cur_line: usize,
    cur_col: usize,
    inner_width: usize,
) -> (usize, usize) {
    if inner_width == 0 {
        return (cur_line, cur_col);
    }
    let mut vis_row = 0usize;
    for (idx, line) in text.split('\n').enumerate() {
        if idx < cur_line {
            vis_row += (line.chars().count() / inner_width) + 1;
        } else {
            return (vis_row + (cur_col / inner_width), cur_col % inner_width);
        }
    }
    (vis_row, 0)
}

pub(crate) fn build_input_text(
    full_text: &str,
    extmarks: &[Extmark],
    selection: Option<(usize, usize)>,
    inner_width: usize,
) -> Text<'static> {
    let media_ranges = build_media_ranges(extmarks);
    let mut media_cursor = 0usize;
    let mut lines = Vec::new();
    let mut global_char_idx = 0usize;
    let wrap_w = inner_width.max(1);

    for line_str in full_text.split('\n') {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut current_style: Option<Style> = None;
        let mut run_start_byte = 0usize;
        let mut line_len = 0usize;
        let mut col_in_row = 0usize;

        for (byte_idx, _) in line_str.char_indices() {
            if col_in_row == wrap_w {
                if let Some(active) = current_style.take() {
                    if byte_idx > run_start_byte {
                        spans.push(Span::styled(
                            line_str[run_start_byte..byte_idx].to_string(),
                            active,
                        ));
                    }
                }
                lines.push(Line::from(std::mem::take(&mut spans)));
                run_start_byte = byte_idx;
                col_in_row = 0;
            }
            let style = style_at_char(
                global_char_idx + line_len,
                &media_ranges,
                &mut media_cursor,
                selection,
            );
            match current_style {
                Some(active) if active == style => {}
                Some(active) => {
                    if byte_idx > run_start_byte {
                        spans.push(Span::styled(
                            line_str[run_start_byte..byte_idx].to_string(),
                            active,
                        ));
                    }
                    run_start_byte = byte_idx;
                    current_style = Some(style);
                }
                None => {
                    run_start_byte = byte_idx;
                    current_style = Some(style);
                }
            }
            line_len += 1;
            col_in_row += 1;
        }

        if let Some(active) = current_style {
            if line_str.len() > run_start_byte {
                spans.push(Span::styled(line_str[run_start_byte..].to_string(), active));
            }
        }
        lines.push(Line::from(spans));
        if line_len > 0 && col_in_row == wrap_w {
            lines.push(Line::default());
        }
        global_char_idx += line_len + 1;
    }

    Text::from(lines)
}

impl App {
    pub(crate) fn render_input(&self, frame: &mut Frame, input_area: Rect) -> Option<(u16, u16)> {
        let model_name = crate::ui::labels::model_display_name(&self.active_model_name());
        let effort_name = self.active_effort_name();
        let input_title = if model_name.is_empty() {
            format!(" {} ", effort_name)
        } else {
            format!(" {} ({}) ", model_name, effort_name)
        };
        let tab = self.active_tab();
        let display_content = tab.editor.text();
        let (cur_line, cur_col) = tab.editor.cursor_coordinates();
        let inner_width = input_area.width.saturating_sub(4) as usize;
        let (vis_row, vis_col) =
            visual_cursor_position(&display_content, cur_line, cur_col, inner_width);

        let max_visible = input_area.height.saturating_sub(2) as usize;
        let scroll_y = if max_visible > 0 && vis_row >= max_visible {
            (vis_row + 1 - max_visible) as u16
        } else {
            0
        };

        let selection = tab.editor.selection_range();
        let input_text = build_input_text(
            &display_content,
            tab.editor.extmarks(),
            selection,
            inner_width,
        );

        let mut title_spans = vec![Span::styled(
            input_title,
            Style::default().fg(ACCENT_PRIMARY),
        )];
        let mode_name = self.active_mode_name();
        if let Some(canonical_mode) = crate::config::BackendKind::normalize_mode(&mode_name) {
            if canonical_mode != crate::config::DEFAULT_MODE
                && tab.backend.supports_mode(canonical_mode)
            {
                title_spans.push(Span::styled(
                    format!("[{}] ", canonical_mode),
                    Style::default()
                        .fg(ACCENT_RUBY)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        let mut input_widget = Paragraph::new(input_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(title_spans))
                .border_style(Style::default().fg(ACCENT_PRIMARY))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        if scroll_y > 0 {
            input_widget = input_widget.scroll((scroll_y, 0));
        }
        frame.render_widget(Clear, input_area);
        frame.render_widget(input_widget, input_area);

        if inner_width == 0 || input_area.height < 3 {
            return None;
        }
        let cursor_x = input_area
            .x
            .saturating_add(2)
            .saturating_add(vis_col as u16);
        let cursor_y = input_area
            .y
            .saturating_add(1)
            .saturating_add(vis_row.saturating_sub(scroll_y as usize) as u16);
        if cursor_x > input_area.x
            && cursor_x
                < input_area
                    .x
                    .saturating_add(input_area.width.saturating_sub(2))
            && cursor_y > input_area.y
            && cursor_y
                < input_area
                    .y
                    .saturating_add(input_area.height.saturating_sub(1))
        {
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }
}
