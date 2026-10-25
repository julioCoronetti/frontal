use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::sync::LazyLock;
use syntect::highlighting::{
    HighlightIterator, HighlightState, Highlighter, Style as SyntectStyle, Theme, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

pub static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
pub static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let ts = ThemeSet::load_defaults();
    ts.themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_else(|| ts.themes.values().next().cloned().unwrap_or_default())
});

#[derive(Clone)]
pub struct LineCheckpoint {
    pub line_content: String,
    pub parse_state: ParseState,
    pub highlight_state: HighlightState,
    pub spans: Vec<Span<'static>>,
}

#[derive(Clone, Default)]
pub struct IncrementalCodeHighlighter {
    pub language: String,
    pub checkpoints: Vec<LineCheckpoint>,
}

impl IncrementalCodeHighlighter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checkpoints_count(&self) -> usize {
        self.checkpoints.len()
    }

    fn syntect_style_to_ratatui(style: SyntectStyle) -> Style {
        let mut rat_style = Style::default().fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ));
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::BOLD)
        {
            rat_style = rat_style.add_modifier(Modifier::BOLD);
        }
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::ITALIC)
        {
            rat_style = rat_style.add_modifier(Modifier::ITALIC);
        }
        if style
            .font_style
            .contains(syntect::highlighting::FontStyle::UNDERLINE)
        {
            rat_style = rat_style.add_modifier(Modifier::UNDERLINED);
        }
        rat_style
    }

    pub fn highlight_code(&mut self, language: &str, code: &str) -> Vec<Vec<Span<'static>>> {
        if self.language != language {
            self.language = language.to_string();
            self.checkpoints.clear();
        }

        let syntax: &SyntaxReference = SYNTAX_SET
            .find_syntax_by_token(language)
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
        let highlighter = Highlighter::new(&THEME);

        let mut lines = Vec::new();
        let mut remainder = code;
        while let Some(idx) = remainder.find('\n') {
            lines.push((&remainder[..=idx], true));
            remainder = &remainder[idx + 1..];
        }
        if !remainder.is_empty() {
            lines.push((remainder, false));
        }

        let mut common_len = 0;
        while common_len < self.checkpoints.len()
            && common_len < lines.len()
            && lines[common_len].1
            && self.checkpoints[common_len].line_content == lines[common_len].0
        {
            common_len += 1;
        }
        self.checkpoints.truncate(common_len);

        let mut result = Vec::with_capacity(lines.len());
        for i in 0..common_len {
            result.push(self.checkpoints[i].spans.clone());
        }

        let (mut current_parse, mut current_highlight) = if let Some(last) = self.checkpoints.last()
        {
            (last.parse_state.clone(), last.highlight_state.clone())
        } else {
            (
                ParseState::new(syntax),
                HighlightState::new(&highlighter, ScopeStack::new()),
            )
        };

        for (line_str, is_complete) in lines.into_iter().skip(common_len) {
            if is_complete {
                let ops = current_parse
                    .parse_line(line_str, &SYNTAX_SET)
                    .unwrap_or_default();
                let iter = HighlightIterator::new(
                    &mut current_highlight,
                    &ops[..],
                    line_str,
                    &highlighter,
                );
                let mut line_spans = Vec::new();
                for (style, text) in iter {
                    let clean_text = text.trim_end_matches(['\r', '\n']);
                    if !clean_text.is_empty() {
                        line_spans.push(Span::styled(
                            clean_text.to_string(),
                            Self::syntect_style_to_ratatui(style),
                        ));
                    }
                }
                self.checkpoints.push(LineCheckpoint {
                    line_content: line_str.to_string(),
                    parse_state: current_parse.clone(),
                    highlight_state: current_highlight.clone(),
                    spans: line_spans.clone(),
                });
                result.push(line_spans);
            } else {
                let mut temp_parse = current_parse.clone();
                let mut temp_highlight = current_highlight.clone();
                let ops = temp_parse
                    .parse_line(line_str, &SYNTAX_SET)
                    .unwrap_or_default();
                let iter =
                    HighlightIterator::new(&mut temp_highlight, &ops[..], line_str, &highlighter);
                let mut line_spans = Vec::new();
                for (style, text) in iter {
                    let clean_text = text.trim_end_matches(['\r', '\n']);
                    if !clean_text.is_empty() {
                        line_spans.push(Span::styled(
                            clean_text.to_string(),
                            Self::syntect_style_to_ratatui(style),
                        ));
                    }
                }
                result.push(line_spans);
            }
        }

        result
    }
}
