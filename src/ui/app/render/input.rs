use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::editor::{Extmark, ExtmarkKind};
use crate::ui::theme::{ACCENT_PRIMARY, ACCENT_RUBY};

fn char_input_style(
    char_idx: usize,
    extmarks: &[Extmark],
    selection: Option<(usize, usize)>,
) -> Style {
    let is_media = extmarks.iter().any(|ext| {
        matches!(ext.kind, ExtmarkKind::Media { .. })
            && char_idx >= ext.start_char
            && char_idx < ext.end_char
    });
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

pub(crate) fn build_input_text(
    full_text: &str,
    extmarks: &[Extmark],
    selection: Option<(usize, usize)>,
) -> Text<'static> {
    let mut lines = Vec::new();
    let mut global_char_idx = 0;

    for line_str in full_text.split('\n') {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut current_style: Option<Style> = None;
        let mut current_buf = String::new();
        let mut line_len = 0;

        for ch in line_str.chars() {
            let style = char_input_style(global_char_idx + line_len, extmarks, selection);
            match current_style {
                Some(active) if active == style => current_buf.push(ch),
                Some(active) => {
                    spans.push(Span::styled(std::mem::take(&mut current_buf), active));
                    current_buf.push(ch);
                    current_style = Some(style);
                }
                None => {
                    current_buf.push(ch);
                    current_style = Some(style);
                }
            }
            line_len += 1;
        }

        if let Some(active) = current_style {
            if !current_buf.is_empty() {
                spans.push(Span::styled(current_buf, active));
            }
        }
        lines.push(Line::from(spans));
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
        let display_content = self.editor.text();
        let (cur_line, cur_col) = self.editor.cursor_coordinates();

        let max_visible = input_area.height.saturating_sub(2) as usize;
        let scroll_y = if max_visible > 0 && cur_line >= max_visible {
            (cur_line + 1 - max_visible) as u16
        } else {
            0
        };

        let selection = self.editor.selection_range();
        let has_media_extmarks = self
            .editor
            .extmarks()
            .iter()
            .any(|ext| matches!(ext.kind, ExtmarkKind::Media { .. }));
        let input_text = if selection.is_some() || has_media_extmarks {
            build_input_text(&display_content, self.editor.extmarks(), selection)
        } else {
            Text::from(display_content)
        };

        let mut input_widget = Paragraph::new(input_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(Span::styled(
                    input_title,
                    Style::default().fg(ACCENT_PRIMARY),
                )))
                .border_style(Style::default().fg(ACCENT_PRIMARY))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        if scroll_y > 0 {
            input_widget = input_widget.scroll((scroll_y, 0));
        }
        frame.render_widget(Clear, input_area);
        frame.render_widget(input_widget, input_area);

        let cursor_x = input_area
            .x
            .saturating_add(2)
            .saturating_add(cur_col as u16);
        let cursor_y = input_area
            .y
            .saturating_add(1)
            .saturating_add(cur_line.saturating_sub(scroll_y as usize) as u16);
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
