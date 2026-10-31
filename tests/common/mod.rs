use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use frontal::protocol::SessionId;
use frontal::session::SessionManager;
use frontal::ui::app::App;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

pub static PGID_TEST_LOCK: Mutex<()> = Mutex::const_new(());
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub fn press_key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

pub fn test_app() -> (App, SessionId) {
    let mut session_manager = SessionManager::new();
    let session_id = SessionId::new();
    session_manager.register_mock_session(session_id.clone());
    let app = App::new_isolated(session_manager, session_id.clone());
    (app, session_id)
}

#[allow(dead_code)]
pub fn render_app(app: &mut App, width: u16, height: u16) -> (Terminal<TestBackend>, String) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<Vec<_>>()
        .join("");
    (terminal, text)
}

pub struct TempDirGuard(pub PathBuf);

impl TempDirGuard {
    pub fn new(prefix: &str) -> Self {
        let seq = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let unique = format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            seq
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct TempFileGuard(pub PathBuf);

impl TempFileGuard {
    pub fn new(prefix: &str, ext: &str, content: &str) -> Self {
        let seq = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let unique = format!(
            "{}_{}_{}_{}.{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            seq,
            ext
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, content).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
