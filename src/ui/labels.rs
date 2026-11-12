pub fn prettify_slug(slug: &str) -> String {
    let tokens: Vec<&str> = slug
        .split(['-', '_'])
        .filter(|token| !token.is_empty())
        .collect();
    let mut words: Vec<String> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let next_numeric = tokens
            .get(index + 1)
            .map(|next| next.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        if token.chars().all(|c| c.is_ascii_digit()) && next_numeric {
            words.push(format!("{}.{}", token, tokens[index + 1]));
            index += 2;
            continue;
        }
        words.push(pretty_token(token));
        index += 1;
    }
    words.join(" ")
}

fn pretty_token(token: &str) -> String {
    let lower = token.to_lowercase();
    match lower.as_str() {
        "gpt" => "GPT".to_string(),
        "glm" => "GLM".to_string(),
        "opencode" => "OpenCode".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "mimo" => "MiMo".to_string(),
        "minimax" => "MiniMax".to_string(),
        "longcat" => "LongCat".to_string(),
        "kimi" => "Kimi".to_string(),
        "qwen" => "Qwen".to_string(),
        "grok" => "Grok".to_string(),
        "claude" => "Claude".to_string(),
        "gemini" => "Gemini".to_string(),
        "sonnet" => "Sonnet".to_string(),
        "haiku" => "Haiku".to_string(),
        "opus" => "Opus".to_string(),
        "luna" => "Luna".to_string(),
        "argon" => "Argon".to_string(),
        "flash" => "Flash".to_string(),
        "pro" => "Pro".to_string(),
        "max" => "Max".to_string(),
        "mini" => "Mini".to_string(),
        "free" => "Free".to_string(),
        "preview" => "Preview".to_string(),
        "code" => "Code".to_string(),
        "pickle" => "Pickle".to_string(),
        "step" => "Step".to_string(),
        "muse" => "Muse".to_string(),
        "spark" => "Spark".to_string(),
        "beam" => "Beam".to_string(),
        "iceman" => "Iceman".to_string(),
        "contributor" => "Contributor".to_string(),
        _ => {
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

pub fn provider_display_name(provider_id: &str) -> String {
    match provider_id {
        "agy" | "antigravity" => "Antigravity".to_string(),
        other => prettify_slug(other),
    }
}

pub fn model_display_name(model_id: &str) -> String {
    let base = model_id
        .split_once('/')
        .map(|(_, model)| model)
        .unwrap_or(model_id);
    prettify_slug(crate::transport::EngineConfig::strip_effort_suffix(base))
}
