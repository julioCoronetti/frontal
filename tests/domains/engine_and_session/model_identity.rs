use frontal::config::{sanitize_model_id, UserConfig};
use frontal::transport::{
    build_opencode_args, model_with_variant, parse_opencode_models_output, strip_ansi, EngineConfig,
};
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

#[test]
fn test_transport_sanitization_and_ansi_control_filtering() {
    let mut cfg = EngineConfig::new_isolated();
    cfg.project = Some("--malicious\nproj".to_string());
    cfg.mode = Some("-ask\r".to_string());
    cfg.effort = Some("--high\x00".to_string());
    cfg.model = Some("--opencode-go/deepseek-v4.1-flash".to_string());

    let args = cfg.build_args("--conv\x07-1");
    assert_eq!(
        args.iter()
            .skip_while(|a| *a != "--conversation")
            .nth(1)
            .map(String::as_str),
        Some("conv-1")
    );
    assert_eq!(
        args.iter()
            .skip_while(|a| *a != "--project")
            .nth(1)
            .map(String::as_str),
        Some("maliciousproj")
    );
    assert_eq!(
        args.iter()
            .skip_while(|a| *a != "--mode")
            .nth(1)
            .map(String::as_str),
        Some("ask")
    );
    assert_eq!(
        args.iter()
            .skip_while(|a| *a != "--effort")
            .nth(1)
            .map(String::as_str),
        Some("high")
    );

    assert_eq!(
        model_with_variant("--provider/model", Some("medium")),
        "provider/model#high"
    );

    let cleaned = strip_ansi("\x1b(B\x1b[32mopencode-go/deepseek\x1b[0m\x07\n\t");
    assert_eq!(cleaned, "opencode-go/deepseek\n\t");

    let parsed = parse_opencode_models_output(
        "\x1b(Bopencode-go/deepseek-v4.1-flash\nbad provider/model\nprovider/bad model\n",
    );
    assert_eq!(
        parsed,
        vec![(
            "opencode-go/deepseek-v4.1-flash".to_string(),
            "deepseek-v4.1-flash".to_string()
        )]
    );
}

#[tokio::test]
async fn test_mode_permission_gating_toml_sanitization_and_respawn_guards() {
    use std::time::Duration;

    use frontal::config::BackendKind;
    use frontal::protocol::{PromptPayload, SessionId};
    use frontal::session::SessionManager;

    let _lock = crate::common::PGID_TEST_LOCK.lock().await;

    let mut cfg = UserConfig::new_isolated();
    cfg.parse_content("mode = \"ACCEPT_EDITS\"\neffort = \"HIGH!@#\"\n");
    assert_eq!(cfg.mode, "accept-edits");
    assert_eq!(cfg.effort, "high");
    assert!(cfg.needs_repair);

    cfg.mode = "plan\"\n[injected]\n".to_string();
    let toml = cfg.to_toml();
    assert!(!toml.contains("[injected]"));
    assert!(toml.contains("mode = \"planinjected\""));

    let mut engine_cfg = EngineConfig::new_isolated();
    assert!(engine_cfg.allows_auto_permissions());
    assert!(engine_cfg
        .build_acp_args()
        .contains(&"--dangerously-skip-permissions".to_string()));
    assert!(build_opencode_args(&engine_cfg, None, "hi").contains(&"--auto".to_string()));

    for restricted in ["plan", "accept-edits"] {
        engine_cfg.mode = Some(restricted.to_string());
        assert!(!engine_cfg.allows_auto_permissions());
        assert!(!engine_cfg
            .build_acp_args()
            .contains(&"--dangerously-skip-permissions".to_string()));
    }
    engine_cfg.mode = Some("plan".to_string());
    let oc_plan = build_opencode_args(&engine_cfg, None, "hi");
    assert!(!oc_plan.contains(&"--auto".to_string()));
    assert!(oc_plan.windows(2).any(|w| w == ["--agent", "plan"]));

    let script = r#"#!/usr/bin/env python3
import sys, json
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}}), flush=True)
    elif method == "session/new":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"acp-downgrade","configOptions":[{"id":"mode","currentValue":"auto","options":[{"value":"auto"}]}]}}), flush=True)
"#;
    let mock = crate::common::TempFileGuard::new("mock_acp_downgrade", "py", script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(mock.path(), perms).unwrap();
    }

    let mut manager = SessionManager::new();
    let mut acp_cfg = EngineConfig::new_isolated();
    acp_cfg.agy_bin = mock.path().to_path_buf();
    acp_cfg.mode = Some("plan".to_string());
    let sid = manager.create_subprocess_session(acp_cfg).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), manager.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        manager.get_metadata(&sid).unwrap().mode.as_deref(),
        Some("auto")
    );
    let _ = manager
        .shutdown_session(&sid, Duration::from_millis(200))
        .await;

    let mut app_mgr = SessionManager::new();
    let tab_sid = SessionId::new();
    app_mgr.register_mock_session(tab_sid.clone());
    let mut app = frontal::ui::app::App::new_isolated(app_mgr, tab_sid.clone());
    app.backend = BackendKind::OpenCode;
    app.user_config.backend = BackendKind::OpenCode;
    app.user_config.mode = "accept-edits".to_string();
    app.pending_engine_prompts
        .push_back((tab_sid.clone(), PromptPayload::from_text("stale")));
    app.respawn_active_session(None, None, None, Some("plan".to_string()))
        .await
        .unwrap();
    assert!(app.pending_engine_prompts.is_empty());

    app.cache.push_user_message("trigger reset");
    app.user_config.mode = "accept-edits".to_string();
    app.session_manager.set_session_metadata(
        &app.active_session,
        None,
        None,
        None,
        Some("accept-edits".to_string()),
    );
    app.close_or_reset_tab(0).await;
    assert_eq!(app.active_mode_name(), "auto");

    let rpc_err_script = r#"#!/usr/bin/env python3
import sys, json
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}}), flush=True)
    elif method == "session/new":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"acp-rpc-err","configOptions":[{"id":"mode","currentValue":"auto","options":[{"value":"auto"},{"value":"plan"}]}]}}), flush=True)
    elif method == "session/set_config_option":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"error":{"code":-32000,"message":"mode rejected"}}), flush=True)
"#;
    let err_mock = crate::common::TempFileGuard::new("mock_acp_mode_err", "py", rpc_err_script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(err_mock.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(err_mock.path(), perms).unwrap();
    }
    let mut err_mgr = SessionManager::new();
    let mut err_cfg = EngineConfig::new_isolated();
    err_cfg.agy_bin = err_mock.path().to_path_buf();
    err_cfg.mode = Some("plan".to_string());
    let err_sid = err_mgr.create_subprocess_session(err_cfg).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), err_mgr.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        err_mgr.get_metadata(&err_sid).unwrap().mode.as_deref(),
        Some("auto")
    );
    let _ = err_mgr
        .shutdown_session(&err_sid, Duration::from_millis(200))
        .await;
}
