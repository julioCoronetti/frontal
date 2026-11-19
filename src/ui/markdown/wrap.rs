use ratatui::style::Style;
use ratatui::text::{Line, Span};

enum WrapToken {
    Word(String),
    Space(String),
    Newline,
}

fn tokenize_span(content: &str) -> Vec<WrapToken> {
    let mut tokens = Vec::new();
    let mut current_word = String::new();

    for c in content.chars() {
        if c == '\r' {
            continue;
        } else if c == '\n' {
            if !current_word.is_empty() {
                tokens.push(WrapToken::Word(std::mem::take(&mut current_word)));
            }
            tokens.push(WrapToken::Newline);
        } else if c == '\t' {
            if !current_word.is_empty() {
                tokens.push(WrapToken::Word(std::mem::take(&mut current_word)));
            }
            tokens.push(WrapToken::Space(String::from("    ")));
        } else if c.is_whitespace() {
            if !current_word.is_empty() {
                tokens.push(WrapToken::Word(std::mem::take(&mut current_word)));
            }
            tokens.push(WrapToken::Space(c.to_string()));
        } else {
            current_word.push(c);
        }
    }
    if !current_word.is_empty() {
        tokens.push(WrapToken::Word(current_word));
    }
    tokens
}

pub fn wrap_styled_spans(
    spans: &[Span<'static>],
    max_width: usize,
    first_prefix: &[Span<'static>],
    cont_prefix: &[Span<'static>],
) -> Vec<Line<'static>> {
    let target_width = max_width.max(1);
    let first_pw: usize = first_prefix.iter().map(|s| s.content.chars().count()).sum();
    let cont_pw: usize = cont_prefix.iter().map(|s| s.content.chars().count()).sum();

    let mut lines = Vec::new();
    let mut current_line = first_prefix.to_vec();
    let mut current_width = first_pw;
    let mut on_cont = false;
    let mut just_soft_wrapped = false;
    let mut ended_with_newline = false;

    for span in spans {
        let style = span.style;
        for token in tokenize_span(&span.content) {
            let active_prefix_len = if on_cont {
                cont_prefix.len()
            } else {
                first_prefix.len()
            };
            match token {
                WrapToken::Newline => {
                    lines.push(Line::from(current_line));
                    current_line = cont_prefix.to_vec();
                    current_width = cont_pw;
                    on_cont = true;
                    just_soft_wrapped = false;
                    ended_with_newline = true;
                }
                WrapToken::Space(ws) => {
                    if just_soft_wrapped {
                        continue;
                    }
                    let token_len = ws.chars().count();
                    if current_width + token_len <= target_width {
                        current_line.push(Span::styled(ws, style));
                        current_width += token_len;
                        ended_with_newline = false;
                    } else if current_line.len() > active_prefix_len {
                        lines.push(Line::from(current_line));
                        current_line = cont_prefix.to_vec();
                        current_width = cont_pw;
                        on_cont = true;
                        just_soft_wrapped = true;
                        ended_with_newline = false;
                    } else {
                        let avail = target_width.saturating_sub(current_width);
                        if avail > 0 {
                            let clamped: String = ws.chars().take(avail).collect();
                            current_line.push(Span::styled(clamped, style));
                            current_width += avail;
                            ended_with_newline = false;
                        }
                    }
                }
                WrapToken::Word(word) => {
                    just_soft_wrapped = false;
                    ended_with_newline = false;
                    let token_len = word.chars().count();
                    if current_width + token_len <= target_width {
                        current_line.push(Span::styled(word, style));
                        current_width += token_len;
                    } else {
                        if current_line.len() > active_prefix_len
                            || (!on_cont
                                && first_pw > cont_pw
                                && (token_len <= target_width.saturating_sub(cont_pw)
                                    || current_width >= target_width))
                        {
                            lines.push(Line::from(current_line));
                            current_line = cont_prefix.to_vec();
                            current_width = cont_pw;
                            on_cont = true;
                        }
                        if current_width + token_len <= target_width {
                            current_line.push(Span::styled(word, style));
                            current_width += token_len;
                        } else {
                            let mut chunk = String::new();
                            for ch in word.chars() {
                                let cur_prefix_len = if on_cont {
                                    cont_prefix.len()
                                } else {
                                    first_prefix.len()
                                };
                                if current_width >= target_width
                                    && (current_line.len() > cur_prefix_len || !chunk.is_empty())
                                {
                                    if !chunk.is_empty() {
                                        current_line
                                            .push(Span::styled(std::mem::take(&mut chunk), style));
                                    }
                                    lines.push(Line::from(current_line));
                                    current_line = cont_prefix.to_vec();
                                    current_width = cont_pw;
                                    on_cont = true;
                                }
                                chunk.push(ch);
                                current_width += 1;
                            }
                            if !chunk.is_empty() {
                                current_line.push(Span::styled(chunk, style));
                            }
                        }
                    }
                }
            }
        }
    }

    let active_prefix_len = if on_cont {
        cont_prefix.len()
    } else {
        first_prefix.len()
    };
    if current_line.len() > active_prefix_len || lines.is_empty() || ended_with_newline {
        lines.push(Line::from(current_line));
    }

    lines
}

pub fn wrap_lines_preserving_indent(
    lines: Vec<Line<'static>>,
    max_width: usize,
) -> Vec<Line<'static>> {
    let target_width = max_width.max(1);
    let mut wrapped = Vec::with_capacity(lines.len());
    for line in lines {
        if line.spans.is_empty() {
            wrapped.push(Line::raw(""));
            continue;
        }
        let total_chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        let has_newline_or_tab = line
            .spans
            .iter()
            .any(|s| s.content.contains('\n') || s.content.contains('\t'));
        if total_chars == 0 && !has_newline_or_tab {
            wrapped.push(Line::raw(""));
            continue;
        }
        if total_chars <= target_width && !has_newline_or_tab {
            wrapped.push(line);
            continue;
        }
        let (indent, body_spans) = split_leading_spaces(line.spans, target_width.saturating_sub(4));
        if indent == 0 {
            wrapped.extend(wrap_styled_spans(&body_spans, target_width, &[], &[]));
        } else {
            let prefix = [Span::styled(" ".repeat(indent), Style::default())];
            wrapped.extend(wrap_styled_spans(
                &body_spans,
                target_width,
                &prefix,
                &prefix,
            ));
        }
    }
    wrapped
}

fn split_leading_spaces(
    spans: Vec<Span<'static>>,
    max_indent: usize,
) -> (usize, Vec<Span<'static>>) {
    let mut indent = 0usize;
    let mut out = Vec::with_capacity(spans.len());
    let mut counting = true;
    for span in spans {
        if !counting || span.style.bg.is_some() {
            counting = false;
            out.push(span);
            continue;
        }
        let mut taken = 0usize;
        let mut added_cols = 0usize;
        for ch in span.content.chars() {
            if ch == ' ' && indent + added_cols < max_indent {
                taken += 1;
                added_cols += 1;
            } else if ch == '\t' && indent + added_cols + 4 <= max_indent {
                taken += 1;
                added_cols += 4;
            } else {
                counting = false;
                break;
            }
        }
        indent += added_cols;
        let rest: String = span.content.chars().skip(taken).collect();
        if !rest.is_empty() {
            out.push(Span::styled(rest, span.style));
        }
    }
    (indent, out)
}
