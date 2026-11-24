pub const MAX_RAW_RGBA_BYTES: usize = 32 * 1024 * 1024;
pub const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            let mask = (c & 1).wrapping_neg();
            c = (c >> 1) ^ (0xEDB8_8320u32 & mask);
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const CRC32_TABLE: [u32; 256] = build_crc32_table();

pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &byte in data {
        let idx = ((c ^ (byte as u32)) & 0xFF) as usize;
        c = (c >> 8) ^ CRC32_TABLE[idx];
    }
    !c
}

pub fn crc32_ieee(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

pub fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &byte in data {
        s1 = (s1 + byte as u32) % MOD_ADLER;
        s2 = (s2 + s1) % MOD_ADLER;
    }
    (s2 << 16) | s1
}

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let crc = crc32_update(crc32_update(0, chunk_type), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("image dimensions must be non-zero".to_string());
    }
    let row_bytes = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| "image row width overflow".to_string())?;
    let expected_len = row_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| "image byte size overflow".to_string())?;
    if expected_len > MAX_RAW_RGBA_BYTES {
        return Err("image exceeds 32 MiB limit".to_string());
    }
    if rgba.len() != expected_len {
        return Err(format!(
            "rgba buffer length mismatch: expected {}, got {}",
            expected_len,
            rgba.len()
        ));
    }

    let mut filtered = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for row in rgba.chunks_exact(row_bytes) {
        filtered.push(0);
        filtered.extend_from_slice(row);
    }

    let mut zlib = Vec::with_capacity(filtered.len() + 64);
    zlib.push(0x78);
    zlib.push(0x01);
    let total_chunks = filtered.len().div_ceil(65_535);
    for (idx, chunk) in filtered.chunks(65_535).enumerate() {
        let is_last = idx + 1 == total_chunks;
        zlib.push(u8::from(is_last));
        let len = chunk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&filtered).to_be_bytes());

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;

    let mut out = Vec::with_capacity(PNG_SIGNATURE.len() + 25 + zlib.len() + 24);
    out.extend_from_slice(&PNG_SIGNATURE);
    write_png_chunk(&mut out, b"IHDR", &ihdr);
    write_png_chunk(&mut out, b"IDAT", &zlib);
    write_png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

fn extract_channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max_val = mask >> shift;
    if max_val == 0 || !max_val.wrapping_add(1).is_power_of_two() {
        return 0;
    }
    let val = (pixel & mask) >> shift;
    if max_val == 255 {
        val as u8
    } else {
        ((val as u64 * 255 + (max_val as u64 / 2)) / max_val as u64) as u8
    }
}

pub fn decode_bmp_to_rgba(bmp: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bmp.len() < 54 || &bmp[0..2] != b"BM" {
        return Err("invalid BMP signature".to_string());
    }
    let pixel_offset = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
    let dib_size = u32::from_le_bytes(bmp[14..18].try_into().unwrap()) as usize;
    if dib_size < 40 || pixel_offset < 14 + dib_size {
        return Err("unsupported BMP DIB header".to_string());
    }
    let width_i32 = i32::from_le_bytes(bmp[18..22].try_into().unwrap());
    let height_i32 = i32::from_le_bytes(bmp[22..26].try_into().unwrap());
    if width_i32 <= 0 || height_i32 == 0 || height_i32 == i32::MIN {
        return Err("invalid BMP dimensions".to_string());
    }
    let width = width_i32 as u32;
    let top_down = height_i32 < 0;
    let height = height_i32.unsigned_abs();
    let bpp = u16::from_le_bytes(bmp[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bmp[30..34].try_into().unwrap());

    if !matches!(bpp, 24 | 32) || !matches!(compression, 0 | 3) {
        return Err(format!(
            "unsupported BMP format: bpp={}, compression={}",
            bpp, compression
        ));
    }
    if bpp == 24 && compression != 0 {
        return Err("unsupported 24bpp BMP compression".to_string());
    }

    let rgba_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "BMP RGBA size overflow".to_string())?;
    if rgba_len > MAX_RAW_RGBA_BYTES {
        return Err("BMP exceeds 32 MiB limit".to_string());
    }

    let stride = (width as usize)
        .checked_mul(bpp as usize)
        .and_then(|bits| bits.checked_add(31))
        .map(|bits| (bits / 32) * 4)
        .ok_or_else(|| "BMP stride overflow".to_string())?;
    let required_len = stride
        .checked_mul(height as usize)
        .and_then(|n| pixel_offset.checked_add(n))
        .ok_or_else(|| "BMP pixel data overflow".to_string())?;
    if bmp.len() < required_len {
        return Err("truncated BMP pixel buffer".to_string());
    }

    let (r_mask, g_mask, b_mask, a_mask) = if bpp == 32 && compression == 3 {
        let r = if bmp.len() >= 58 && (dib_size >= 52 || pixel_offset >= 58) {
            u32::from_le_bytes(bmp[54..58].try_into().unwrap())
        } else {
            0x00FF_0000
        };
        let g = if bmp.len() >= 62 && (dib_size >= 52 || pixel_offset >= 62) {
            u32::from_le_bytes(bmp[58..62].try_into().unwrap())
        } else {
            0x0000_FF00
        };
        let b = if bmp.len() >= 66 && (dib_size >= 52 || pixel_offset >= 66) {
            u32::from_le_bytes(bmp[62..66].try_into().unwrap())
        } else {
            0x0000_00FF
        };
        let a = if bmp.len() >= 70 && (dib_size >= 56 || pixel_offset >= 70) {
            u32::from_le_bytes(bmp[66..70].try_into().unwrap())
        } else {
            0
        };
        (r, g, b, a)
    } else {
        (0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000)
    };

    let mut rgba = vec![0u8; rgba_len];
    let mut any_non_zero_alpha = false;
    let pixels = &bmp[pixel_offset..];

    for dst_y in 0..height as usize {
        let src_y = if top_down {
            dst_y
        } else {
            height as usize - 1 - dst_y
        };
        let src_row = &pixels[src_y * stride..src_y * stride + stride];
        let dst_row = &mut rgba[dst_y * (width as usize * 4)..(dst_y + 1) * (width as usize * 4)];

        if bpp == 24 {
            for x in 0..width as usize {
                let s = x * 3;
                let d = x * 4;
                dst_row[d] = src_row[s + 2];
                dst_row[d + 1] = src_row[s + 1];
                dst_row[d + 2] = src_row[s];
                dst_row[d + 3] = 255;
            }
            any_non_zero_alpha = true;
        } else {
            for x in 0..width as usize {
                let s = x * 4;
                let d = x * 4;
                let px = u32::from_le_bytes(src_row[s..s + 4].try_into().unwrap());
                dst_row[d] = extract_channel(px, r_mask);
                dst_row[d + 1] = extract_channel(px, g_mask);
                dst_row[d + 2] = extract_channel(px, b_mask);
                let alpha = if a_mask == 0 {
                    255
                } else {
                    extract_channel(px, a_mask)
                };
                if alpha != 0 {
                    any_non_zero_alpha = true;
                }
                dst_row[d + 3] = alpha;
            }
        }
    }

    if bpp == 32 && !any_non_zero_alpha {
        for chunk in rgba.as_chunks_mut::<4>().0 {
            chunk[3] = 255;
        }
    }

    Ok((width, height, rgba))
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf = [0u8; 4];
    let mut count = 0usize;
    let mut pad = 0usize;

    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            buf[count] = 0;
            count += 1;
            pad += 1;
            if pad > 2 {
                return Err("invalid base64 padding".to_string());
            }
        } else {
            if pad > 0 {
                return Err("invalid base64 padding".to_string());
            }
            let val = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return Err("invalid base64 character".to_string()),
            };
            buf[count] = val;
            count += 1;
        }

        if count == 4 {
            let n = ((buf[0] as u32) << 18)
                | ((buf[1] as u32) << 12)
                | ((buf[2] as u32) << 6)
                | (buf[3] as u32);
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
            count = 0;
            pad = 0;
        }
    }

    if count != 0 {
        return Err("truncated base64 input".to_string());
    }
    Ok(out)
}
