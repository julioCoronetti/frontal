use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::{App, HitAction, HitRegion};
use crate::ui::editor::TriggerKind;
use crate::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BORDER_SUBTLE, TAB_ACTIVE_BG, TEXT_MUTED,
    TEXT_PRIMARY,
};

const TARGET_TAB_WIDTH: usize = 28;
const MIN_TAB_WIDTH: usize = 16;
const CLOSE_SLOT_WIDTH: usize = 3;
const NEW_TAB_LABEL: &str = " + ";

fn truncate_to_display_width(text: &str, max_width: usize) -> String {
    if Span::raw(text).width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return String::from("…");
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut candidate = out.clone();
        candidate.push(ch);
        if Span::raw(&candidate).width() + 1 > max_width {
            break;
        }
        out = candidate;
    }
    out.push('…');
    out
}

impl App {
    pub(crate) fn render_tab_bar(
        &mut self,
        frame: &mut Frame,
        tab_bar_area: Rect,
        spinner_char: &str,
    ) {
        self.hit_regions.clear();
        if tab_bar_area.width < 3 || tab_bar_area.height == 0 || self.tabs.is_empty() {
            return;
        }

        let total_w = tab_bar_area.width as usize;
        let plus_w = Span::raw(NEW_TAB_LABEL).width();
        let plus_reserve = plus_w + 1;
        let tabs_budget = total_w.saturating_sub(plus_reserve);

        let count = self.tabs.len();
        let sep_count = count.saturating_sub(1);
        let tab_width = if count * TARGET_TAB_WIDTH + sep_count <= tabs_budget {
            TARGET_TAB_WIDTH
        } else if count * MIN_TAB_WIDTH + sep_count <= tabs_budget {
            tabs_budget.saturating_sub(sep_count) / count
        } else {
            MIN_TAB_WIDTH.min(tabs_budget.max(CLOSE_SLOT_WIDTH + 4))
        };

        let active_idx = self.active_tab_index.min(count - 1);
        let mut start_idx = active_idx;
        let mut end_idx = active_idx;
        let mut used_w = tab_width;

        loop {
            let mut expanded = false;
            if end_idx + 1 < count && used_w + 1 + tab_width <= tabs_budget {
                end_idx += 1;
                used_w += 1 + tab_width;
                expanded = true;
            }
            if start_idx > 0 && used_w + 1 + tab_width <= tabs_budget {
                start_idx -= 1;
                used_w += 1 + tab_width;
                expanded = true;
            }
            if !expanded {
                break;
            }
        }

        let mut tab_spans = Vec::new();
        let mut cursor_x = tab_bar_area.x;
        let max_x = tab_bar_area.right();
        let y = tab_bar_area.y;

        for i in start_idx..=end_idx {
            let tab = &self.tabs[i];
            let is_active = i == active_idx;
            let is_hovered = self.hovered_tab_index == Some(i);

            let body_target_w = tab_width.saturating_sub(CLOSE_SLOT_WIDTH);
            let spinner = if tab.waiting_for_response {
                format!("{} ", spinner_char)
            } else {
                String::new()
            };
            let unread = if tab.has_unread { "*" } else { "" };
            let prefix_w = 2 + Span::raw(&spinner).width() + Span::raw(unread).width();
            let title_cap = body_target_w.saturating_sub(prefix_w + 1);
            let title = truncate_to_display_width(&tab.title, title_cap);
            let mut body_text = format!("  {}{}{}", spinner, title, unread);
            let current_w = Span::raw(&body_text).width();
            if current_w < body_target_w {
                body_text.push_str(&" ".repeat(body_target_w - current_w));
            }

            let mut body_style = Style::default()
                .fg(if is_active {
                    TEXT_PRIMARY
                } else if tab.has_unread {
                    ACCENT_HIGHLIGHT
                } else {
                    TEXT_MUTED
                })
                .bg(if is_active {
                    TAB_ACTIVE_BG
                } else {
                    Color::Reset
                });
            if is_active {
                body_style = body_style.add_modifier(Modifier::BOLD);
            }

            let close_text = if is_hovered { " × " } else { "   " };
            let mut close_style = Style::default()
                .fg(if is_hovered {
                    ACCENT_HIGHLIGHT
                } else if is_active {
                    TEXT_PRIMARY
                } else {
                    TEXT_MUTED
                })
                .bg(if is_active {
                    TAB_ACTIVE_BG
                } else {
                    Color::Reset
                });
            if is_active {
                close_style = close_style.add_modifier(Modifier::BOLD);
            }

            let body_span = Span::styled(body_text, body_style);
            let close_span = Span::styled(close_text, close_style);
            let body_w = body_span.width() as u16;
            let close_w = close_span.width() as u16;
            let total_tab_w = body_w.saturating_add(close_w);

            if cursor_x.saturating_add(total_tab_w) > max_x.saturating_sub(plus_w as u16) {
                break;
            }

            if is_hovered {
                if body_w > 0 {
                    self.hit_regions.push(HitRegion {
                        rect: Rect {
                            x: cursor_x,
                            y,
                            width: body_w,
                            height: 1,
                        },
                        action: HitAction::SwitchTab(i),
                    });
                }
                if close_w > 0 {
                    self.hit_regions.push(HitRegion {
                        rect: Rect {
                            x: cursor_x.saturating_add(body_w),
                            y,
                            width: close_w,
                            height: 1,
                        },
                        action: HitAction::CloseOrResetTab(i),
                    });
                }
            } else if total_tab_w > 0 {
                self.hit_regions.push(HitRegion {
                    rect: Rect {
                        x: cursor_x,
                        y,
                        width: total_tab_w,
                        height: 1,
                    },
                    action: HitAction::SwitchTab(i),
                });
            }

            tab_spans.push(body_span);
            tab_spans.push(close_span);
            cursor_x = cursor_x.saturating_add(total_tab_w);

            if i < end_idx && cursor_x < max_x.saturating_sub(plus_w as u16) {
                tab_spans.push(Span::raw(" "));
                cursor_x = cursor_x.saturating_add(1);
            }
        }

        if cursor_x.saturating_add(1 + plus_w as u16) <= max_x {
            tab_spans.push(Span::raw(" "));
            cursor_x = cursor_x.saturating_add(1);
        }

        if cursor_x.saturating_add(plus_w as u16) <= max_x {
            let mut plus_style = Style::default()
                .fg(if self.hovered_new_tab {
                    TEXT_PRIMARY
                } else {
                    TEXT_MUTED
                })
                .bg(if self.hovered_new_tab {
                    TAB_ACTIVE_BG
                } else {
                    Color::Reset
                });
            if self.hovered_new_tab {
                plus_style = plus_style.add_modifier(Modifier::BOLD);
            }
            let plus_span = Span::styled(NEW_TAB_LABEL, plus_style);
            self.hit_regions.push(HitRegion {
                rect: Rect {
                    x: cursor_x,
                    y,
                    width: plus_w as u16,
                    height: 1,
                },
                action: HitAction::NewTab,
            });
            tab_spans.push(plus_span);
        }

        let tab_bar_widget = Paragraph::new(Line::from(tab_spans));
        frame.render_widget(tab_bar_widget, tab_bar_area);
    }

    pub(crate) fn render_divider(&self, frame: &mut Frame, divider_area: Rect, width: u16) {
        let divider_widget = Paragraph::new(Line::from(vec![Span::styled(
            "─".repeat(width as usize),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        frame.render_widget(divider_widget, divider_area);
    }

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

        let input_text = if let Some((sel_start, sel_end)) = self.editor.selection_range() {
            let full_text = self.editor.text();
            let mut lines = Vec::new();
            let mut global_char_idx = 0;
            for line_str in full_text.split('\n') {
                let line_chars: Vec<char> = line_str.chars().collect();
                let line_len = line_chars.len();
                let line_start = global_char_idx;
                let line_end = line_start + line_len;

                if sel_end <= line_start || sel_start >= line_end {
                    lines.push(Line::from(line_str.to_string()));
                } else {
                    let mut spans = Vec::new();
                    let overlap_start = sel_start.max(line_start).saturating_sub(line_start);
                    let overlap_end = sel_end.min(line_end).saturating_sub(line_start);

                    if overlap_start > 0 {
                        let before: String = line_chars[..overlap_start].iter().collect();
                        spans.push(Span::raw(before));
                    }
                    if overlap_end > overlap_start {
                        let selected: String =
                            line_chars[overlap_start..overlap_end].iter().collect();
                        spans.push(Span::styled(
                            selected,
                            Style::default().add_modifier(Modifier::REVERSED),
                        ));
                    }
                    if overlap_end < line_len {
                        let after: String = line_chars[overlap_end..].iter().collect();
                        spans.push(Span::raw(after));
                    }
                    lines.push(Line::from(spans));
                }
                global_char_idx = line_end + 1;
            }
            Text::from(lines)
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

    pub(crate) fn render_footer(&self, frame: &mut Frame, footer_area: Rect) {
        let footer_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(18)])
            .split(footer_area);

        let cwd_widget = Paragraph::new(Line::from(vec![Span::styled(
            &self.cwd_display,
            Style::default().fg(TEXT_MUTED),
        )]));
        frame.render_widget(cwd_widget, footer_chunks[0]);

        let shortcuts_widget = Paragraph::new(Line::from(vec![Span::styled(
            "Ctrl+P: Palette",
            Style::default().fg(TEXT_MUTED),
        )]))
        .alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(shortcuts_widget, footer_chunks[1]);
    }

    pub(crate) fn render_popover(&self, frame: &mut Frame, input_area: Rect, screen_area: Rect) {
        if !self.popover.is_visible() {
            return;
        }

        let item_count = self.popover.items.len().min(8) as u16;
        let popover_height = item_count + 2;
        let popover_width = (input_area.width.saturating_sub(4)).min(60);
        let popover_y = input_area.y.saturating_sub(popover_height);
        let popover_rect = Rect {
            x: input_area.x.saturating_add(2),
            y: popover_y,
            width: popover_width,
            height: popover_height,
        }
        .intersection(screen_area);

        frame.render_widget(Clear, popover_rect);

        let max_visible = 8usize;
        let total = self.popover.items.len();
        let sel = self.popover.selected_idx;
        let start_idx = if total <= max_visible || sel < max_visible {
            0
        } else {
            (sel + 1).saturating_sub(max_visible)
        };

        let mut popover_lines = Vec::new();
        for (idx, item) in self
            .popover
            .items
            .iter()
            .enumerate()
            .skip(start_idx)
            .take(max_visible)
        {
            let is_sel = idx == self.popover.selected_idx;
            let (prefix, cmd_style, desc_style) = if is_sel {
                (
                    " > ",
                    Style::default()
                        .bg(ACCENT_RUBY)
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                    Style::default().bg(ACCENT_RUBY).fg(TEXT_PRIMARY),
                )
            } else {
                (
                    "   ",
                    Style::default()
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(TEXT_MUTED),
                )
            };
            let mut spans = vec![Span::styled(prefix, desc_style)];
            let mut row_len = 3 + item.label.chars().count();
            if let Some(desc) = &item.description {
                let cmd_col_w: usize = 14;
                let pad_count = cmd_col_w.saturating_sub(item.label.chars().count()).max(1);
                spans.push(Span::styled(&item.label, cmd_style));
                spans.push(Span::styled(" ".repeat(pad_count), desc_style));
                spans.push(Span::styled(desc, desc_style));
                row_len += pad_count + desc.chars().count();
            } else {
                spans.push(Span::styled(&item.label, cmd_style));
            }
            let inner_w = (popover_rect.width as usize).saturating_sub(2);
            if is_sel && inner_w > row_len {
                spans.push(Span::styled(" ".repeat(inner_w - row_len), desc_style));
            }
            popover_lines.push(Line::from(spans));
        }

        let title = match self.popover.kind {
            TriggerKind::Command => " Commands (Tab/Enter: apply, Esc: close) ",
            TriggerKind::File => " Files (Tab/Enter: apply, Esc: close) ",
        };

        let popover_widget = Paragraph::new(popover_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(ACCENT_RUBY))
                .style(Style::default().bg(Color::Reset)),
        );
        frame.render_widget(popover_widget, popover_rect);
    }
}
