use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::DialogSelect;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

pub fn render_select<T: Clone>(dialog: &DialogSelect<T>, frame: &mut Frame, area: Rect) {
    dialog.last_area.set(area);
    if area.width < 4 || area.height < 4 {
        frame.render_widget(Clear, area);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(Color::Reset));

    let filtered = dialog.filtered_indices();
    let mut lines = Vec::new();

    let clean_title = truncate_chars(
        dialog.title.trim_end_matches(':'),
        (area.width as usize).saturating_sub(8),
    );
    let title = format!(" {}:", clean_title);
    let esc_pad = (area.width as usize).saturating_sub(title.chars().count() + 5);
    lines.push(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:>esc_pad$}", "esc "),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::raw(""));

    let query_budget = (area.width as usize).saturating_sub(16);
    let display_query = if dialog.query.is_empty() && !dialog.placeholder.is_empty() {
        Span::styled(
            format!(
                "Search: {}",
                truncate_chars(&dialog.placeholder, query_budget)
            ),
            Style::default().fg(TEXT_MUTED),
        )
    } else if dialog.query.is_empty() {
        Span::styled("Search: ", Style::default().fg(TEXT_MUTED))
    } else {
        Span::styled(
            format!("Search: {}", truncate_chars(&dialog.query, query_budget)),
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
    };

    lines.push(Line::from(vec![
        Span::styled(
            "   > ",
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        display_query,
        Span::styled("█", Style::default().fg(ACCENT_HIGHLIGHT)),
    ]));
    let line_w = (area.width as usize).saturating_sub(8);
    let divider = "─".repeat(line_w);
    lines.push(Line::from(vec![Span::styled(
        format!("   {}", divider),
        Style::default().fg(BORDER_SUBTLE),
    )]));
    lines.push(Line::raw(""));

    let rows = dialog.build_visual_rows(&filtered);

    let selected_row_idx = rows
        .iter()
        .position(|(is_cat, _, pos)| !*is_cat && *pos == dialog.selected_idx)
        .unwrap_or(0);

    let inner_height = area.height.saturating_sub(7) as usize;
    let mut offset = dialog.scroll_offset.get();
    if selected_row_idx < offset {
        offset = if selected_row_idx > 0 && rows[selected_row_idx - 1].0 {
            selected_row_idx - 1
        } else {
            selected_row_idx
        };
    } else if inner_height > 0 && selected_row_idx >= offset + inner_height {
        offset = selected_row_idx + 1 - inner_height;
    }
    offset = offset.min(rows.len().saturating_sub(inner_height));
    dialog.scroll_offset.set(offset);

    let visible_slice = if inner_height > 0 && !rows.is_empty() {
        let end = (offset + inner_height).min(rows.len());
        &rows[offset..end]
    } else {
        &[]
    };

    for (is_cat, cat_name, pos) in visible_slice {
        if *is_cat {
            let cat_budget = (area.width as usize).saturating_sub(6);
            let clean_cat = truncate_chars(cat_name, cat_budget);
            lines.push(Line::from(vec![Span::styled(
                format!("  [{}]", clean_cat),
                Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
            )]));
        } else {
            let item = &dialog.items[filtered[*pos]];
            let is_sel = *pos == dialog.selected_idx;
            let is_pending_delete = dialog.pending_delete.as_deref() == Some(item.id.as_str());
            let (style, badge_style, shortcut_style, desc_style) = if is_sel {
                (
                    Style::default()
                        .bg(ACCENT_RUBY)
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .bg(ACCENT_RUBY)
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                    Style::default().bg(ACCENT_RUBY).fg(TEXT_PRIMARY),
                    Style::default().bg(ACCENT_RUBY).fg(TEXT_MUTED),
                )
            } else {
                (
                    Style::default().fg(TEXT_PRIMARY),
                    Style::default().fg(ACCENT_HIGHLIGHT),
                    Style::default().fg(TEXT_MUTED),
                    Style::default().fg(TEXT_MUTED),
                )
            };
            let inner_w = (area.width as usize).saturating_sub(2);
            let prefix_raw = if is_sel { " ● " } else { "   " };
            let prefix_len = prefix_raw.chars().count().min(inner_w);
            let prefix: String = prefix_raw.chars().take(prefix_len).collect();
            let remaining = inner_w.saturating_sub(prefix_len);

            let raw_right = if is_pending_delete {
                Some("Confirm delete?".to_string())
            } else {
                item.shortcut.clone()
            };
            let right_budget = raw_right.as_ref().and_then(|label| {
                let desired = label.chars().count() + 2;
                let max_allowed = if is_pending_delete {
                    remaining
                } else if remaining >= desired + 6 {
                    desired
                } else if remaining >= 8 {
                    remaining / 2
                } else {
                    0
                };
                if max_allowed <= 2 {
                    None
                } else {
                    let fitted = truncate_chars(label, max_allowed - 2);
                    if fitted.is_empty() {
                        None
                    } else {
                        let cols = fitted.chars().count() + 2;
                        Some((fitted, cols))
                    }
                }
            });
            let right_cols = right_budget.as_ref().map(|(_, c)| *c).unwrap_or(0);
            let left_budget = remaining.saturating_sub(right_cols);

            let badge_fitted = item.badge.as_ref().and_then(|badge| {
                let clean_badge: String = badge.chars().filter(|c| !c.is_control()).collect();
                if clean_badge.is_empty() || left_budget < 5 {
                    return None;
                }
                let desired = clean_badge.chars().count() + 3;
                if desired <= left_budget.saturating_sub(3) {
                    Some((clean_badge, desired))
                } else {
                    let max_b = left_budget.saturating_sub(6);
                    if max_b == 0 {
                        None
                    } else {
                        let b = truncate_chars(&clean_badge, max_b);
                        let c = b.chars().count() + 3;
                        Some((b, c))
                    }
                }
            });
            let badge_cols = badge_fitted.as_ref().map(|(_, c)| *c).unwrap_or(0);

            let title_budget = left_budget.saturating_sub(badge_cols);
            let display_title = truncate_chars(&item.title, title_budget);
            let title_cols = display_title.chars().count();

            let mut spans = vec![
                Span::styled(prefix, style),
                Span::styled(display_title, style),
            ];
            let mut left_len = prefix_len + title_cols;
            if let Some((badge_text, b_cols)) = badge_fitted {
                spans.push(Span::styled(format!(" [{}]", badge_text), badge_style));
                left_len += b_cols;
            }
            let show_desc = item.description.is_some() && !is_pending_delete;
            if show_desc {
                if let Some(ref desc) = item.description {
                    let avail = remaining.saturating_sub((left_len - prefix_len) + right_cols);
                    if avail > 3 {
                        let desc_text = truncate_chars(desc, avail - 3);
                        if !desc_text.is_empty() {
                            let d_len = desc_text.chars().count();
                            spans.push(Span::styled(format!(" - {}", desc_text), desc_style));
                            left_len += d_len + 3;
                        }
                    }
                }
            }
            if let Some((label, label_len)) = right_budget {
                let spaces = inner_w.saturating_sub(left_len + label_len);
                if spaces > 0 {
                    spans.push(Span::styled(" ".repeat(spaces), style));
                }
                let label_style = if is_pending_delete {
                    badge_style
                } else {
                    shortcut_style
                };
                spans.push(Span::styled(format!(" {} ", label), label_style));
            } else if is_sel && inner_w > left_len {
                spans.push(Span::styled(" ".repeat(inner_w - left_len), style));
            }
            lines.push(Line::from(spans));
        }
    }

    if filtered.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "   (No matching results)",
            Style::default().fg(TEXT_MUTED),
        )]));
    }

    let widget = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

fn truncate_chars(text: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= max_cols {
        return clean;
    }
    if max_cols > 3 {
        let mut out: String = clean.chars().take(max_cols - 3).collect();
        out.push_str("...");
        out
    } else {
        clean.chars().take(max_cols).collect()
    }
}
