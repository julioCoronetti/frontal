use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::ModelEffortModal;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

impl ModelEffortModal {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width < 4 || area.height < 4 {
            frame.render_widget(Clear, area);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .style(Style::default().bg(Color::Reset));

        let mut lines = Vec::new();
        let title = format!(" Select Model: [{}]", self.backend_label());
        let esc_pad = (area.width as usize).saturating_sub(title.chars().count() + 8);
        let title_style = Style::default()
            .fg(TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled(title.as_str(), title_style),
            Span::styled(
                format!("{:>esc_pad$}", "F2 esc "),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        lines.push(Line::raw(""));

        let query_span = if self.query.is_empty() {
            Span::styled("   Search: ", Style::default().fg(TEXT_MUTED))
        } else {
            Span::styled(
                format!("   Search: {}", self.query),
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )
        };
        lines.push(Line::from(vec![
            query_span,
            Span::styled("█", Style::default().fg(ACCENT_HIGHLIGHT)),
        ]));
        let line_w = (area.width as usize).saturating_sub(8);
        let divider = "─".repeat(line_w);
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", divider),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        lines.push(Line::raw(""));

        let filtered = self.filtered_indices();
        let visible_count = area.height.saturating_sub(10) as usize;
        let visible_count = visible_count.max(1);
        let scroll_offset = if self.selected_model_idx >= visible_count {
            self.selected_model_idx + 1 - visible_count
        } else {
            0
        };

        if filtered.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "   No models matching query",
                Style::default().fg(TEXT_MUTED),
            )]));
        } else {
            for (display_idx, &real_idx) in filtered
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible_count)
            {
                let is_sel = display_idx == self.selected_model_idx;
                let m = &self.models[real_idx];
                let is_current = self
                    .current_model
                    .as_ref()
                    .map(|cur| cur == &m.id || cur.starts_with(&format!("{}-", m.id)))
                    .unwrap_or(false);

                let bullet = if is_sel { " ● " } else { "   " };
                let current_label = if is_current { "  (current)" } else { "" };

                let style = if is_sel {
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_PRIMARY)
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        bullet,
                        Style::default().fg(if is_sel { ACCENT_PRIMARY } else { TEXT_MUTED }),
                    ),
                    Span::styled(&m.name, style),
                    Span::styled(current_label, Style::default().fg(TEXT_MUTED)),
                ]));
            }
        }

        lines.push(Line::raw(""));

        let efforts = self.current_supported_efforts();
        if !efforts.is_empty() {
            let seg_len = if efforts.len() <= 3 { 7 } else { 5 };
            let seg_str = "─".repeat(seg_len);
            let track_nodes_len = efforts.len().saturating_sub(1) * (seg_len + 1) + 1;
            let total_track_len = 1 + 2 + track_nodes_len + 2 + 1;
            let pad_left = (area.width as usize)
                .saturating_sub(2)
                .saturating_sub(total_track_len)
                / 2;

            let mut track_spans = Vec::new();
            if pad_left > 0 {
                track_spans.push(Span::raw(" ".repeat(pad_left)));
            }
            track_spans.push(Span::styled("◀  ", Style::default().fg(ACCENT_PRIMARY)));

            for (i, _) in efforts.iter().enumerate() {
                if i > 0 {
                    let seg_color = if i <= self.selected_effort_idx {
                        ACCENT_PRIMARY
                    } else {
                        TEXT_MUTED
                    };
                    track_spans.push(Span::styled(
                        seg_str.clone(),
                        Style::default().fg(seg_color),
                    ));
                }

                let dot_span = if i == self.selected_effort_idx {
                    Span::styled(
                        "◎",
                        Style::default()
                            .fg(ACCENT_HIGHLIGHT)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if i < self.selected_effort_idx {
                    Span::styled("●", Style::default().fg(ACCENT_PRIMARY))
                } else {
                    Span::styled("●", Style::default().fg(TEXT_MUTED))
                };
                track_spans.push(dot_span);
            }
            track_spans.push(Span::styled("  ▶", Style::default().fg(ACCENT_PRIMARY)));
            lines.push(Line::from(track_spans));

            let mut label_spans = Vec::new();
            let mut curr_col = 0;
            for (i, eff) in efforts.iter().enumerate() {
                let node_col = pad_left + 3 + i * (seg_len + 1);
                let target_col = node_col.saturating_sub(eff.len() / 2);
                let spaces = target_col.saturating_sub(curr_col);
                if spaces > 0 {
                    label_spans.push(Span::raw(" ".repeat(spaces)));
                }
                let label_style = if i == self.selected_effort_idx {
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_MUTED)
                };
                label_spans.push(Span::styled(eff.clone(), label_style));
                curr_col = target_col + eff.len();
            }
            lines.push(Line::from(label_spans));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
