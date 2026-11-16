use frontal::infra::clipboard::{adler32, crc32_ieee, encode_rgba_png, SystemClipboard};
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

    let rgba = vec![
        10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
    ];
    cb.set_isolated_image(2, 2, rgba);

    let first = cb.get_image().unwrap().expect("first image saved");
    let second = cb.get_image().unwrap().expect("second image saved");

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
    assert!(editor.is_empty());

    editor.insert_media_attachment("image/webp", "/tmp/only.webp");
    let media_only = editor
        .take_submitted_payload()
        .expect("image-only prompt allowed");
    assert_eq!(media_only.text, "");
    assert_eq!(media_only.media.len(), 1);
    assert!(editor.extmarks().is_empty());

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
