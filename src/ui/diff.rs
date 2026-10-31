use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Header,
    HunkHeader,
    Addition,
    Deletion,
    Context,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

pub struct DiffViewer {
    pub lines: Vec<DiffLine>,
    pub scroll_offset: usize,
}

impl DiffViewer {
    pub fn new(diff_text: &str) -> Self {
        let mut lines = Vec::new();
        for line in diff_text.lines() {
            let kind = if line.starts_with("diff --git")
                || line.starts_with("index ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                DiffLineKind::Header
            } else if line.starts_with("@@") {
                DiffLineKind::HunkHeader
            } else if line.starts_with('+') && !line.starts_with("+++") {
                DiffLineKind::Addition
            } else if line.starts_with('-') && !line.starts_with("---") {
                DiffLineKind::Deletion
            } else {
                DiffLineKind::Context
            };

            lines.push(DiffLine {
                kind,
                text: line.to_string(),
            });
        }

        Self {
            lines,
            scroll_offset: 0,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        if self.lines.is_empty() {
            return;
        }
        self.scroll_offset = (self.scroll_offset + amount).min(self.lines.len() - 1);
    }

    pub fn render_lines(&self, height: usize) -> Vec<Line<'static>> {
        let start = self.scroll_offset.min(self.lines.len());
        let end = (start + height).min(self.lines.len());

        self.lines[start..end]
            .iter()
            .map(|l| {
                let style = match l.kind {
                    DiffLineKind::Header => Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                    DiffLineKind::HunkHeader => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    DiffLineKind::Addition => Style::default().fg(Color::Green),
                    DiffLineKind::Deletion => Style::default().fg(Color::Red),
                    DiffLineKind::Context => Style::default().fg(Color::Gray),
                };
                Line::from(Span::styled(l.text.clone(), style))
            })
            .collect()
    }
}
