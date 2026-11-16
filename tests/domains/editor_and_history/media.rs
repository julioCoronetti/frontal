use frontal::infra::clipboard::{adler32, crc32_ieee, encode_rgba_png, SystemClipboard};

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
