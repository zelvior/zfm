//! Local, unencrypted, visually scannable clipboard QR. No networking.
use qrcodegen::{QrCode, QrCodeEcc, QrSegment};
use std::io;

pub const MAX_BYTES: usize = 512;
const QUIET: i32 = 4;
const SCALE: usize = 4;
/// An in-memory 32-bit top-down BGRX image, with a four-module quiet zone.
/// The QR contains the original plaintext; no secrecy from a viewer is implied.
pub struct Image { pub width: usize, pub pixels: Vec<u32> }
pub fn encode(text: &str) -> io::Result<Image> {
    if text.is_empty() || text.len() > MAX_BYTES || text.contains('\0') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Clipboard text must contain 1–512 UTF-8 bytes and no NUL"));
    }
    // ECI 26 declares UTF-8 instead of relying on scanner-specific guessing.
    let segments = [QrSegment::make_eci(26), QrSegment::make_bytes(text.as_bytes())];
    let qr = QrCode::encode_segments(&segments, QrCodeEcc::Medium)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Text is too large for a QR code"))?;
    let width = (qr.size() + QUIET * 2) as usize * SCALE;
    let mut pixels = vec![0x00ff_ffff; width * width];
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                let left = (x + QUIET) as usize * SCALE; let top = (y + QUIET) as usize * SCALE;
                for row in top..top + SCALE { pixels[row * width + left..row * width + left + SCALE].fill(0); }
            }
        }
    }
    Ok(Image { width, pixels })
}
/// BMP for in-memory GTK image decoding. No temporary files are created.
pub fn bmp(image: &Image) -> Vec<u8> {
    let stride = (image.width * 3 + 3) & !3;
    let size = 54 + stride * image.width;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(b"BM"); bytes.extend_from_slice(&(size as u32).to_le_bytes()); bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes()); bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(image.width as i32).to_le_bytes()); bytes.extend_from_slice(&(-(image.width as i32)).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    for row in image.pixels.chunks(image.width) {
        for &pixel in row { bytes.extend_from_slice(&pixel.to_le_bytes()[..3]); }
        for _ in 0..(stride - image.width * 3) { bytes.push(0); }
    }
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn bounds_bitmap_and_utf8() {
        assert!(encode("").is_err()); assert!(encode(&"x".repeat(MAX_BYTES + 1)).is_err());
        assert!(encode("x\0y").is_err());
        let image = encode("Example ü 🌱\nsecond line").unwrap();
        assert!(image.width >= 116 && image.width < 500);
        assert!(image.pixels.iter().any(|p| *p == 0)); assert_eq!(image.pixels[0], 0x00ff_ffff);
        assert!((0..16).all(|x| image.pixels[x] == 0x00ff_ffff));
        let encoded = bmp(&image); assert_eq!(&encoded[..2], b"BM");
        assert_eq!(encoded.len(), 54 + ((image.width * 3 + 3) & !3) * image.width);
        assert_eq!(u32::from_le_bytes(encoded[2..6].try_into().unwrap()) as usize, encoded.len());
        assert!(encode(&"x".repeat(MAX_BYTES)).is_ok());
    }
}
