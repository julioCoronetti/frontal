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

pub fn sensory_notification_bytes(title: &str, message: &str) -> Vec<u8> {
    let osc9 = osc9_notification(message);
    let osc777 = osc777_notification(title, message);
    let mut buf = Vec::with_capacity(1 + osc9.len() + osc777.len());
    buf.extend_from_slice(terminal_bell());
    buf.extend_from_slice(&osc9);
    buf.extend_from_slice(&osc777);
    buf
}

pub fn emit_sensory_notification(title: &str, message: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(&sensory_notification_bytes(title, message));
    let _ = out.flush();
}
