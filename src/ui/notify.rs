use std::io::{self, Write};

pub fn terminal_bell() -> &'static [u8] {
    b"\x07"
}

pub fn osc9_notification(message: &str) -> Vec<u8> {
    let sanitized = message.replace(['\x07', '\x1b'], "");
    format!("\x1b]9;{}\x07", sanitized).into_bytes()
}

pub fn osc777_notification(title: &str, message: &str) -> Vec<u8> {
    let san_title = title.replace(['\x07', '\x1b'], "");
    let san_msg = message.replace(['\x07', '\x1b'], "");
    format!("\x1b]777;notify;{};{}\x07", san_title, san_msg).into_bytes()
}

pub fn emit_sensory_notification(title: &str, message: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(terminal_bell());
    let _ = out.write_all(&osc9_notification(message));
    let _ = out.write_all(&osc777_notification(title, message));
    let _ = out.flush();
}
