use super::{danger_assessment, DangerAssessment, RiskCategory};

pub fn is_likely_sql(raw: &str) -> bool {
    let mut s = raw.trim();
    while s.starts_with("--") || s.starts_with("/*") {
        s = if s.starts_with("--") {
            s.find('\n').map(|p| &s[p + 1..]).unwrap_or("").trim()
        } else {
            s.find("*/").map(|p| &s[p + 2..]).unwrap_or("").trim()
        };
    }
    let first = s.split_whitespace().next().unwrap_or("");
    matches!(
        first.to_ascii_uppercase().as_str(),
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "DROP" | "TRUNCATE" | "ALTER" | "CREATE"
    )
}

pub fn check_sql_statement(raw: &str) -> Option<DangerAssessment> {
    let tokens = tokenize_sql(raw);
    let n = tokens.len();
    if n < 2 {
        return None;
    }
    for i in 0..n {
        let t1 = tokens[i].to_ascii_uppercase();
        if t1 == "DROP" && i + 1 < n {
            let t2 = tokens[i + 1].to_ascii_uppercase();
            if matches!(t2.as_str(), "DATABASE" | "SCHEMA" | "TABLE" | "VIEW") {
                let is_if_exists = tokens
                    .get(i + 2)
                    .is_some_and(|s| s.eq_ignore_ascii_case("IF"))
                    && tokens
                        .get(i + 3)
                        .is_some_and(|s| s.eq_ignore_ascii_case("EXISTS"));
                let target = if is_if_exists {
                    tokens.get(i + 4)
                } else {
                    tokens.get(i + 2)
                }
                .cloned()
                .unwrap_or_else(|| String::from("database entity"));
                return Some(danger_assessment(
                    RiskCategory::DestructiveSql,
                    raw,
                    target,
                    "Permanent drop of database schema or table",
                ));
            }
        }
        if t1 == "TRUNCATE" && i + 1 < n {
            let next = tokens[i + 1].to_ascii_uppercase();
            let target = if next == "TABLE" && i + 2 < n {
                tokens[i + 2].clone()
            } else {
                tokens[i + 1].clone()
            };
            return Some(danger_assessment(
                RiskCategory::DestructiveSql,
                raw,
                target,
                "Truncate operation erases all rows irreversibly",
            ));
        }
    }
    None
}

fn tokenize_sql(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let (mut in_single, mut in_comment) = (false, false);
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            i += 1;
            continue;
        }
        if !in_single && ((ch == '-' && i + 1 < len && chars[i + 1] == '-') || ch == '#') {
            in_comment = true;
            i += 1;
            continue;
        }
        if ch == '\'' {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if !in_single {
            if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                current.push(ch);
            } else if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        }
        i += 1;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}
