use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use frontal::infra::usage::UsageService;
use frontal::protocol::TokenUsage;
use frontal::ui::modal::usage::{
    build_stats_lines, parse_opencode_stats_json, StatsState, UsagePage,
};
use frontal::ui::modal::{ModalAction, UsageModal};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::common::{TempDirGuard, PGID_TEST_LOCK};

struct EnvRestore {
    key: &'static str,
    prev: Option<String>,
}

impl EnvRestore {
    fn set(key: &'static str, val: &std::path::Path) -> Self {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, val);
        Self { key, prev }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn render_usage_text(modal: &UsageModal) -> String {
    let backend = TestBackend::new(72, 26);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| modal.render(f, Rect::new(0, 0, 72, 26)))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn test_parse_opencode_stats_json_and_build_lines() {
    let raw = r#"{
        "sessions": 20,
        "subagents": 4,
        "prompts": 85,
        "steps": 588,
        "tokens": {
            "input": 3701028,
            "output": 208755,
            "reasoning": 224484,
            "cache": { "read": 106882604, "write": 1200 }
        },
        "cost": 1.25,
        "activeDays": 4,
        "streak": 3,
        "tools": {
            "totals": { "calls": 100, "succeeded": 95, "failed": 5, "unfinished": 0 },
            "usage": [
                { "name": "read", "calls": 20, "succeeded": 19, "failed": 1, "durationP50": 15.0 },
                { "name": "shell", "calls": 80, "succeeded": 76, "failed": 4, "durationP50": 120.0 }
            ]
        },
        "models": [
            {
                "model": { "id": "gemini-4-argon", "providerID": "opencode-go", "variant": "high" },
                "steps": 10,
                "tokens": { "input": 100, "output": 50, "reasoning": 20, "cache": { "read": 30 } },
                "cost": 0.25
            },
            {
                "model": { "id": "deepseek-v4.1-flash", "providerID": "opencode-go", "variant": "default" },
                "steps": 578,
                "tokens": { "input": 3000000, "output": 200000, "reasoning": 200000, "cache": { "read": 100000000 } },
                "cost": 1.0
            }
        ]
    }"#;

    let stats = parse_opencode_stats_json(raw).unwrap();
    assert_eq!(stats.sessions, 20);
    assert_eq!(stats.steps, 588);
    assert_eq!(stats.cost, 1.25);
    assert_eq!(stats.cache_write_tokens, 1200);
    assert_eq!(stats.models[0].id, "deepseek-v4.1-flash");
    assert_eq!(stats.models[0].variant, None);
    assert_eq!(stats.models[1].variant.as_deref(), Some("high"));
    assert_eq!(stats.tools[0].name, "shell");

    let lines = build_stats_lines(&StatsState::Ready(stats));
    let rendered: String = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("OVERVIEW"));
    assert!(rendered.contains("$1.2500"));
    assert!(rendered.contains("GLOBAL TOKENS"));
    assert!(rendered.contains("3.70M / 208.8K"));
    assert!(rendered.contains("CONSUMPTION BY MODEL"));
    assert!(rendered.contains("DeepSeek V4.1 Flash"));
    assert!(rendered.contains("TOOL RELIABILITY"));
    assert!(rendered.contains("95.0% (95/100 ok, 5 failed)"));
    assert!(rendered.contains("shell"));

    let dirty_json = r#"{"cost":-5,"tools":{"totals":{"calls":2,"succeeded":99,"failed":0},"usage":[{"name":"bad\ntool\tname_exceeding_12","calls":1,"succeeded":5,"failed":0,"durationP50":-10}]},"models":[]}"#;
    let sanitized = parse_opencode_stats_json(dirty_json).unwrap();
    assert_eq!(sanitized.cost, 0.0);
    assert_eq!(sanitized.tools[0].name, "badtoolname_exceeding_12");
    assert_eq!(sanitized.tools[0].duration_p50_ms, None);
    let sanitized_lines = build_stats_lines(&StatsState::Ready(sanitized))
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(sanitized_lines.contains("100.0%"));

    assert_eq!(
        frontal::transport::strip_ansi("\u{1b}[2@opencode-go/model\u{1b}]0;title\x07\u{1b}[0~"),
        "opencode-go/model"
    );

    let mut offline_modal = UsageModal::for_opencode(
        "opencode-go/deepseek-v4.1-flash",
        None,
        TokenUsage::default(),
    )
    .with_stats_cli(false);
    offline_modal.open_stats_subpage();
    let offline_state = offline_modal.stats_state.borrow().clone();
    assert!(matches!(offline_state, StatsState::Unavailable(ref msg) if msg == "Offline mode"));
}

#[tokio::test]
async fn test_opencode_usage_modal_subpage_navigation_and_refresh() {
    let _lock = PGID_TEST_LOCK.lock().await;

    let dir = TempDirGuard::new("opencode_stats_bin");
    let count_file = dir.path().join("invocations.txt");
    let script_path = dir.path().join("opencode");
    let script = format!(
        "#!/usr/bin/env bash\necho \"$*\" >> \"{}\"\ncat <<'JSON'\n{{\"sessions\":7,\"subagents\":1,\"prompts\":19,\"steps\":42,\"tokens\":{{\"input\":5000,\"output\":1200,\"reasoning\":300,\"cache\":{{\"read\":9000,\"write\":0}}}},\"cost\":0.42,\"activeDays\":2,\"streak\":2,\"tools\":{{\"totals\":{{\"calls\":10,\"succeeded\":10,\"failed\":0,\"unfinished\":0}},\"usage\":[{{\"name\":\"read\",\"calls\":10,\"succeeded\":10,\"failed\":0,\"durationP50\":12}}]}},\"models\":[]}}\nJSON\n",
        count_file.display()
    );
    std::fs::write(&script_path, script).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    let _env_guard = EnvRestore::set("FRONTAL_OPENCODE_BIN", &script_path);

    let mut agy_modal = UsageModal::offline_fallback();
    assert!(!agy_modal.is_opencode);
    assert_eq!(
        agy_modal.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
        ModalAction::Close
    );

    let mut modal = UsageModal::for_opencode(
        "opencode-go/deepseek-v4.1-flash",
        Some(TokenUsage::default()),
        TokenUsage::default(),
    );
    assert!(modal.is_opencode);
    assert_eq!(modal.page, UsagePage::Main);
    let main_rendered = render_usage_text(&modal);
    assert!(main_rendered.contains("Global Usage & Tool Stats"));

    modal.scroll_offset = 2;
    let act = modal.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(act, ModalAction::None);
    assert_eq!(modal.page, UsagePage::OpenCodeStats);
    assert_eq!(modal.scroll_offset, 0);

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if modal.poll_rx() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!modal.is_loading());
    let stats_rendered = render_usage_text(&modal);
    assert!(stats_rendered.contains("OpenCode Global Stats:"));
    assert!(stats_rendered.contains("$0.4200"));
    assert!(stats_rendered.contains("100.0%"));

    assert_eq!(
        modal.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
        ModalAction::None
    );

    modal.handle_key(key(KeyCode::Char('r'), KeyModifiers::NONE));
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if modal.poll_rx() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let calls = std::fs::read_to_string(&count_file).unwrap();
    assert_eq!(calls.lines().count(), 2);

    modal.scroll_offset = 3;
    let back_act = modal.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(back_act, ModalAction::None);
    assert_eq!(modal.page, UsagePage::Main);
    assert_eq!(modal.scroll_offset, 0);

    let close_act = modal.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(close_act, ModalAction::Close);
}

#[tokio::test]
async fn test_opencode_stats_error_and_command_timeout() {
    let _lock = PGID_TEST_LOCK.lock().await;

    let dir = TempDirGuard::new("opencode_stats_err");
    let fail_script = dir.path().join("opencode_fail");
    std::fs::write(
        &fail_script,
        "#!/usr/bin/env bash\necho 'boom' >&2\nexit 1\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fail_script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fail_script, perms).unwrap();

    let _env_guard = EnvRestore::set("FRONTAL_OPENCODE_BIN", &fail_script);

    let mut modal = UsageModal::for_opencode(
        "opencode-go/deepseek-v4.1-flash",
        None,
        TokenUsage::default(),
    );
    modal.open_stats_subpage();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if modal.poll_rx() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let state = modal.stats_state.borrow().clone();
    assert!(matches!(state, StatsState::Unavailable(ref msg) if msg.contains("boom")));

    let hang_script = dir.path().join("opencode_hang");
    std::fs::write(&hang_script, "#!/usr/bin/env bash\nsleep 10\n").unwrap();
    let mut perms = std::fs::metadata(&hang_script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&hang_script, perms).unwrap();
    let _hang_env = EnvRestore::set("FRONTAL_OPENCODE_BIN", &hang_script);

    let res =
        frontal::transport::run_opencode_command(&["stats", "--json"], Duration::from_millis(150));
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("timed out"));
}

#[test]
fn test_usage_modal_for_opencode_enables_stats_cli_by_default_and_validates_token() {
    let default_modal = UsageModal::for_opencode(
        "opencode-go/deepseek-v4.1-flash",
        None,
        TokenUsage::default(),
    );
    assert!(default_modal.allow_stats_cli);

    let mut offline = default_modal.with_stats_cli(false);
    offline.open_stats_subpage();
    let offline_state = offline.stats_state.borrow().clone();
    assert!(matches!(offline_state, StatsState::Unavailable(ref msg) if msg == "Offline mode"));

    let bad_tok = frontal::infra::usage::opencode_api::fetch_go_usage("bad\"token\nheader");
    assert!(bad_tok.is_err());
    assert!(bad_tok.unwrap_err().contains("invalid credential token"));
    assert!(
        frontal::ui::modal::usage::opencode_api::parse_iso_ms("2026--1-01T00:00:00Z").is_none()
    );
    assert_eq!(
        frontal::ui::modal::usage::format_reset_time("2026--1-01T00:00:00Z"),
        "Resets periodically"
    );
}

#[tokio::test]
async fn test_stats_fetch_async_coalesces_concurrent_waiters() {
    let _lock = PGID_TEST_LOCK.lock().await;
    let service = UsageService::new();
    let dir = TempDirGuard::new("stats_coalesce");
    let script = dir.path().join("opencode");
    let body = r#"{"models":{"opencode-go/deepseek-v4.1-flash":{"requests":12,"input":100,"output":50,"cacheRead":25,"cost":0.1}}}"#;
    let script_src = format!("#!/usr/bin/env bash\nsleep 0.05\nprintf '%s' '{}'\n", body);
    std::fs::write(&script, script_src).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    let _env_guard = EnvRestore::set("FRONTAL_OPENCODE_BIN", &script);

    let (tx1, rx1) = std::sync::mpsc::channel();
    let (tx2, rx2) = std::sync::mpsc::channel();
    service.fetch_stats_async(1, tx1);
    service.fetch_stats_async(2, tx2);

    let (id1, s1) = rx1.recv_timeout(Duration::from_secs(3)).unwrap();
    let (id2, s2) = rx2.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert!(matches!(s1, StatsState::Ready(_)));
    assert!(matches!(s2, StatsState::Ready(_)));
}

#[test]
fn test_usage_modal_wraps_long_model_descriptions_and_errors() {
    use frontal::ui::modal::usage::{QuotaBucket, QuotaGroup, UsagePlanInfo, UsageSummary};

    let summary = UsageSummary {
        plan: UsagePlanInfo {
            tier_name: "Google AI Pro".to_string(),
            account_email: None,
            upgrade_url: None,
            manage_url: None,
        },
        groups: vec![QuotaGroup {
            name: "GEMINI MODELS".to_string(),
            description: Some(
                "Models within this group: Gemini 4 Neon, Gemini 4 Hydrogen, Gemini 4 Argon, Orchestrator Pro"
                    .to_string(),
            ),
            buckets: vec![QuotaBucket {
                name: "Weekly Limit Remaining".to_string(),
                remaining_pct: 80.0,
                reset_label: "Quota available".to_string(),
                exhausted: false,
            }],
        }],
    };
    let modal = UsageModal::with_data(summary);
    let backend = TestBackend::new(56, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| modal.render(f, Rect::new(0, 0, 56, 20)))
        .unwrap();
    let rows: Vec<String> = (0..20)
        .map(|y| {
            (0..56)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    let full = rows.join("\n");
    assert!(full.contains("Gemini 4 Neon"));
    assert!(full.contains("Orchestrator"));
    assert!(full.contains("Pro"));
    assert!(rows
        .iter()
        .any(|r| r.contains("     ") && r.contains("Orchestrator")));
}

#[test]
fn test_usage_modal_body_cache_invalidates_on_resize_and_state_change() {
    use frontal::ui::modal::usage::{
        QuotaBucket, QuotaGroup, StatsState, UsagePage, UsagePlanInfo, UsageState, UsageSummary,
    };
    use frontal::ui::modal::UsageModal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    let mut modal = UsageModal::with_data(UsageSummary {
        plan: UsagePlanInfo {
            tier_name: "Pro".to_string(),
            account_email: None,
            upgrade_url: None,
            manage_url: None,
        },
        groups: vec![QuotaGroup {
            name: "G".to_string(),
            description: Some("Alpha Beta Gamma Delta Epsilon Zeta".to_string()),
            buckets: vec![QuotaBucket {
                name: "Weekly".to_string(),
                remaining_pct: 50.0,
                reset_label: "1h".to_string(),
                exhausted: false,
            }],
        }],
    });

    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| modal.render(f, Rect::new(0, 0, 80, 20)))
        .unwrap();
    let wide_max_scroll = modal.max_scroll.get();

    term.draw(|f| modal.render(f, Rect::new(0, 0, 24, 10)))
        .unwrap();
    let narrow_max_scroll = modal.max_scroll.get();
    assert!(narrow_max_scroll > wide_max_scroll);

    *modal.state.borrow_mut() = UsageState::Unavailable("offline".to_string());
    term.draw(|f| modal.render(f, Rect::new(0, 0, 80, 20)))
        .unwrap();
    let buf = term.backend().buffer();
    let screen: String = (0..20)
        .map(|y| (0..80).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("Status: offline"));

    modal.page = UsagePage::OpenCodeStats;
    *modal.stats_state.borrow_mut() = StatsState::Unavailable("stats err".to_string());
    term.draw(|f| modal.render(f, Rect::new(0, 0, 80, 20)))
        .unwrap();
    let buf2 = term.backend().buffer();
    let stats_screen: String = (0..20)
        .map(|y| (0..80).map(|x| buf2[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(stats_screen.contains("stats err"));
}
