use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::{EngineEvent, StreamEvent};
use crate::session::SessionManager;
use crate::transport::{EngineConfig, ReplayMode};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub headless: bool,
    pub prompts: Vec<String>,
    pub replay_file: Option<PathBuf>,
    pub agy_bin: Option<PathBuf>,
    pub opencode_bin: Option<PathBuf>,
    pub conversation_id: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub effort: Option<String>,
    pub cwd: Option<PathBuf>,
    pub dangerously_skip_permissions: bool,
    pub show_help: bool,
    pub show_version: bool,
}

impl CliArgs {
    pub fn parse_from(args: impl Iterator<Item = String>) -> Self {
        let mut cli = Self {
            headless: false,
            prompts: Vec::new(),
            replay_file: None,
            agy_bin: None,
            opencode_bin: None,
            conversation_id: None,
            project: None,
            model: None,
            mode: None,
            effort: None,
            cwd: None,
            dangerously_skip_permissions: true,
            show_help: false,
            show_version: false,
        };

        let mut iter = args.skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--headless" => cli.headless = true,
                "--prompt" | "-p" => {
                    if let Some(val) = iter.next() {
                        cli.prompts.push(val);
                    }
                }
                "--replay-file" => {
                    if let Some(val) = iter.next() {
                        cli.replay_file = Some(PathBuf::from(val));
                    }
                }
                "--agy-bin" => {
                    if let Some(val) = iter.next() {
                        cli.agy_bin = Some(PathBuf::from(val));
                    }
                }
                "--opencode-bin" => {
                    if let Some(val) = iter.next() {
                        cli.opencode_bin = Some(PathBuf::from(val));
                    }
                }
                "--conversation" | "-c" => {
                    if let Some(val) = iter.next() {
                        cli.conversation_id = Some(val);
                    }
                }
                "--project" => {
                    if let Some(val) = iter.next() {
                        cli.project = Some(val);
                    }
                }
                "--model" => {
                    if let Some(val) = iter.next() {
                        cli.model = Some(val);
                    }
                }
                "--mode" => {
                    if let Some(val) = iter.next() {
                        cli.mode = Some(val);
                    }
                }
                "--effort" => {
                    if let Some(val) = iter.next() {
                        cli.effort = Some(val);
                    }
                }
                "--cwd" => {
                    if let Some(val) = iter.next() {
                        cli.cwd = Some(PathBuf::from(val));
                    }
                }
                "--dangerously-skip-permissions" => {
                    cli.dangerously_skip_permissions = true;
                }
                "--help" | "-h" => {
                    cli.show_help = true;
                }
                "--version" | "-V" => {
                    cli.show_version = true;
                }
                _ => {}
            }
        }
        cli
    }

    pub fn print_help() {
        println!("Frontal - Terminal User Interface & Engine Harness");
        println!();
        println!("Usage: frontal [OPTIONS]");
        println!();
        println!("Options:");
        println!("  --headless                     Run in headless mode without TUI");
        println!("  --prompt, -p <PROMPT>          Initial prompt to execute (repeatable)");
        println!(
            "  --replay-file <PATH>           Path to fixture .ndjson for deterministic testing"
        );
        println!("  --agy-bin <PATH>               Custom path to agy binary");
        println!("  --opencode-bin <PATH>          Custom path to opencode binary");
        println!("  --conversation, -c <UUID>      Resume conversation ID");
        println!("  --project <ID>                 Project ID for the session");
        println!("  --model <MODEL>                Model ID (e.g. gemini-3.8-flash-high)");
        println!("  --mode <MODE>                  Execution mode (auto, accept-edits, plan)");
        println!(
            "  --effort <EFFORT>              Reasoning effort (low, medium, high, xhigh, max)"
        );
        println!("  --cwd <PATH>                   Working directory");
        println!("  --help, -h                     Show this help message");
        println!("  --version, -V                  Show version information");
    }

    pub fn apply_to_config(&self, config: &mut EngineConfig) {
        if let Some(bin) = &self.agy_bin {
            config.agy_bin = bin.clone();
        }
        if let Some(bin) = &self.opencode_bin {
            config.opencode_bin = bin.clone();
        }
        if self.conversation_id.is_some() {
            config.conversation_id = self.conversation_id.clone();
        }
        if self.project.is_some() {
            config.project = self.project.clone();
        }
        if self.model.is_some() {
            config.model = self.model.clone();
        }
        if self.mode.is_some() {
            config.mode = self.mode.clone();
        }
        if self.effort.is_some() {
            config.effort = self.effort.clone();
        }
        if self.cwd.is_some() {
            config.cwd = self.cwd.clone();
        }
        if self.dangerously_skip_permissions {
            config.dangerously_skip_permissions = true;
        }
    }
}

pub async fn run_from_env() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse_from(env::args());
    if args.show_version {
        println!("frontal {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.show_help {
        CliArgs::print_help();
        return Ok(());
    }

    let mut session_manager = SessionManager::new();

    let session_id = if let Some(replay_path) = &args.replay_file {
        session_manager
            .create_replay_session(replay_path, ReplayMode::TurnGated)
            .await?
    } else {
        let mut config = EngineConfig::new();
        args.apply_to_config(&mut config);
        session_manager.create_subprocess_session(config).await?
    };

    if !args.headless {
        let mut app = crate::ui::app::App::new(session_manager, session_id.clone());
        for p in args.prompts {
            let _ = app.session_manager.send_prompt(&session_id, &p).await;
            app.cache.push_user_message(&p);
        }
        return crate::ui::run_tui(app).await;
    }

    if args.prompts.is_empty() {
        println!("Session initialized: {}", session_id);
        if let Some(envelope) = session_manager.recv().await {
            if let EngineEvent::Stream(ev) = envelope.event {
                if let StreamEvent::Init(init_ev) = *ev {
                    if let Some(conv_id) = init_ev.conversation_id {
                        println!("Conversation ID: {}", conv_id);
                    }
                }
            }
        }
        session_manager
            .shutdown_session(&session_id, Duration::from_millis(1000))
            .await?;
        return Ok(());
    }

    for prompt in args.prompts {
        session_manager.send_prompt(&session_id, &prompt).await?;

        loop {
            tokio::select! {
                maybe_envelope = session_manager.recv() => {
                    match maybe_envelope {
                        Some(envelope) => {
                            match envelope.event {
                                EngineEvent::Stream(ev) => match *ev {
                                    StreamEvent::StepUpdate(update) => {
                                        if let Some(data) = update.step_update {
                                            if let Some(delta) = data.text_delta {
                                                print!("{}", delta);
                                                let _ = io::stdout().flush();
                                            }
                                        }
                                    }
                                    StreamEvent::Result(res) => {
                                        if let Some(data) = res.result {
                                            if let Some(resp) = data.response {
                                                if !resp.ends_with('\n') {
                                                    println!();
                                                }
                                            }
                                        }
                                        break;
                                    }
                                    StreamEvent::Error(err) => {
                                        if let Some(e) = err.error.or(err.message) {
                                            eprintln!("Error: {}", e);
                                        }
                                        break;
                                    }
                                    _ => {}
                                },
                                EngineEvent::ProtocolError { message, .. } => {
                                    eprintln!("Protocol error: {}", message);
                                    break;
                                }
                                EngineEvent::ProcessExited { code, .. } => {
                                    eprintln!("Process exited with code: {:?}", code);
                                    break;
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("Interrupted by user");
                    let _ = session_manager.interrupt_active();
                    break;
                }
            }
        }
    }

    session_manager
        .shutdown_session(&session_id, Duration::from_millis(1000))
        .await?;

    Ok(())
}
