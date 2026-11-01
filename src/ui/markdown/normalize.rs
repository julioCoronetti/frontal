#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedMarkdown {
    pub text: String,
    pub has_unclosed_fence: bool,
    pub fence_lang: Option<String>,
}

pub fn normalize_streaming_markdown(input: &str, consolidated: bool) -> NormalizedMarkdown {
    if consolidated || input.is_empty() {
        return NormalizedMarkdown {
            text: input.to_string(),
            has_unclosed_fence: false,
            fence_lang: None,
        };
    }

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0;
    let mut current_lang = None;

    for line in input.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker = trimmed.chars().next().unwrap();
                let count = trimmed.chars().take_while(|&c| c == marker).count();
                if count >= 3 {
                    in_fence = true;
                    fence_char = marker;
                    fence_len = count;
                    let info = trimmed[count..].trim();
                    let lang = info.split_whitespace().next().unwrap_or("");
                    current_lang = if lang.is_empty() {
                        None
                    } else {
                        Some(lang.to_string())
                    };
                }
            }
        } else {
            let marker_str = std::iter::repeat_n(fence_char, fence_len).collect::<String>();
            if trimmed.starts_with(&marker_str) {
                let rest = trimmed[fence_len..].trim();
                if rest.is_empty() {
                    in_fence = false;
                    current_lang = None;
                }
            }
        }
    }

    let mut output = input.to_string();
    let mut has_unclosed = false;

    if in_fence {
        has_unclosed = true;
        if !output.ends_with('\n') {
            output.push('\n');
        }
        for _ in 0..fence_len {
            output.push(fence_char);
        }
        output.push('\n');
    } else {
        let last_line = input.split('\n').next_back().unwrap_or("");
        let backticks = last_line.chars().filter(|&c| c == '`').count();
        if backticks % 2 != 0 {
            output.push('`');
        }
    }

    NormalizedMarkdown {
        text: output,
        has_unclosed_fence: has_unclosed,
        fence_lang: current_lang,
    }
}
