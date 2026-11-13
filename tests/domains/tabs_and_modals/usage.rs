use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use frontal::model_spec::ModelSpec;
use frontal::protocol::{SessionId, TokenUsage};
use frontal::ui::modal::usage::{
    fetch::{
        build_usage_summary, parse_available_models, parse_plan_info, parse_quota_summary_groups,
    },
    format_reset_time,
    render::render_progress_bar,
    QuotaBucket, QuotaGroup, UsagePlanInfo, UsageState, UsageSummary,
};
use frontal::ui::modal::{Modal, ModalAction, UsageModal};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Terminal;

use crate::common::{press_key, test_app};

#[test]
fn test_token_usage_accumulate_and_metrics() {
    let mut total = TokenUsage::default();
    assert_eq!(total.effective_total(), 0);
    assert_eq!(total.cache_hit_percentage(), 0.0);

    let t1 = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        thinking_tokens: 20,
        cache_read_tokens: 300,
        total_tokens: 0,
    };
    assert_eq!(t1.effective_total(), 470);
    assert_eq!(t1.cache_hit_percentage(), 75.0);

    total.accumulate(&t1);
    assert_eq!(total.input_tokens, 100);
    assert_eq!(total.output_tokens, 50);
    assert_eq!(total.thinking_tokens, 20);
    assert_eq!(total.cache_read_tokens, 300);
    assert_eq!(total.total_tokens, 470);

    let t2 = TokenUsage {
        input_tokens: 200,
        output_tokens: 100,
        thinking_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 300,
    };
    total.accumulate(&t2);
    assert_eq!(total.input_tokens, 300);
    assert_eq!(total.output_tokens, 150);
    assert_eq!(total.thinking_tokens, 20);
    assert_eq!(total.cache_read_tokens, 300);
    assert_eq!(total.total_tokens, 770);
    assert_eq!(total.cache_hit_percentage(), 50.0);
}

#[test]
fn test_model_spec_family_resolution_is_case_insensitive() {
    assert_eq!(
        ModelSpec::resolve(&["gemini-2.5-pro"], None, &[]).usable(),
        Some(1_000_000)
    );
    assert_eq!(
        ModelSpec::resolve(&["Gemini-Flash"], None, &[]).usable(),
        Some(1_000_000)
    );
    assert_eq!(
        ModelSpec::resolve(&["claude-3-7-sonnet"], None, &[]).usable(),
        Some(200_000)
    );
    assert_eq!(
        ModelSpec::resolve(&["Claude-Opus"], None, &[]).usable(),
        Some(200_000)
    );
    assert_eq!(ModelSpec::resolve(&["gpt-4o"], None, &[]).usable(), None);
    assert_eq!(
        ModelSpec::resolve(&["custom-local-model"], None, &[]).usable(),
        None
    );
}

#[test]
fn test_usage_modal_sections_and_offline_fallback() {
    let modal = UsageModal::offline_fallback();
    assert!(!modal.allow_network);

    let state = modal.state.lock().unwrap().clone();
    match state {
        UsageState::Ready(summary) => {
            assert!(summary.plan.tier_name.contains("Google AI Pro"));
            assert_eq!(summary.groups.len(), 3);

            assert_eq!(summary.groups[0].name, "GEMINI MODELS");
            assert_eq!(summary.groups[0].buckets.len(), 2);
            assert_eq!(summary.groups[0].buckets[0].name, "Weekly Limit Remaining");
            assert_eq!(
                summary.groups[0].buckets[1].name,
                "Five Hour Limit Remaining"
            );

            assert_eq!(summary.groups[1].name, "CLAUDE AND GPT MODELS");
            assert_eq!(summary.groups[1].buckets.len(), 2);
            assert_eq!(summary.groups[1].buckets[0].name, "Weekly Limit Remaining");
            assert_eq!(
                summary.groups[1].buckets[1].name,
                "Five Hour Limit Remaining"
            );

            assert_eq!(summary.groups[2].name, "GEMINI 4 ARGON");
            assert_eq!(summary.groups[2].buckets.len(), 1);
            assert_eq!(
                summary.groups[2].buckets[0].name,
                "Gemini 4 Argon Weekly Limit"
            );
        }
        _ => panic!("Expected UsageState::Ready in offline fallback"),
    }
}

#[test]
fn test_usage_modal_key_navigation_and_closing() {
    let mut modal = UsageModal::offline_fallback();
    modal.max_scroll.set(10);

    let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(down_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 1);

    let j_key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(modal.handle_key(j_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 2);

    let page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(page_down), ModalAction::None);
    assert_eq!(modal.scroll_offset, 7);

    let end_key = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(end_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 10);

    let up_key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(up_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 9);

    let k_key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(modal.handle_key(k_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 8);

    let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(page_up), ModalAction::None);
    assert_eq!(modal.scroll_offset, 3);

    let home_key = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(home_key), ModalAction::None);
    assert_eq!(modal.scroll_offset, 0);

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(esc_key), ModalAction::Close);

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(modal.handle_key(enter_key), ModalAction::Close);

    let q_key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(modal.handle_key(q_key), ModalAction::Close);
}

#[test]
fn test_usage_modal_mouse_scroll() {
    let mut modal = UsageModal::offline_fallback();
    modal.max_scroll.set(10);

    let mouse_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(modal.handle_mouse(mouse_down), ModalAction::None);
    assert_eq!(modal.scroll_offset, 2);

    let mouse_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(modal.handle_mouse(mouse_up), ModalAction::None);
    assert_eq!(modal.scroll_offset, 0);

    let mouse_click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(modal.handle_mouse(mouse_click), ModalAction::None);
}

#[test]
fn test_usage_modal_reset_time_formatting() {
    let malformed = format_reset_time("not-a-valid-date");
    assert_eq!(malformed, "Resets periodically");

    let past = format_reset_time("1970-01-01T00:00:00Z");
    assert_eq!(past, "Quota available");

    let future = format_reset_time("2099-12-31T23:59:59Z");
    assert!(future.starts_with("Refreshes in "));
}

#[test]
fn test_usage_modal_exhausted_bucket_and_rendering() {
    let summary = UsageSummary {
        plan: UsagePlanInfo {
            account_email: None,
            tier_name: "Ultra Pro Max".to_string(),
            upgrade_url: None,
            manage_url: Some("https://example.com/manage".to_string()),
        },
        groups: vec![QuotaGroup {
            name: "Test Group".to_string(),
            description: Some("Window: 5h".to_string()),
            buckets: vec![
                QuotaBucket {
                    name: "Exhausted Model".to_string(),
                    remaining_pct: 0.0,
                    reset_label: "Resets in 1h 20m".to_string(),
                    exhausted: true,
                },
                QuotaBucket {
                    name: "Warning Model".to_string(),
                    remaining_pct: 15.0,
                    reset_label: "Resets in 2h 00m".to_string(),
                    exhausted: false,
                },
                QuotaBucket {
                    name: "Healthy Model".to_string(),
                    remaining_pct: 90.0,
                    reset_label: "Resets in 4h 30m".to_string(),
                    exhausted: false,
                },
            ],
        }],
    };

    let (bar_ex, col_ex) = render_progress_bar(0.0, 10, true);
    assert_eq!(col_ex, Color::Red);
    assert_eq!(bar_ex, "░░░░░░░░░░");

    let (_bar_warn, col_warn) = render_progress_bar(15.0, 10, false);
    assert_eq!(col_warn, Color::Yellow);

    let (bar_ok, col_ok) = render_progress_bar(90.0, 10, false);
    assert_eq!(col_ok, Color::Green);
    assert!(bar_ok.contains('█'));

    let modal = UsageModal::with_data(summary);
    let backend = TestBackend::new(80, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 80, 25);
            modal.render(f, area);
        })
        .unwrap();
}

#[test]
fn test_slash_quota_command_and_palette_dispatch() {
    let (mut app, _) = test_app();
    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);

    app.editor.insert_str("/quota");
    let act = app.handle_terminal_event(enter);
    assert_eq!(act, None);
    assert_eq!(app.modals.stack.len(), 1);
    assert!(matches!(app.modals.top(), Some(Modal::Usage(_))));

    let esc = press_key(KeyCode::Esc, KeyModifiers::NONE);
    let act_close = app.handle_terminal_event(esc);
    assert_eq!(act_close, None);
    assert!(app.modals.is_empty());
}

#[test]
fn test_accumulated_usage_preserved_across_multisession_tab_swap() {
    let (mut app, _) = test_app();

    app.accumulated_usage = TokenUsage {
        input_tokens: 1500,
        output_tokens: 800,
        thinking_tokens: 200,
        cache_read_tokens: 500,
        total_tokens: 3000,
    };
    app.last_turn_usage = Some(TokenUsage {
        input_tokens: 500,
        output_tokens: 200,
        thinking_tokens: 50,
        cache_read_tokens: 100,
        total_tokens: 850,
    });

    let tab2_id = SessionId("tab-2-sess".to_string());
    app.add_tab(tab2_id, None);
    assert_eq!(app.active_tab_index, 1);
    assert_eq!(app.accumulated_usage, TokenUsage::default());
    assert_eq!(app.last_turn_usage, None);

    app.switch_tab(0);
    assert_eq!(app.active_tab_index, 0);
    assert_eq!(app.accumulated_usage.total_tokens, 3000);
    assert_eq!(app.last_turn_usage.unwrap().total_tokens, 850);

    let enter = press_key(KeyCode::Enter, KeyModifiers::NONE);
    app.editor.insert_str("/clear");
    app.handle_terminal_event(enter);
    assert_eq!(app.accumulated_usage, TokenUsage::default());
    assert_eq!(app.last_turn_usage, None);
}

#[test]
fn test_parse_available_models_maps_shared_pools_and_canonical_names() {
    let json = serde_json::json!({
        "models": {
            "gemini-3-flash": {
                "displayName": "Gemini 3 Flash",
                "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": {
                    "remainingFraction": 0.725,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            },
            "gemini-2.5-pro": {
                "displayName": "Gemini 2.5 Pro",
                "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": {
                    "remainingFraction": 0.725,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            },
            "claude-sonnet-5-5-medium": {
                "displayName": "Claude Sonnet 5.5 (Medium)",
                "modelProvider": "MODEL_PROVIDER_ANTHROPIC",
                "quotaInfo": {
                    "remainingFraction": 1.0,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            },
            "claude-opus-5-5-medium": {
                "displayName": "Claude Opus 5.5 (Medium)",
                "modelProvider": "MODEL_PROVIDER_ANTHROPIC",
                "quotaInfo": {
                    "remainingFraction": 1.0,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            },
            "gpt-oss-120b-medium": {
                "displayName": "GPT-OSS 120B (Medium)",
                "modelProvider": "MODEL_PROVIDER_OPENAI",
                "quotaInfo": {
                    "remainingFraction": 0.5,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            }
        }
    });

    let groups = parse_available_models(&json).expect("should parse models");
    assert_eq!(groups.len(), 3);

    assert_eq!(groups[0].name, "GEMINI MODELS");
    assert_eq!(
        groups[0].description,
        Some("Models within this group: Gemini Flash, Gemini Pro".to_string())
    );
    assert_eq!(groups[0].buckets[0].name, "Weekly Limit Remaining");
    assert_eq!(groups[0].buckets[1].name, "Five Hour Limit Remaining");
    assert_eq!(groups[0].buckets[1].remaining_pct, 72.5);
    assert!(groups[0].buckets[1]
        .reset_label
        .starts_with("Refreshes in "));
    assert!(!groups[0].buckets[1].exhausted);

    assert_eq!(groups[1].name, "CLAUDE AND GPT MODELS");
    assert_eq!(
        groups[1].description,
        Some("Models within this group: Claude Opus, Claude Sonnet, GPT-OSS".to_string())
    );
    assert_eq!(groups[1].buckets[0].name, "Weekly Limit Remaining");
    assert_eq!(groups[1].buckets[0].remaining_pct, 100.0);
    assert_eq!(groups[1].buckets[1].name, "Five Hour Limit Remaining");
    assert_eq!(groups[1].buckets[1].remaining_pct, 50.0);

    assert_eq!(groups[2].name, "GEMINI 4 ARGON");
    assert_eq!(groups[2].buckets[0].name, "Gemini 4 Argon Weekly Limit");
}

#[test]
fn test_parse_available_models_handles_exhausted_and_missing_fields() {
    let json = serde_json::json!({
        "models": {
            "gemini-flash": {
                "displayName": "Flash",
                "modelProvider": "GOOGLE",
                "quotaInfo": {
                    "remainingFraction": 0.0,
                    "resetTime": "2099-12-31T23:59:59Z"
                }
            },
            "claude-sonnet": {
                "disabled": true,
                "modelProvider": "ANTHROPIC"
            }
        }
    });

    let groups = parse_available_models(&json).expect("should parse");
    assert_eq!(groups[0].buckets[1].name, "Five Hour Limit Remaining");
    assert_eq!(groups[0].buckets[1].remaining_pct, 0.0);
    assert!(groups[0].buckets[1].exhausted);

    assert_eq!(groups[1].buckets[0].name, "Weekly Limit Remaining");
    assert_eq!(groups[1].buckets[0].remaining_pct, 0.0);
    assert!(groups[1].buckets[0].exhausted);
}

#[test]
fn test_build_usage_summary_rejects_empty_models_without_mock_fallback() {
    let empty_models = serde_json::json!({"models": {}});
    let plan = serde_json::json!({
        "currentTier": {
            "name": "Antigravity Pro",
            "upgradeSubscriptionUri": "https://example.com/upgrade"
        }
    });

    assert!(build_usage_summary(Some(&plan), None, &empty_models, None).is_none());

    let plan_info = parse_plan_info(Some(&plan), Some("test@example.com".to_string()));
    assert_eq!(plan_info.tier_name, "Antigravity Pro");
    assert_eq!(
        plan_info.account_email,
        Some("test@example.com".to_string())
    );

    let default_plan = parse_plan_info(None, None);
    assert_eq!(default_plan.tier_name, "Google AI Pro");
    assert_eq!(default_plan.account_email, None);

    let cli_json = serde_json::json!({
        "command": {"name": "usage", "data": {"groups": [
            {
                "name": "Gemini Models",
                "buckets": [
                    {"name": "Weekly Limit Remaining", "remaining_fraction": 0.45, "reset_time": "2099-12-31T23:59:59Z"},
                    {"name": "Five Hour Limit Remaining", "remaining_fraction": 0.75, "reset_time": "2099-12-31T23:59:59Z"}
                ]
            },
            {
                "name": "Claude and GPT models",
                "buckets": [
                    {"name": "Weekly Limit Remaining", "remaining_fraction": 0.95, "reset_time": "2099-12-31T23:59:59Z"},
                    {"name": "Five Hour Limit Remaining", "remaining_fraction": 1.0, "reset_time": "2099-12-31T23:59:59Z"}
                ]
            },
            {
                "name": "Gemini 4 Argon",
                "buckets": [
                    {"name": "Gemini 4 Argon Weekly Limit", "remaining_fraction": 0.50, "reset_time": "2099-12-31T23:59:59Z"}
                ]
            }
        ]}}
    });

    let cli_groups = parse_quota_summary_groups(&cli_json);
    assert_eq!(cli_groups.len(), 3);
    assert_eq!(cli_groups[0].name, "GEMINI MODELS");
    assert_eq!(cli_groups[0].buckets[0].remaining_pct, 45.0);
    assert_eq!(cli_groups[0].buckets[1].remaining_pct, 75.0);
    assert_eq!(cli_groups[1].name, "CLAUDE AND GPT MODELS");
    assert_eq!(cli_groups[1].buckets[0].remaining_pct, 95.0);
    assert_eq!(cli_groups[1].buckets[1].remaining_pct, 100.0);
    assert_eq!(cli_groups[2].name, "GEMINI 4 ARGON");
    assert_eq!(cli_groups[2].buckets[0].remaining_pct, 50.0);
}
