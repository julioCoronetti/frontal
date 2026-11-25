use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use arboard::Clipboard;

use super::image_codec::{decode_base64, decode_bmp_to_rgba, MAX_RAW_RGBA_BYTES, PNG_SIGNATURE};
use super::{save_encoded_media_dedup, save_rgba_png_dedup, SavedMediaImage};
use crate::core::media::{extract_pasted_image_path, image_mime_for_path};

fn run_command_bounded(program: &str, args: &[&str], max_bytes: usize) -> Option<Vec<u8>> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut child, mut guard) = crate::engine::spawn_std_isolated(&mut cmd).ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.take(max_bytes as u64 + 1).read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                guard.signal(libc::SIGTERM);
                guard.disarm();
                break status;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    guard.kill_sigkill_and_disarm();
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                guard.kill_sigkill_and_disarm();
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    };
    let buf = reader.join().ok()?;
    if !status.success() || buf.is_empty() || buf.len() > max_bytes {
        return None;
    }
    Some(buf)
}

fn strip_prefix_ascii_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

pub fn windows_path_to_wsl(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"');
    if let Some(rest) = strip_prefix_ascii_ci(trimmed, "\\\\wsl$\\")
        .or_else(|| strip_prefix_ascii_ci(trimmed, "\\\\wsl.localhost\\"))
    {
        if let Some((_, inner)) = rest.split_once('\\') {
            return PathBuf::from(format!("/{}", inner.replace('\\', "/")));
        }
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let tail = trimmed[2..].replace('\\', "/");
        return PathBuf::from(format!("/mnt/{}{}", drive, tail));
    }
    PathBuf::from(trimmed)
}

fn find_offered_type<'a>(types: &'a [String], candidates: &[&str]) -> Option<&'a str> {
    for candidate in candidates {
        if let Some(found) = types
            .iter()
            .find(|t| t.trim().eq_ignore_ascii_case(candidate))
        {
            return Some(found.as_str());
        }
    }
    None
}

fn first_uri_image_from_list(raw: &[u8]) -> Option<SavedMediaImage> {
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((path, mime)) = extract_pasted_image_path(trimmed) {
            return Some(SavedMediaImage {
                path,
                mime_type: mime,
            });
        }
    }
    None
}

fn read_offered_media(
    cache_dir: &Path,
    program: &str,
    list_args: &[&str],
    read_prefix: &[&str],
) -> Result<Option<SavedMediaImage>, String> {
    let Some(types_raw) = run_command_bounded(program, list_args, 64 * 1024) else {
        return Ok(None);
    };
    let types: Vec<String> = String::from_utf8_lossy(&types_raw)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if let Some(uri_type) = find_offered_type(&types, &["text/uri-list"]) {
        let mut args = read_prefix.to_vec();
        args.push(uri_type);
        if let Some(bytes) = run_command_bounded(program, &args, 64 * 1024) {
            if let Some(saved) = first_uri_image_from_list(&bytes) {
                return Ok(Some(saved));
            }
        }
    }

    let image_candidates: &[(&str, &str, &str)] = &[
        ("image/png", "png", "image/png"),
        ("image/bmp", "bmp", "image/bmp"),
        ("image/x-bmp", "bmp", "image/bmp"),
        ("image/x-MS-bmp", "bmp", "image/bmp"),
        ("image/jpeg", "jpg", "image/jpeg"),
        ("image/webp", "webp", "image/webp"),
    ];

    for &(mime_target, ext, canonical_mime) in image_candidates {
        let Some(offered) = find_offered_type(&types, &[mime_target]) else {
            continue;
        };
        let mut args = read_prefix.to_vec();
        args.push(offered);
        let Some(bytes) = run_command_bounded(program, &args, MAX_RAW_RGBA_BYTES) else {
            continue;
        };
        if canonical_mime == "image/bmp" {
            let Ok((w, h, rgba)) = decode_bmp_to_rgba(&bytes) else {
                continue;
            };
            return save_rgba_png_dedup(cache_dir, w, h, &rgba).map(Some);
        }
        if canonical_mime == "image/png" && !bytes.starts_with(&PNG_SIGNATURE) {
            continue;
        }
        return save_encoded_media_dedup(cache_dir, &bytes, ext, canonical_mime).map(Some);
    }

    Ok(None)
}

fn read_arboard_media(cache_dir: &Path) -> Result<Option<SavedMediaImage>, String> {
    let Ok(mut cb) = Clipboard::new() else {
        return Ok(None);
    };
    let Ok(img) = cb.get_image() else {
        return Ok(None);
    };
    let w = u32::try_from(img.width).map_err(|_| "clipboard image width overflow".to_string())?;
    let h = u32::try_from(img.height).map_err(|_| "clipboard image height overflow".to_string())?;
    save_rgba_png_dedup(cache_dir, w, h, img.bytes.as_ref()).map(Some)
}

fn read_wl_paste_media(cache_dir: &Path) -> Result<Option<SavedMediaImage>, String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(None);
    }
    read_offered_media(
        cache_dir,
        "wl-paste",
        &["--list-types"],
        &["--no-newline", "--type"],
    )
}

fn read_xclip_media(cache_dir: &Path) -> Result<Option<SavedMediaImage>, String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Ok(None);
    }
    read_offered_media(
        cache_dir,
        "xclip",
        &["-selection", "clipboard", "-t", "TARGETS", "-o"],
        &["-selection", "clipboard", "-o", "-t"],
    )
}

fn is_wsl_environment() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

fn read_wsl_powershell_media(cache_dir: &Path) -> Result<Option<SavedMediaImage>, String> {
    if !is_wsl_environment() {
        return Ok(None);
    }
    let script = concat!(
        "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; ",
        "if ([System.Windows.Forms.Clipboard]::ContainsFileDropList()) { ",
        "foreach ($f in [System.Windows.Forms.Clipboard]::GetFileDropList()) { Write-Output ('FILE:' + $f) } ",
        "} elseif ([System.Windows.Forms.Clipboard]::ContainsImage()) { ",
        "$img = [System.Windows.Forms.Clipboard]::GetImage(); ",
        "if ($img -ne $null) { ",
        "$ms = New-Object System.IO.MemoryStream; ",
        "$img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); ",
        "Write-Output ('PNG:' + [Convert]::ToBase64String($ms.ToArray())) ",
        "} }"
    );
    let Some(stdout) = run_command_bounded(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Sta", "-Command", script],
        MAX_RAW_RGBA_BYTES * 2,
    ) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&stdout);
    let trimmed = text.trim();
    if trimmed.starts_with("FILE:") {
        for line in trimmed.lines() {
            if let Some(raw_path) = line.trim().strip_prefix("FILE:") {
                let wsl_path = windows_path_to_wsl(raw_path);
                if let Some(mime) = image_mime_for_path(&wsl_path) {
                    if wsl_path.is_file() {
                        return Ok(Some(SavedMediaImage {
                            path: wsl_path,
                            mime_type: mime,
                        }));
                    }
                }
            }
        }
        return Ok(None);
    }
    if let Some(b64) = trimmed.strip_prefix("PNG:") {
        let png_bytes = decode_base64(b64)?;
        if !png_bytes.starts_with(&PNG_SIGNATURE) {
            return Err("invalid PNG payload from Windows clipboard".to_string());
        }
        return save_encoded_media_dedup(cache_dir, &png_bytes, "png", "image/png").map(Some);
    }
    Ok(None)
}

pub fn read_system_clipboard_image(cache_dir: &Path) -> Result<Option<SavedMediaImage>, String> {
    let mut last_err = None;
    for reader in [
        read_arboard_media,
        read_wl_paste_media,
        read_xclip_media,
        read_wsl_powershell_media,
    ] {
        match reader(cache_dir) {
            Ok(Some(saved)) => return Ok(Some(saved)),
            Ok(None) => {}
            Err(err) => last_err = Some(err),
        }
    }
    if let Some(err) = last_err {
        Err(err)
    } else {
        Ok(None)
    }
}

pub fn read_system_clipboard_text() -> Option<String> {
    if let Ok(mut cb) = Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        for args in [
            ["--no-newline", "--type", "text/plain;charset=utf-8"].as_slice(),
            ["--no-newline"].as_slice(),
        ] {
            if let Some(raw) = run_command_bounded("wl-paste", args, 8 * 1024 * 1024) {
                let s = String::from_utf8_lossy(&raw).into_owned();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        if let Some(raw) =
            run_command_bounded("xclip", &["-selection", "clipboard", "-o"], 8 * 1024 * 1024)
        {
            let s = String::from_utf8_lossy(&raw).into_owned();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    if is_wsl_environment() {
        if let Some(raw) = run_command_bounded(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Sta",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::Write([System.Windows.Forms.Clipboard]::GetText())",
            ],
            8 * 1024 * 1024,
        ) {
            let s = String::from_utf8_lossy(&raw).into_owned();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}
