use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::cache::format::sanitize_terminal_line;
use crate::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BG_USER_CONTAINER, TEXT_MUTED, TEXT_PRIMARY,
};

fn parse_image_marker_len(slice: &str) -> Option<usize> {
    let rest = slice.strip_prefix("[Image ")?;
    let rest = rest.strip_prefix('#').unwrap_or(rest);
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || rest.as_bytes().get(digits) != Some(&b']') {
        return None;
    }
    Some(slice.len() - rest.len() + digits + 1)
}

fn tokenize_user_line(line: &str) -> Vec<(bool, String)> {
    let mut tokens = Vec::new();
    let mut text_start = 0;
    let mut idx = 0;
    while idx < line.len() {
        if line.as_bytes()[idx] == b'[' {
            if let Some(marker_len) = parse_image_marker_len(&line[idx..]) {
                if idx > text_start {
                    tokens.push((false, line[text_start..idx].to_string()));
                }
                tokens.push((true, line[idx..idx + marker_len].to_string()));
                idx += marker_len;
                text_start = idx;
                continue;
            }
        }
        let ch_len = line[idx..].chars().next().map_or(1, char::len_utf8);
        idx += ch_len;
    }
    if text_start < line.len() {
        tokens.push((false, line[text_start..].to_string()));
    }
    tokens
}

fn push_wrapped_segment(
    rows: &mut Vec<Vec<(bool, String)>>,
    current_row: &mut Vec<(bool, String)>,
    current_w: &mut usize,
    is_marker: bool,
    seg: &str,
    max_w: usize,
) {
    let seg_w = seg.chars().count();
    if is_marker && seg_w <= max_w {
        if *current_w > 0 && *current_w + seg_w > max_w {
            rows.push(std::mem::take(current_row));
            *current_w = 0;
        }
        current_row.push((true, seg.to_string()));
        *current_w += seg_w;
        return;
    }
    for ch in seg.chars() {
        if *current_w + 1 > max_w && *current_w > 0 {
            rows.push(std::mem::take(current_row));
            *current_w = 0;
        }
        match current_row.last_mut() {
            Some((last_marker, buf)) if *last_marker == is_marker => buf.push(ch),
            _ => current_row.push((is_marker, ch.to_string())),
        }
        *current_w += 1;
    }
}

fn wrap_user_body(body: &str, max_width: usize) -> Vec<Vec<(bool, String)>> {
    let mut rows = Vec::new();
    let max_w = max_width.max(1);
    for raw_line in body.split('\n') {
        let clean = sanitize_terminal_line(raw_line);
        if clean.is_empty() {
            rows.push(Vec::new());
            continue;
        }
        let mut current_row = Vec::new();
        let mut current_w = 0;
        for (is_marker, seg) in tokenize_user_line(&clean) {
            push_wrapped_segment(
                &mut rows,
                &mut current_row,
                &mut current_w,
                is_marker,
                &seg,
                max_w,
            );
        }
        rows.push(current_row);
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

fn split_user_attachment_footer(content: &str) -> (&str, Vec<&str>) {
    if let Some((body, tail)) = content.rsplit_once("\n\n") {
        if !tail.is_empty()
            && body.contains("[Image ")
            && tail.lines().all(|line| line.starts_with("[file] "))
        {
            let labels = tail
                .lines()
                .filter_map(|line| line.strip_prefix("[file] "))
                .collect();
            return (body, labels);
        }
    }
    (content, Vec::new())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    if max_chars > 3 {
        let mut out: String = text.chars().take(max_chars - 3).collect();
        out.push_str("...");
        out
    } else {
        text.chars().take(max_chars).collect()
    }
}

pub(crate) fn format_user_block_lines(content: &str, width: u16) -> Vec<Line<'static>> {
    let mut result = Vec::new();
    let (body, footer_labels) = split_user_attachment_footer(content);
    let wrapped = wrap_user_body(body, width.saturating_sub(4) as usize);
    let pad_empty = (width as usize).saturating_sub(2);
    let bg_style = Style::default().bg(BG_USER_CONTAINER);
    let bar_style = Style::default()
        .fg(ACCENT_PRIMARY)
        .bg(BG_USER_CONTAINER)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(TEXT_PRIMARY).bg(BG_USER_CONTAINER);
    let marker_style = Style::default()
        .fg(ACCENT_HIGHLIGHT)
        .bg(BG_USER_CONTAINER)
        .add_modifier(Modifier::BOLD);
    let badge_style = Style::default()
        .fg(TEXT_PRIMARY)
        .bg(ACCENT_RUBY)
        .add_modifier(Modifier::BOLD);
    let muted_style = Style::default().fg(TEXT_MUTED).bg(BG_USER_CONTAINER);

    let blank_user_line = || {
        Line::from(vec![
            Span::styled("▌ ", bar_style),
            Span::styled(" ".repeat(pad_empty), bg_style),
        ])
    };

    result.push(blank_user_line());

    for row in wrapped {
        let mut spans = Vec::with_capacity(row.len() + 2);
        spans.push(Span::styled("▌ ", bar_style));
        let mut row_len = 0;
        for (is_marker, seg) in row {
            row_len += seg.chars().count();
            let style = if is_marker { marker_style } else { text_style };
            spans.push(Span::styled(seg, style));
        }
        let pad_len = (width as usize).saturating_sub(2 + row_len);
        spans.push(Span::styled(" ".repeat(pad_len), bg_style));
        result.push(Line::from(spans));
    }

    if !footer_labels.is_empty() {
        result.push(blank_user_line());
        let max_content_w = (width as usize).saturating_sub(2);
        for label in footer_labels {
            let clean_label = sanitize_terminal_line(label);
            if max_content_w >= 8 {
                let badge = " file ";
                let avail = max_content_w.saturating_sub(7);
                let label_text = format!(" {}", truncate_chars(&clean_label, avail));
                let used = 6 + label_text.chars().count();
                let pad_len = max_content_w.saturating_sub(used);
                result.push(Line::from(vec![
                    Span::styled("▌ ", bar_style),
                    Span::styled(badge, badge_style),
                    Span::styled(label_text, muted_style),
                    Span::styled(" ".repeat(pad_len), bg_style),
                ]));
            } else {
                let fallback = truncate_chars(&format!("file {}", clean_label), max_content_w);
                let used = fallback.chars().count();
                let pad_len = max_content_w.saturating_sub(used);
                result.push(Line::from(vec![
                    Span::styled("▌ ", bar_style),
                    Span::styled(fallback, muted_style),
                    Span::styled(" ".repeat(pad_len), bg_style),
                ]));
            }
        }
    }

    result.push(blank_user_line());
    result.push(Line::from(""));
    result
}
