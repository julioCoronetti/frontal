use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::ui::markdown::normalize::normalize_streaming_markdown;

#[derive(Clone, Debug)]
pub enum MarkdownSegment {
    Heading {
        level: u8,
        spans: Vec<Span<'static>>,
    },
    Paragraph(Vec<Span<'static>>),
    ListItem {
        bullet: String,
        spans: Vec<Span<'static>>,
    },
    BlockQuote(Vec<Span<'static>>),
    CodeBlock {
        language: String,
        code: String,
        is_unclosed: bool,
    },
    Table {
        headers: Vec<Vec<Span<'static>>>,
        rows: Vec<Vec<Vec<Span<'static>>>>,
    },
    Rule,
}

pub fn parse_markdown_segments(markdown_text: &str, consolidated: bool) -> Vec<MarkdownSegment> {
    let normalized = normalize_streaming_markdown(markdown_text, consolidated);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&normalized.text, options);
    let mut segments: Vec<MarkdownSegment> = Vec::new();

    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut active_styles: Vec<Style> = vec![Style::default().fg(Color::White)];
    let mut current_heading_level: Option<u8> = None;
    let mut in_code_block: Option<(String, String)> = None;
    let mut list_depth: usize = 0;
    let mut is_ordered_list: Vec<u64> = Vec::new();
    let mut in_blockquote = false;

    let mut in_table = false;
    let mut in_table_head = false;
    let mut table_headers: Vec<Vec<Span<'static>>> = Vec::new();
    let mut table_rows: Vec<Vec<Vec<Span<'static>>>> = Vec::new();
    let mut current_table_row: Vec<Vec<Span<'static>>> = Vec::new();
    let mut pending_item_bullets: Vec<Option<String>> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    current_heading_level = Some(lvl);
                    current_spans.clear();
                    let h_style = match lvl {
                        1 => Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED),
                        2 => Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        _ => Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    };
                    active_styles.push(h_style);
                }
                Tag::Paragraph => {
                    if list_depth == 0 {
                        current_spans.clear();
                    }
                }
                Tag::BlockQuote(_) => {
                    in_blockquote = true;
                    current_spans.clear();
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    in_code_block = Some((lang, String::new()));
                }
                Tag::List(start_num) => {
                    if list_depth > 0 && !current_spans.is_empty() {
                        let bullet = pending_item_bullets
                            .last_mut()
                            .and_then(|b| b.take())
                            .unwrap_or_else(|| String::from("  "));
                        segments.push(MarkdownSegment::ListItem {
                            bullet,
                            spans: std::mem::take(&mut current_spans),
                        });
                    }
                    list_depth += 1;
                    if let Some(start) = start_num {
                        is_ordered_list.push(start);
                    } else {
                        is_ordered_list.push(0);
                    }
                }
                Tag::Item => {
                    if list_depth > 0 && !current_spans.is_empty() {
                        let bullet = pending_item_bullets
                            .last_mut()
                            .and_then(|b| b.take())
                            .unwrap_or_else(|| String::from("  "));
                        segments.push(MarkdownSegment::ListItem {
                            bullet,
                            spans: std::mem::take(&mut current_spans),
                        });
                    }
                    let bullet = if let Some(order) = is_ordered_list.last_mut() {
                        if *order > 0 {
                            let b = format!("{}. ", order);
                            *order += 1;
                            b
                        } else {
                            String::from("• ")
                        }
                    } else {
                        String::from("• ")
                    };
                    pending_item_bullets.push(Some(bullet));
                    current_spans.clear();
                }
                Tag::Emphasis => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::BOLD));
                }
                Tag::Strikethrough => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::CROSSED_OUT));
                }
                Tag::Table(_) => {
                    in_table = true;
                    table_headers.clear();
                    table_rows.clear();
                }
                Tag::TableHead => {
                    in_table_head = true;
                    table_headers.clear();
                }
                Tag::TableRow => {
                    current_table_row.clear();
                }
                Tag::TableCell => {
                    current_spans.clear();
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if let Some(lvl) = current_heading_level.take() {
                        segments.push(MarkdownSegment::Heading {
                            level: lvl,
                            spans: std::mem::take(&mut current_spans),
                        });
                    }
                    active_styles.pop();
                }
                TagEnd::Paragraph => {
                    if !current_spans.is_empty() {
                        if list_depth > 0 {
                            let bullet = pending_item_bullets
                                .last_mut()
                                .and_then(|b| b.take())
                                .unwrap_or_else(|| String::from("  "));
                            segments.push(MarkdownSegment::ListItem {
                                bullet,
                                spans: std::mem::take(&mut current_spans),
                            });
                        } else {
                            segments.push(MarkdownSegment::Paragraph(std::mem::take(
                                &mut current_spans,
                            )));
                        }
                    }
                }
                TagEnd::BlockQuote(_) => {
                    in_blockquote = false;
                    if !current_spans.is_empty() {
                        segments.push(MarkdownSegment::BlockQuote(std::mem::take(
                            &mut current_spans,
                        )));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, code)) = in_code_block.take() {
                        segments.push(MarkdownSegment::CodeBlock {
                            language: lang,
                            code,
                            is_unclosed: normalized.has_unclosed_fence,
                        });
                    }
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    is_ordered_list.pop();
                }
                TagEnd::Item => {
                    let rem_bullet = pending_item_bullets.pop().flatten();
                    if !current_spans.is_empty() {
                        let bullet = rem_bullet.unwrap_or_else(|| String::from("• "));
                        segments.push(MarkdownSegment::ListItem {
                            bullet,
                            spans: std::mem::take(&mut current_spans),
                        });
                    } else if let Some(bullet) = rem_bullet {
                        segments.push(MarkdownSegment::ListItem {
                            bullet,
                            spans: Vec::new(),
                        });
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    active_styles.pop();
                }
                TagEnd::TableCell => {
                    let cell_spans = std::mem::take(&mut current_spans);
                    if in_table_head {
                        table_headers.push(cell_spans);
                    } else {
                        current_table_row.push(cell_spans);
                    }
                }
                TagEnd::TableRow => {
                    if !in_table_head && !current_table_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_table_row));
                    }
                }
                TagEnd::TableHead => {
                    in_table_head = false;
                }
                TagEnd::Table => {
                    in_table = false;
                    segments.push(MarkdownSegment::Table {
                        headers: std::mem::take(&mut table_headers),
                        rows: std::mem::take(&mut table_rows),
                    });
                }
                _ => {}
            },
            Event::Text(txt) => {
                if let Some((_, ref mut code)) = in_code_block {
                    code.push_str(&txt);
                } else {
                    let style = *active_styles.last().unwrap_or(&Style::default());
                    current_spans.push(Span::styled(txt.to_string(), style));
                }
            }
            Event::Code(code_txt) => {
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(30, 30, 45));
                current_spans.push(Span::styled(format!("`{}`", code_txt), code_style));
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block.is_none() && !in_table {
                    current_spans.push(Span::raw(" "));
                }
            }
            Event::Rule => {
                segments.push(MarkdownSegment::Rule);
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                let marker_style = Style::default()
                    .fg(if checked {
                        Color::Green
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD);
                current_spans.insert(0, Span::styled(marker, marker_style));
            }
            _ => {}
        }
    }

    if let Some((lang, code)) = in_code_block {
        segments.push(MarkdownSegment::CodeBlock {
            language: lang,
            code,
            is_unclosed: normalized.has_unclosed_fence,
        });
    }
    if !current_spans.is_empty() {
        if let Some(lvl) = current_heading_level {
            segments.push(MarkdownSegment::Heading {
                level: lvl,
                spans: current_spans,
            });
        } else if in_blockquote {
            segments.push(MarkdownSegment::BlockQuote(current_spans));
        } else {
            segments.push(MarkdownSegment::Paragraph(current_spans));
        }
    }

    segments
}
