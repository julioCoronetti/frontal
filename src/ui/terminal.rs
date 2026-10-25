use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, stdout, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crate::engine::kill_registered_subprocesses;

static PANIC_HOOK_SET: Once = Once::new();

pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    restored: AtomicBool,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self, io::Error> {
        install_panic_hook();
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            restored: AtomicBool::new(false),
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    pub fn restore(&self) {
        if !self.restored.swap(true, Ordering::SeqCst) {
            let mut out = stdout();
            let _ = execute!(out, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

pub fn restore_terminal_raw() {
    let mut out = stdout();
    let _ = execute!(out, DisableBracketedPaste, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

pub fn install_panic_hook() {
    PANIC_HOOK_SET.call_once(|| {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            restore_terminal_raw();
            kill_registered_subprocesses();
            prev_hook(panic_info);
        }));
    });
}
