#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeToken {
    Ident(String),
    DoubleColon,
    Punct(char),
}

pub struct ParsedSource {
    pub tokens: Vec<CodeToken>,
    pub string_literals: Vec<String>,
}

pub fn tokenize_rust_source(content: &str) -> ParsedSource {
    let chars: Vec<char> = content.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut prev_char = ' ';
    let mut tokens = Vec::new();
    let mut string_literals = Vec::new();

    while i < n {
        let ch = chars[i];
        if ch.is_whitespace() {
            prev_char = ch;
            i += 1;
            continue;
        }

        if ch == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            prev_char = ' ';
            continue;
        }

        if ch == '/' && i + 1 < n && chars[i + 1] == '*' {
            i += 2;
            let mut depth = 1;
            while i < n && depth > 0 {
                if chars[i..].starts_with(&['/', '*']) {
                    depth += 1;
                    i += 1;
                } else if chars[i..].starts_with(&['*', '/']) {
                    depth -= 1;
                    i += 1;
                }
                i += 1;
            }
            prev_char = ' ';
            continue;
        }

        if ch == '\'' {
            if i + 2 < n && chars[i + 1] != '\\' && chars[i + 2] == '\'' {
                i += 3;
                prev_char = '\'';
                continue;
            } else if i + 3 < n && chars[i + 1] == '\\' && chars[i + 3] == '\'' {
                i += 4;
                prev_char = '\'';
                continue;
            } else if i + 1 < n && chars[i + 1] == '\\' {
                let mut end = i + 2;
                while end < n && end - i <= 10 && chars[end] != '\n' && chars[end] != '\'' {
                    end += 1;
                }
                if end < n && chars[end] == '\'' {
                    i = end + 1;
                    prev_char = '\'';
                    continue;
                }
            }
        }

        let is_ident_prev = prev_char.is_alphanumeric() || prev_char == '_';
        let is_raw_lead =
            ch == 'r' || ((ch == 'b' || ch == 'c') && i + 1 < n && chars[i + 1] == 'r');
        if is_raw_lead && !is_ident_prev {
            let r_idx = if ch == 'r' { i } else { i + 1 };
            let mut h = r_idx + 1;
            while h < n && chars[h] == '#' {
                h += 1;
            }
            if h < n && chars[h] == '"' {
                let hashes = h - (r_idx + 1);
                i = h + 1;
                let start = i;
                let mut closing = Vec::with_capacity(1 + hashes);
                closing.push('"');
                closing.resize(1 + hashes, '#');
                let mut end = n;
                while i < n {
                    if chars[i..].starts_with(&closing) {
                        end = i;
                        i += closing.len();
                        break;
                    }
                    i += 1;
                }
                string_literals.push(chars[start..end].iter().collect());
                prev_char = if hashes > 0 { '#' } else { '"' };
                continue;
            }
        }

        if ch == '"' {
            i += 1;
            let mut lit = String::new();
            while i < n {
                if chars[i] == '\\' {
                    i += 1;
                    if i < n {
                        lit.push(chars[i]);
                    }
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    lit.push(chars[i]);
                }
                i += 1;
            }
            string_literals.push(lit);
            prev_char = '"';
            continue;
        }

        if ch == ':' && i + 1 < n && chars[i + 1] == ':' {
            tokens.push(CodeToken::DoubleColon);
            i += 2;
            prev_char = ':';
            continue;
        }

        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(CodeToken::Ident(ident));
            prev_char = chars[i - 1];
            continue;
        }

        tokens.push(CodeToken::Punct(ch));
        prev_char = ch;
        i += 1;
    }

    ParsedSource {
        tokens,
        string_literals,
    }
}

pub fn has_ident(tokens: &[CodeToken], target: &str) -> bool {
    tokens
        .iter()
        .any(|t| matches!(t, CodeToken::Ident(id) if id == target))
}

fn matches_group_direct_child(tokens: &[CodeToken], brace_idx: usize, member: &str) -> bool {
    let mut depth = 1usize;
    let mut at_item_start = true;
    let mut j = brace_idx + 1;
    while j < tokens.len() && depth > 0 {
        match &tokens[j] {
            CodeToken::Punct('{') => {
                if depth == 1 {
                    at_item_start = false;
                }
                depth += 1;
            }
            CodeToken::Punct('}') => {
                depth -= 1;
                if depth == 1 {
                    at_item_start = false;
                }
            }
            CodeToken::Punct(',') if depth == 1 => {
                at_item_start = true;
            }
            CodeToken::Ident(inner) if depth == 1 && at_item_start => {
                if inner == member {
                    return true;
                }
                at_item_start = false;
            }
            _ if depth == 1 => {
                at_item_start = false;
            }
            _ => {}
        }
        j += 1;
    }
    false
}

pub fn has_path_or_grouped_use(tokens: &[CodeToken], root: &str, member: &str) -> bool {
    let mut i = 0;
    while i < tokens.len() {
        if let CodeToken::Ident(id) = &tokens[i] {
            if id == root && matches!(tokens.get(i + 1), Some(CodeToken::DoubleColon)) {
                match tokens.get(i + 2) {
                    Some(CodeToken::Ident(next)) if next == member => return true,
                    Some(CodeToken::Punct('{'))
                        if matches_group_direct_child(tokens, i + 2, member) =>
                    {
                        return true
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    false
}

pub fn has_super_escape(tokens: &[CodeToken], target: &str) -> bool {
    let mut i = 0;
    while i < tokens.len() {
        if matches!(&tokens[i], CodeToken::Ident(id) if id == "super") {
            let mut j = i;
            while matches!(tokens.get(j), Some(CodeToken::Ident(id)) if id == "super")
                && matches!(tokens.get(j + 1), Some(CodeToken::DoubleColon))
            {
                j += 2;
            }
            if j > i {
                match tokens.get(j) {
                    Some(CodeToken::Ident(next)) if next == target => return true,
                    Some(CodeToken::Punct('{'))
                        if matches_group_direct_child(tokens, j, target) =>
                    {
                        return true
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    false
}
