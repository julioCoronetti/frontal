use frontal::config::{sanitize_model_id, UserConfig};
use frontal::transport::build_opencode_args;
use frontal::transport::EngineConfig;
use frontal::ui::labels::model_display_name;

#[test]
fn test_sanitize_model_id_matrix() {
    assert_eq!(
        sanitize_model_id("opencode-go/deepseek-v4.1-flash]"),
        "opencode-go/deepseek-v4.1-flash"
    );
    assert_eq!(
        sanitize_model_id("opencode-go/deepseek-v4.1-flash]#max"),
        "opencode-go/deepseek-v4.1-flash#max"
    );
    assert_eq!(
        sanitize_model_id("[opencode-go/deepseek-v4.1-flash"),
        "opencode-go/deepseek-v4.1-flash"
    );
    assert_eq!(
        sanitize_model_id("anthropic/claude-3-5-sonnet@20241022"),
        "anthropic/claude-3-5-sonnet@20241022"
    );
    assert_eq!(
        sanitize_model_id("google/gemini-2.0-flash:free"),
        "google/gemini-2.0-flash:free"
    );
    assert_eq!(sanitize_model_id("  model name\t"), "modelname");
    assert_eq!(sanitize_model_id(""), "");
    assert_eq!(sanitize_model_id("]"), "");
    assert_eq!(sanitize_model_id("m#"), "m");
}

#[test]
fn test_config_repairs_corrupted_model_and_flags_repair() {
    let mut config = UserConfig::new_isolated();
    assert!(!config.needs_repair);

    config.parse_content(
        "model = \"opencode-go/deepseek-v4.1-flash]\"\n\n[context_windows]\n\"opencode-go/deepseek-v4.1-flash]\" = 1_000_000\n",
    );
    assert_eq!(config.model, "opencode-go/deepseek-v4.1-flash");
    assert!(config.needs_repair);
    assert_eq!(
        config.context_windows,
        vec![("opencode-go/deepseek-v4.1-flash".to_string(), 1_000_000)]
    );
    let rendered = config.to_toml();
    assert!(rendered.contains("model = \"opencode-go/deepseek-v4.1-flash\""));
    assert!(!rendered.contains("flash]"));

    let mut clean = UserConfig::new_isolated();
    clean.parse_content("model = \"opencode-go/deepseek-v4.1-flash\"\n");
    assert!(!clean.needs_repair);
}

#[test]
fn test_engine_argument_builders_sanitize_model_ids() {
    let mut config = EngineConfig::new_isolated();
    config.model = Some("opencode-go/deepseek-v4.1-flash]".to_string());
    config.effort = Some("high".to_string());

    let agy = config.build_args("conv-1");
    let agy_model = agy
        .iter()
        .position(|arg| arg == "--model")
        .map(|index| agy[index + 1].as_str());
    assert_eq!(agy_model, Some("opencode-go/deepseek-v4.1-flash"));

    let opencode = build_opencode_args(&config, None, "hi");
    let opencode_model = opencode
        .iter()
        .position(|arg| arg == "--model")
        .map(|index| opencode[index + 1].as_str());
    assert_eq!(opencode_model, Some("opencode-go/deepseek-v4.1-flash#max"));
}

#[test]
fn test_labels_clean_stray_punctuation() {
    assert_eq!(
        model_display_name("opencode-go/deepseek-v4.1-flash]"),
        "DeepSeek V4.1 Flash"
    );
    assert_eq!(
        model_display_name("opencode-go/deepseek-v4.1-flash]#max"),
        "DeepSeek V4.1 Flash"
    );
    assert_eq!(
        model_display_name("anthropic/claude-3-5-sonnet@20241022"),
        "Claude 3.5 Sonnet20241022"
    );
}
