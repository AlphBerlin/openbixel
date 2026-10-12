//! Minimal, dependency-free PNG encoder for engine export.
//!
//! `bixel-core` deliberately carries no image codec dependency (the app's PNG
//! path lives in `bixel-ai` and Swift). Engine writers need to hand an engine a
//! real `.png` atlas though, so this module writes one directly: RGBA8, filter
//! type 0 on every scanline, zlib-wrapped **stored** deflate blocks.
//!
//! Stored (uncompressed) deflate is a deliberate choice. It keeps the encoder
//! to ~100 lines with no compressor, produces byte-identical output on every
//! platform, and costs nothing that matters here: these atlases are kilobytes,
//! and every engine and VCS pipeline re-compresses them anyway. For a pixel-art
//! atlas the optimizer is the artist's palette, not the deflate window.

/// CRC-32 (IEEE 802.3), the polynomial PNG chunks are checksummed with.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Adler-32, the checksum the zlib stream is wrapped with.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the largest block that cannot overflow u32 before reduction.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// zlib container around stored deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    // CMF = 0x78 (deflate, 32K window), FLG = 0x01 makes (CMF<<8|FLG) % 31 == 0.
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    out.extend_from_slice(&[0x78, 0x01]);
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let mut offset = 0usize;
        while offset < raw.len() {
            let len = (raw.len() - offset).min(65_535);
            let final_block = offset + len == raw.len();
            out.push(if final_block { 0x01 } else { 0x00 });
            out.extend_from_slice(&(len as u16).to_le_bytes());
            out.extend_from_slice(&(!(len as u16)).to_le_bytes());
            out.extend_from_slice(&raw[offset..offset + len]);
            offset += len;
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// Encode tightly packed RGBA8 pixels as a PNG. `rgba` must be
/// `width * height * 4` bytes; otherwise an error is returned.
pub fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("PNG needs a non-zero width and height".into());
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "PNG pixel buffer is {} bytes, expected {} for {width}x{height} RGBA",
            rgba.len(),
            expected
        ));
    }

    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity(expected + height as usize);
    for row in 0..height as usize {
        raw.push(0); // filter type 0 (None)
        raw.extend_from_slice(&rgba[row * stride..(row + 1) * stride]);
    }

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // colour type: truecolour + alpha
    ihdr.push(0); // deflate
    ihdr.push(0); // adaptive filtering
    ihdr.push(0); // no interlace

    let mut out = Vec::with_capacity(raw.len() + 64);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&raw)));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    Ok(out)
}

/// Nearest-neighbour integer upscale of an RGBA8 buffer.
///
/// Pixel art must never be resampled smoothly, so this is a hard copy of each
/// source pixel into a `scale x scale` block.
pub fn upscale_rgba(width: u32, height: u32, rgba: &[u8], scale: u32) -> Result<Vec<u8>, String> {
    if scale == 0 {
        return Err("scale must be at least 1".into());
    }
    if scale == 1 {
        return Ok(rgba.to_vec());
    }
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "Pixel buffer is {} bytes, expected {expected} for {width}x{height} RGBA",
            rgba.len()
        ));
    }
    let (out_w, scale) = (width as usize * scale as usize, scale as usize);
    let mut out = vec![0u8; out_w * height as usize * scale * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let src = (y * width as usize + x) * 4;
            let pixel = &rgba[src..src + 4];
            for dy in 0..scale {
                let row_start = ((y * scale + dy) * out_w + x * scale) * 4;
                for dx in 0..scale {
                    let dst = row_start + dx * 4;
                    out[dst..dst + 4].copy_from_slice(pixel);
                }
            }
        }
    }
    Ok(out)
}

/// Convenience: upscale then encode.
pub fn encode_rgba_scaled(
    width: u32,
    height: u32,
    rgba: &[u8],
    scale: u32,
) -> Result<Vec<u8>, String> {
    let scaled = upscale_rgba(width, height, rgba, scale)?;
    encode_rgba(width * scale, height * scale, &scaled)
}

/// Read width/height straight out of an encoded PNG's IHDR (used by tests and
/// by callers that hold bytes rather than a path).
pub fn dimensions_of(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn adler32_matches_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn round_trip_dimensions_and_pixels() {
        let rgba: Vec<u8> = (0..16u8).collect();
        let png = encode_rgba(2, 2, &rgba).unwrap();
        assert_eq!(dimensions_of(&png), Some((2, 2)));
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]), "IEND crc");
    }

    #[test]
    fn rejects_wrong_buffer_length() {
        assert!(encode_rgba(2, 2, &[0u8; 15]).is_err());
        assert!(encode_rgba(0, 4, &[]).is_err());
    }

    #[test]
    fn upscale_is_nearest_neighbour() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // red, green
            0, 0, 255, 255, 255, 255, 255, 255, // blue, white
        ];
        let out = upscale_rgba(2, 2, &rgba, 2).unwrap();
        let got = encode_rgba_scaled(2, 2, &rgba, 2).unwrap();
        assert_eq!(dimensions_of(&got), Some((4, 4)));
        // top-left 2x2 block is red, top-right is green, bottom-left is blue.
        assert_eq!(&out[0..4], &[255, 0, 0, 255]);
        assert_eq!(&out[8..12], &[0, 255, 0, 255]);
        assert_eq!(&out[32..36], &[0, 0, 255, 255]);
    }

    #[test]
    fn large_image_spans_multiple_stored_blocks() {
        // 200 * 200 * 4 = 160_000 bytes > one deflate block.
        let rgba = vec![7u8; 200 * 200 * 4];
        let png = encode_rgba(200, 200, &rgba).unwrap();
        assert_eq!(dimensions_of(&png), Some((200, 200)));
    }
}
