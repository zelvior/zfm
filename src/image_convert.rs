//! Small, deliberately restricted, dependency-free BMP/ICO conversion.
//! Only uncompressed bottom-up 24-bit images, up to 256x256, are supported.
//! Input is bounded, and the output uses the converter's no-overwrite staging.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_INPUT: u64 = 1024 * 1024;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Conversion cancelled")
}
fn u16_at(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes([b[n], b[n + 1]])
}
fn u32_at(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes([b[n], b[n + 1], b[n + 2], b[n + 3]])
}
fn put16(b: &mut [u8], n: usize, v: u16) {
    b[n..n + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], n: usize, v: u32) {
    b[n..n + 4].copy_from_slice(&v.to_le_bytes());
}

struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
fn dimensions(dib: &[u8], icon: bool) -> io::Result<(u32, u32, usize, usize)> {
    if dib.len() < 40
        || u32_at(dib, 0) != 40
        || u16_at(dib, 12) != 1
        || u16_at(dib, 14) != 24
        || u32_at(dib, 16) != 0
        || u32_at(dib, 32) != 0
    {
        return Err(invalid(
            "Only uncompressed 24-bit BITMAPINFOHEADER images are supported",
        ));
    }
    let w = u32_at(dib, 4);
    let h_raw = u32_at(dib, 8);
    // A negative signed height would otherwise appear as a huge u32.
    let h = if icon {
        if h_raw % 2 != 0 {
            return Err(invalid("Invalid icon height"));
        }
        h_raw / 2
    } else {
        h_raw
    };
    if w == 0 || w > 256 || h == 0 || h > 256 {
        return Err(invalid("Image dimensions must be between 1 and 256 pixels"));
    }
    let stride = ((w as usize * 3 + 3) / 4) * 4;
    let mask_stride = ((w as usize + 31) / 32) * 4;
    Ok((w, h, stride * h as usize, mask_stride * h as usize))
}
fn parse_bmp(data: &[u8]) -> io::Result<Image> {
    if data.len() < 54 || &data[..2] != b"BM" || u32_at(data, 6) != 0 {
        return Err(invalid("Invalid BMP header"));
    }
    let (width, height, len, _) = dimensions(&data[14..], false)?;
    let offset = u32_at(data, 10) as usize;
    let declared = u32_at(data, 2) as usize;
    let end = offset
        .checked_add(len)
        .ok_or_else(|| invalid("Invalid BMP offset"))?;
    let size_image = u32_at(data, 34) as usize;
    if offset < 54
        || end > data.len()
        || declared < end
        || declared > data.len()
        || (size_image != 0 && size_image != len)
    {
        return Err(invalid("Truncated or inconsistent BMP"));
    }
    Ok(Image {
        width,
        height,
        pixels: data[offset..end].to_vec(),
    })
}
fn parse_ico(data: &[u8]) -> io::Result<Image> {
    if data.len() < 22
        || u16_at(data, 0) != 0
        || u16_at(data, 2) != 1
        || u16_at(data, 4) != 1
        || data[8] != 0
        || data[9] != 0
        || u16_at(data, 10) != 1
        || u16_at(data, 12) != 24
    {
        return Err(invalid(
            "Only single-image 24-bit BMP-based icons are supported",
        ));
    }
    let length = u32_at(data, 14) as usize;
    let offset = u32_at(data, 18) as usize;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("Invalid icon offset"))?;
    if offset < 22 || length < 40 || end > data.len() {
        return Err(invalid("Truncated icon"));
    }
    let dib = &data[offset..end];
    let (width, height, pixel_len, mask_len) = dimensions(dib, true)?;
    let dir_width = if data[6] == 0 {
        256
    } else {
        u32::from(data[6])
    };
    let dir_height = if data[7] == 0 {
        256
    } else {
        u32::from(data[7])
    };
    if width != dir_width || height != dir_height || 40 + pixel_len + mask_len != length {
        return Err(invalid("Inconsistent icon dimensions or image size"));
    }
    // BMP has no transparency. Refuse icons with a nonzero AND mask rather
    // than silently discarding transparency, including nonzero mask padding.
    if dib[40 + pixel_len..].iter().any(|b| *b != 0) {
        return Err(invalid(
            "Transparent icons cannot be converted to 24-bit BMP",
        ));
    }
    Ok(Image {
        width,
        height,
        pixels: dib[40..40 + pixel_len].to_vec(),
    })
}
fn dib_header(image: &Image, icon: bool) -> Vec<u8> {
    let mut h = vec![0u8; 40];
    put32(&mut h, 0, 40);
    put32(&mut h, 4, image.width);
    put32(&mut h, 8, image.height * if icon { 2 } else { 1 });
    put16(&mut h, 12, 1);
    put16(&mut h, 14, 24);
    put32(&mut h, 20, image.pixels.len() as u32);
    h
}
fn encode_bmp(image: &Image) -> Vec<u8> {
    let mut out = vec![0u8; 14];
    out[..2].copy_from_slice(b"BM");
    put32(&mut out, 2, (54 + image.pixels.len()) as u32);
    put32(&mut out, 10, 54);
    out.extend_from_slice(&dib_header(image, false));
    out.extend_from_slice(&image.pixels);
    out
}
fn encode_ico(image: &Image) -> Vec<u8> {
    let mask_len = ((image.width as usize + 31) / 32) * 4 * image.height as usize;
    let mut out = vec![0u8; 22];
    put16(&mut out, 2, 1);
    put16(&mut out, 4, 1);
    out[6] = if image.width == 256 {
        0
    } else {
        image.width as u8
    };
    out[7] = if image.height == 256 {
        0
    } else {
        image.height as u8
    };
    put16(&mut out, 10, 1);
    put16(&mut out, 12, 24);
    put32(&mut out, 14, (40 + image.pixels.len() + mask_len) as u32);
    put32(&mut out, 18, 22);
    out.extend_from_slice(&dib_header(image, true));
    out.extend_from_slice(&image.pixels);
    out.resize(out.len() + mask_len, 0);
    out
}
/// Converts a restricted BMP to an opaque ICO, or an opaque ICO to BMP.
/// Rejects unsupported data rather than flattening or silently losing alpha.
pub fn convert<F: FnMut(u64)>(
    input: &Path,
    output: &Path,
    bmp_to_ico: bool,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<u64> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let file = File::open(input)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Input must be a regular file"));
    }
    let mut data = Vec::new();
    file.take(MAX_INPUT + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_INPUT {
        return Err(invalid("Image exceeds 1 MiB input limit"));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    progress(data.len() as u64);
    let image = if bmp_to_ico {
        parse_bmp(&data)?
    } else {
        parse_ico(&data)?
    };
    let result = if bmp_to_ico {
        encode_ico(&image)
    } else {
        encode_bmp(&image)
    };
    let (temp, mut staged) = crate::converter::staged_output(output)?;
    staged.write_all(&result)?;
    staged.flush()?;
    drop(staged);
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    fs::hard_link(&temp.0, output)?;
    Ok(data.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-images"))
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn sample(w: u32, h: u32) -> Image {
        let stride = ((w as usize * 3 + 3) / 4) * 4;
        let mut pixels = vec![0; stride * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = y * stride + x * 3;
                pixels[i..i + 3].copy_from_slice(&[x as u8, y as u8, 200]);
            }
        }
        Image {
            width: w,
            height: h,
            pixels,
        }
    }
    #[test]
    fn bmp_icon_roundtrip_odd_width_and_maximum_size() {
        for (w, h) in [(1, 1), (3, 2), (256, 256)] {
            let image = sample(w, h);
            let ico = encode_ico(&parse_bmp(&encode_bmp(&image)).unwrap());
            let restored = parse_bmp(&encode_bmp(&parse_ico(&ico).unwrap())).unwrap();
            assert_eq!((restored.width, restored.height), (w, h));
            assert_eq!(restored.pixels, image.pixels);
        }
    }
    #[test]
    fn rejects_bad_sizes_formats_and_transparency() {
        let image = sample(3, 2);
        let mut bmp = encode_bmp(&image);
        put32(&mut bmp, 10, u32::MAX);
        assert!(parse_bmp(&bmp).is_err());
        let mut bmp = encode_bmp(&image);
        put32(&mut bmp, 22, u32::MAX);
        assert!(parse_bmp(&bmp).is_err());
        let mut bmp = encode_bmp(&image);
        put16(&mut bmp, 28, 32);
        assert!(parse_bmp(&bmp).is_err());
        let mut ico = encode_ico(&image);
        *ico.last_mut().unwrap() = 1;
        assert!(parse_ico(&ico).is_err());
        let mut ico = encode_ico(&image);
        put32(&mut ico, 18, u32::MAX);
        assert!(parse_ico(&ico).is_err());
        for n in 0..54 {
            assert!(parse_bmp(&encode_bmp(&image)[..n]).is_err());
        }
        for n in 0..22 {
            assert!(parse_ico(&encode_ico(&image)[..n]).is_err());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn generated_icon_is_readable_by_imagemagick_when_available() {
        let d = Dir::new();
        let path = d.0.join("fixture.ico");
        fs::write(&path, encode_ico(&sample(16, 16))).unwrap();
        let output = match std::process::Command::new("identify")
            .arg("-format")
            .arg("%w %h")
            .arg(&path)
            .output()
        {
            Ok(output) => output,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return,
            Err(e) => panic!("could not run identify: {e}"),
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"16 16");
        let pixels = std::process::Command::new("convert")
            .arg(&path)
            .args(["-depth", "8", "rgba:-"])
            .output()
            .unwrap();
        assert!(
            pixels.status.success(),
            "{}",
            String::from_utf8_lossy(&pixels.stderr)
        );
        assert_eq!(&pixels.stdout[..4], &[200, 15, 0, 255]);
    }
    #[test]
    fn oversized_input_is_rejected_before_publication() {
        let d = Dir::new();
        let src = d.0.join("large.bmp");
        let output = d.0.join("out.ico");
        fs::write(&src, vec![0; MAX_INPUT as usize + 1]).unwrap();
        assert_eq!(
            convert(&src, &output, true, &AtomicBool::new(false), |_| {})
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn destination_symlink_and_input_are_not_overwritten() {
        let d = Dir::new();
        let src = d.0.join("in.bmp");
        let target = d.0.join("target");
        let output = d.0.join("out.ico");
        fs::write(&src, encode_bmp(&sample(3, 2))).unwrap();
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &output).unwrap();
        assert!(convert(&src, &output, true, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        let original = fs::read(&src).unwrap();
        assert!(convert(&src, &src, true, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read(&src).unwrap(), original);
    }
    #[test]
    fn conversion_refuses_overwrite_and_cleans_cancel_and_bad_input() {
        let d = Dir::new();
        let src = d.0.join("in.bmp");
        let dest = d.0.join("out.ico");
        fs::write(&src, encode_bmp(&sample(3, 2))).unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            convert(&src, &dest, true, &flag, |_| flag
                .store(true, Ordering::Relaxed))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!dest.exists());
        fs::write(&dest, b"keep").unwrap();
        assert!(convert(&src, &dest, true, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"keep");
        fs::remove_file(&dest).unwrap();
        fs::write(&src, b"not an image").unwrap();
        assert!(convert(&src, &dest, true, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1);
    }
}
