use frontal::infra::clipboard::{
    adler32, crc32_ieee, crc32_update, decode_base64, decode_bmp_to_rgba, encode_rgba_png,
    save_encoded_media_dedup, windows_path_to_wsl, SystemClipboard,
};
use frontal::ui::editor::{extract_pasted_image_path, ExtmarkKind, PromptEditor};

use crate::common::TempDirGuard;

#[test]
fn test_encode_rgba_png_signature_chunks_and_checksums() {
    let rgba = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
    ];
    let png = encode_rgba_png(2, 2, &rgba).expect("valid 2x2 RGBA should encode");

    assert_eq!(&png[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

    let ihdr_len = u32::from_be_bytes(png[8..12].try_into().unwrap());
    assert_eq!(ihdr_len, 13);
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 2);
    assert_eq!(png[24], 8);
    assert_eq!(png[25], 6);
    let ihdr_crc = u32::from_be_bytes(png[29..33].try_into().unwrap());
    assert_eq!(ihdr_crc, crc32_ieee(&png[12..29]));

    let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    assert_eq!(&png[37..41], b"IDAT");
    let zlib = &png[41..41 + idat_len];
    assert_eq!(&zlib[0..2], &[0x78, 0x01]);
    assert_eq!(zlib[2], 0x01);

    let expected_filtered = vec![
        0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 128,
    ];
    let block_len = u16::from_le_bytes(zlib[3..5].try_into().unwrap()) as usize;
    assert_eq!(block_len, expected_filtered.len());
    assert_eq!(&zlib[7..7 + block_len], expected_filtered.as_slice());
    let trailer = u32::from_be_bytes(zlib[7 + block_len..11 + block_len].try_into().unwrap());
    assert_eq!(trailer, adler32(&expected_filtered));

    let idat_crc_off = 41 + idat_len;
    let idat_crc = u32::from_be_bytes(png[idat_crc_off..idat_crc_off + 4].try_into().unwrap());
    assert_eq!(idat_crc, crc32_ieee(&png[37..41 + idat_len]));

    let iend_off = idat_crc_off + 4;
    assert_eq!(
        &png[iend_off..iend_off + 8],
        &[0, 0, 0, 0, b'I', b'E', b'N', b'D']
    );
}

#[test]
fn test_encode_rgba_png_multiblock_and_validation_guards() {
    assert!(encode_rgba_png(0, 10, &[]).is_err());
    assert!(encode_rgba_png(10, 0, &[]).is_err());
    assert!(encode_rgba_png(2, 2, &[0u8; 15]).is_err());
    assert!(encode_rgba_png(4096, 4096, &[]).is_err());

    let large_rgba = vec![42u8; 200 * 200 * 4];
    let large_png = encode_rgba_png(200, 200, &large_rgba).expect("200x200 should chunk deflate");
    let idat_len = u32::from_be_bytes(large_png[33..37].try_into().unwrap()) as usize;
    let zlib = &large_png[41..41 + idat_len];
    assert_eq!(zlib[2], 0x00);
    let first_block_len = u16::from_le_bytes(zlib[3..5].try_into().unwrap());
    assert_eq!(first_block_len, u16::MAX);
}

#[test]
fn test_isolated_clipboard_get_image_persists_png_without_collision() {
    let temp = TempDirGuard::new("clipboard_media_slice1");
    let mut cb = SystemClipboard::new_isolated_with_dir(temp.path().to_path_buf());
    assert!(cb.get_image().unwrap().is_none());

    let rgba1 = vec![
        10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
    ];
    cb.set_isolated_image(2, 2, rgba1);

    let first = cb.get_image().unwrap().expect("first image saved");
    let dedup = cb
        .get_image()
        .unwrap()
        .expect("identical image deduplicated");
    assert_eq!(first.path, dedup.path);

    let rgba2 = vec![
        99, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
    ];
    cb.set_isolated_image(2, 2, rgba2);
    let second = cb.get_image().unwrap().expect("distinct image saved");

    assert_eq!(first.mime_type, "image/png");
    assert_ne!(first.path, second.path);
    assert!(first.path.starts_with(temp.path()));
    assert!(first.path.is_file());
    assert!(second.path.is_file());

    let bytes = std::fs::read(&first.path).unwrap();
    assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

    cb.clear_isolated_image();
    assert!(cb.get_image().unwrap().is_none());
}

#[test]
fn test_editor_media_extmark_atomic_deletion_and_payload_submission() {
    let mut editor = PromptEditor::new();
    editor.insert_str("analyze ");
    editor.insert_media_attachment("image/png", "/tmp/first.png");
    editor.insert_str(" and ");
    editor.insert_media_attachment("image/jpeg", "/tmp/second.jpg");
    assert_eq!(editor.extmarks().len(), 2);
    assert!(editor.text().contains("[Image #1]"));
    assert!(editor.text().contains("[Image #2]"));

    editor.backspace();
    assert_eq!(editor.extmarks().len(), 1);
    assert!(!editor.text().contains("[Image #2]"));

    let payload = editor.take_submitted_payload().expect("payload expected");
    assert_eq!(payload.text, "analyze  and");
    assert_eq!(payload.media.len(), 1);
    assert_eq!(payload.media[0].mime_type, "image/png");
    assert_eq!(payload.media[0].uri, "/tmp/first.png");
    let chat_display = payload.display_for_chat();
    assert_eq!(chat_display, "analyze [Image 1] and\n\n[file] first.png");
    assert!(editor.is_empty());

    editor.insert_media_attachment("image/webp", "/tmp/only.webp");
    assert!(editor.text().contains("[Image #1]"));
    let media_only = editor
        .take_submitted_payload()
        .expect("image-only prompt allowed");
    assert_eq!(media_only.text, "");
    assert_eq!(media_only.media.len(), 1);
    assert_eq!(
        media_only.display_for_chat(),
        "[Image 1]\n\n[file] only.webp"
    );
    assert!(editor.extmarks().is_empty());

    editor.insert_str("look ");
    editor.insert_media_attachment(
        "image/png",
        "/home/user/.frontal/cache/media/img_500x500_caa4ffb2.png",
    );
    editor.insert_str(" vs ");
    editor.insert_media_attachment("image/png", "file:///tmp/my%20shot.png");
    let expanded = editor.take_submitted_expanded().expect("expanded text");
    assert_eq!(
        expanded,
        "look [Image 1] vs [Image 2]\n\n[file] clipboard\n[file] my shot.png"
    );

    let id = editor.insert_media_attachment("image/png", "/tmp/undo_test.png");
    assert_eq!(editor.extmarks()[0].id, id);
    assert!(matches!(
        editor.extmarks()[0].kind,
        ExtmarkKind::Media { .. }
    ));
    editor.backspace();
    assert!(editor.extmarks().is_empty());
    assert!(editor.undo());
    assert_eq!(editor.extmarks().len(), 1);
}

#[test]
fn test_extract_pasted_image_path_formats_and_fallback() {
    let temp = TempDirGuard::new("paste_image_path");
    let spaced_png = temp.path().join("my shot.PNG");
    let webp_file = temp.path().join("preview.webp");
    let text_file = temp.path().join("notes.txt");
    std::fs::write(&spaced_png, b"fake").unwrap();
    std::fs::write(&webp_file, b"fake").unwrap();
    std::fs::write(&text_file, b"hello").unwrap();

    let file_uri = format!(
        "file://{}\r\n",
        spaced_png.to_string_lossy().replace(' ', "%20")
    );
    let (p1, m1) = extract_pasted_image_path(&file_uri).expect("file:// URI should match");
    assert_eq!(p1, spaced_png);
    assert_eq!(m1, "image/png");

    let quoted = format!("'{}'", webp_file.display());
    let (p2, m2) = extract_pasted_image_path(&quoted).expect("quoted path should match");
    assert_eq!(p2, webp_file);
    assert_eq!(m2, "image/webp");

    let escaped = spaced_png.to_string_lossy().replace(' ', "\\ ");
    let (p3, m3) =
        extract_pasted_image_path(&escaped).expect("backslash-escaped path should match");
    assert_eq!(p3, spaced_png);
    assert_eq!(m3, "image/png");

    assert!(extract_pasted_image_path(&text_file.to_string_lossy()).is_none());
    assert!(extract_pasted_image_path("/nonexistent/path/img.png").is_none());
    assert!(extract_pasted_image_path("line1\n/tmp/img.png").is_none());
}

#[test]
fn test_decode_bmp_to_rgba_32bpp_bitfields_and_24bpp_rgb() {
    let mut bmp32 = Vec::new();
    bmp32.extend_from_slice(b"BM");
    bmp32.extend_from_slice(&(66u32 + 16).to_le_bytes());
    bmp32.extend_from_slice(&0u32.to_le_bytes());
    bmp32.extend_from_slice(&66u32.to_le_bytes());
    bmp32.extend_from_slice(&40u32.to_le_bytes());
    bmp32.extend_from_slice(&2i32.to_le_bytes());
    bmp32.extend_from_slice(&2i32.to_le_bytes());
    bmp32.extend_from_slice(&1u16.to_le_bytes());
    bmp32.extend_from_slice(&32u16.to_le_bytes());
    bmp32.extend_from_slice(&3u32.to_le_bytes());
    bmp32.extend_from_slice(&16u32.to_le_bytes());
    bmp32.extend_from_slice(&[0u8; 16]);
    bmp32.extend_from_slice(&0x00FF_0000u32.to_le_bytes());
    bmp32.extend_from_slice(&0x0000_FF00u32.to_le_bytes());
    bmp32.extend_from_slice(&0x0000_00FFu32.to_le_bytes());
    bmp32.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]);
    bmp32.extend_from_slice(&[255, 0, 0, 0, 255, 255, 255, 0]);

    let (w, h, rgba) = decode_bmp_to_rgba(&bmp32).expect("32bpp BI_BITFIELDS should decode");
    assert_eq!((w, h), (2, 2));
    assert_eq!(
        rgba,
        vec![0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255]
    );

    let mut bmp24 = Vec::new();
    bmp24.extend_from_slice(b"BM");
    bmp24.extend_from_slice(&(54u32 + 8).to_le_bytes());
    bmp24.extend_from_slice(&0u32.to_le_bytes());
    bmp24.extend_from_slice(&54u32.to_le_bytes());
    bmp24.extend_from_slice(&40u32.to_le_bytes());
    bmp24.extend_from_slice(&1i32.to_le_bytes());
    bmp24.extend_from_slice(&(-2i32).to_le_bytes());
    bmp24.extend_from_slice(&1u16.to_le_bytes());
    bmp24.extend_from_slice(&24u16.to_le_bytes());
    bmp24.extend_from_slice(&0u32.to_le_bytes());
    bmp24.extend_from_slice(&8u32.to_le_bytes());
    bmp24.extend_from_slice(&[0u8; 16]);
    bmp24.extend_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);

    let (w24, h24, rgba24) = decode_bmp_to_rgba(&bmp24).expect("24bpp top-down should decode");
    assert_eq!((w24, h24), (1, 2));
    assert_eq!(rgba24, vec![30, 20, 10, 255, 60, 50, 40, 255]);
}

#[test]
fn test_decode_base64_encoded_dedup_and_wsl_path_translation() {
    let decoded = decode_base64("SGVs\nbG8h\r\n").unwrap();
    assert_eq!(decoded, b"Hello!");
    assert!(decode_base64("@@@").is_err());

    let temp = TempDirGuard::new("encoded_media_dedup");
    let first = save_encoded_media_dedup(temp.path(), b"png-data", "png", "image/png").unwrap();
    let second = save_encoded_media_dedup(temp.path(), b"png-data", "png", "image/png").unwrap();
    assert_eq!(first.path, second.path);
    assert_eq!(std::fs::read(&first.path).unwrap(), b"png-data");

    assert_eq!(
        windows_path_to_wsl(r#"C:\Users\Julio\Pictures\shot.png"#),
        std::path::PathBuf::from("/mnt/c/Users/Julio/Pictures/shot.png")
    );
    assert_eq!(
        windows_path_to_wsl(r#"\\wsl$\Ubuntu\tmp\img.png"#),
        std::path::PathBuf::from("/tmp/img.png")
    );
    assert_eq!(
        windows_path_to_wsl(r#"\\WSL.LOCALHOST\Ubuntu\tmp\img.png"#),
        std::path::PathBuf::from("/tmp/img.png")
    );
}

#[test]
fn test_crc32_update_and_background_clipboard_prepare() {
    let part_a = b"IHDR";
    let part_b = b"payload-1234567890";
    let mut joined = Vec::from(part_a.as_slice());
    joined.extend_from_slice(part_b);
    assert_eq!(
        crc32_update(crc32_update(0, part_a), part_b),
        crc32_ieee(&joined)
    );

    let mut isolated = SystemClipboard::new_isolated();
    let (seq1, writer1) = isolated.prepare_background_write();
    let (seq2, writer2) = isolated.prepare_background_write();
    assert!(seq2 > seq1);
    assert_eq!(writer2("latest".to_string()), None);
    assert_eq!(writer1("stale".to_string()), None);
    assert_eq!(isolated.get_text().as_deref(), Some("latest"));
}

#[test]
fn decode_base64_rejects_excess_padding() {
    assert!(decode_base64("====").is_err());
    assert!(decode_base64("A===").is_err());
    assert_eq!(decode_base64("Zg==").unwrap(), b"f");
}

#[test]
fn editor_extmark_overlap_move_down_and_sanitize_osc_st() {
    let mut ed = PromptEditor::new();
    ed.insert_str("hello world");
    ed.replace_range_with_extmark(
        0,
        5,
        ExtmarkKind::FileMention {
            path: "a".to_string(),
        },
        "@one",
    );
    ed.replace_range_with_extmark(
        0,
        4,
        ExtmarkKind::FileMention {
            path: "b".to_string(),
        },
        "@two",
    );
    assert_eq!(ed.extmarks().len(), 1);
    assert!(ed.text().starts_with("@two"));

    let mut ed2 = PromptEditor::new();
    ed2.insert_str("abc\ndefg");
    ed2.set_cursor_char(3);
    assert!(ed2.move_down());
    assert_eq!(ed2.cursor_char(), 7);

    let sanitized = frontal::ui::editor::sanitize_paste_text("a\x1b]0;title\x1b\\b");
    assert_eq!(sanitized, "ab");

    let mut ed3 = PromptEditor::new();
    ed3.insert_paste("1\n2\n3\n4\n5\n6");
    assert_eq!(ed3.extmarks().len(), 1);
    let len = ed3.text().chars().count();
    ed3.replace_range_with_extmark(
        0,
        len,
        ExtmarkKind::FileMention {
            path: "x".to_string(),
        },
        "@x",
    );
    assert_eq!(ed3.extmarks().len(), 1);
    assert!(ed3.undo());
    assert_eq!(
        ed3.take_submitted_expanded().as_deref(),
        Some("1\n2\n3\n4\n5\n6")
    );
}
