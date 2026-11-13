//! Streaming, bounded conversion between uncompressed PCM WAV, AIFF and AU.
//! Only 8/16-bit mono/stereo, integer 1–192000 Hz is supported. Metadata is
//! not preserved. The converter uses no external codecs or processes.
use std::fs;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_INPUT: u64 = 512 * 1024 * 1024;
const MAX_CHUNKS: usize = 4096;
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn cancelled() -> io::Error { io::Error::new(io::ErrorKind::Interrupted, "Conversion cancelled") }
fn le16(b: &[u8]) -> u16 { u16::from_le_bytes([b[0], b[1]]) }
fn be16(b: &[u8]) -> u16 { u16::from_be_bytes([b[0], b[1]]) }
fn le32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
fn be32(b: &[u8]) -> u32 { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }
#[derive(Debug, Clone, Copy)]
struct Audio { channels: u16, bits: u16, rate: u32, frames: u32, data_offset: u64, data_len: u32 }
fn validate_format(channels: u16, bits: u16, rate: u32) -> io::Result<u32> {
    if !(1..=2).contains(&channels) || !matches!(bits, 8 | 16) || !(1..=192_000).contains(&rate) {
        return Err(invalid("Only mono/stereo 8-bit or 16-bit PCM at 1–192000 Hz is supported"));
    }
    Ok(u32::from(channels) * u32::from(bits / 8))
}
fn encode_rate(rate: u32) -> [u8; 10] {
    let exponent = 31 - rate.leading_zeros();
    let mut output = [0; 10];
    output[..2].copy_from_slice(&(16383u16 + exponent as u16).to_be_bytes());
    output[2..].copy_from_slice(&(u64::from(rate) << (63 - exponent)).to_be_bytes());
    output
}
fn decode_rate(b: &[u8]) -> io::Result<u32> {
    let exponent = be16(b);
    let mantissa = u64::from_be_bytes(b[2..10].try_into().unwrap());
    if !(16383..=16400).contains(&exponent) || mantissa & (1 << 63) == 0 {
        return Err(invalid("Unsupported AIFF sample rate"));
    }
    let shift = 63 - u32::from(exponent - 16383);
    if mantissa & ((1u64 << shift) - 1) != 0 { return Err(invalid("AIFF sample rate must be an integer")); }
    let rate = (mantissa >> shift) as u32;
    if !(1..=192_000).contains(&rate) { return Err(invalid("Unsupported AIFF sample rate")); }
    Ok(rate)
}

fn parse<R: Read + Seek>(reader: &mut R, length: u64, wav: bool, cancel: &AtomicBool) -> io::Result<Audio> {
    if length < 12 || length > MAX_INPUT { return Err(invalid("Audio input must be between 12 bytes and 512 MiB")); }
    let mut header = [0u8; 12];
    reader.read_exact(&mut header)?;
    let (id, kind) = if wav { (&b"RIFF"[..], &b"WAVE"[..]) } else { (&b"FORM"[..], &b"AIFF"[..]) };
    if &header[..4] != id || &header[8..] != kind { return Err(invalid("Invalid or unsupported WAV/AIFF container")); }
    let declared = if wav { le32(&header[4..8]) } else { be32(&header[4..8]) };
    if u64::from(declared) + 8 != length { return Err(invalid("Truncated or inconsistent audio container")); }
    let mut format = None;
    let mut samples = None;
    let mut pos = 12u64;
    let mut chunks = 0usize;
    while pos < length {
        if cancel.load(Ordering::Relaxed) { return Err(cancelled()); }
        chunks += 1;
        if chunks > MAX_CHUNKS || length - pos < 8 { return Err(invalid("Invalid or excessive audio chunks")); }
        reader.seek(SeekFrom::Start(pos))?;
        let mut chunk = [0u8; 8]; reader.read_exact(&mut chunk)?;
        let len = if wav { le32(&chunk[4..]) } else { be32(&chunk[4..]) };
        let payload = pos + 8;
        let next = payload.checked_add(u64::from(len) + u64::from(len % 2)).ok_or_else(|| invalid("Audio chunk overflow"))?;
        if next > length { return Err(invalid("Truncated audio chunk")); }
        if wav && &chunk[..4] == b"fmt " {
            if format.is_some() || (len != 16 && len != 18) { return Err(invalid("Unsupported WAV format chunk")); }
            let mut fmt = [0u8; 18]; reader.read_exact(&mut fmt[..len as usize])?;
            if le16(&fmt) != 1 || (len == 18 && le16(&fmt[16..]) != 0) { return Err(invalid("Only uncompressed PCM WAV is supported")); }
            let channels = le16(&fmt[2..]); let rate = le32(&fmt[4..]); let bits = le16(&fmt[14..]);
            let alignment = validate_format(channels, bits, rate)?;
            if u32::from(le16(&fmt[12..])) != alignment || le32(&fmt[8..]) != rate * alignment { return Err(invalid("Inconsistent WAV format")); }
            format = Some((channels, bits, rate, None));
        } else if !wav && &chunk[..4] == b"COMM" {
            if format.is_some() || len != 18 { return Err(invalid("Unsupported AIFF format chunk")); }
            let mut fmt = [0u8; 18]; reader.read_exact(&mut fmt)?;
            let channels = be16(&fmt); let frames = be32(&fmt[2..]); let bits = be16(&fmt[6..]); let rate = decode_rate(&fmt[8..])?;
            validate_format(channels, bits, rate)?;
            format = Some((channels, bits, rate, Some(frames)));
        } else if wav && &chunk[..4] == b"data" {
            if samples.is_some() { return Err(invalid("Duplicate WAV data chunk")); }
            samples = Some((payload, len));
        } else if !wav && &chunk[..4] == b"SSND" {
            if samples.is_some() || len < 8 { return Err(invalid("Invalid AIFF sound chunk")); }
            let mut sound = [0u8; 8]; reader.read_exact(&mut sound)?;
            let offset = be32(&sound); let block = be32(&sound[4..]);
            if block != 0 || offset > len - 8 { return Err(invalid("Unsupported AIFF sound layout")); }
            samples = Some((payload + 8 + u64::from(offset), len - 8 - offset));
        }
        pos = next;
    }
    let (channels, bits, rate, declared_frames) = format.ok_or_else(|| invalid("Audio format chunk is missing"))?;
    let (data_offset, data_len) = samples.ok_or_else(|| invalid("Audio sample chunk is missing"))?;
    let alignment = validate_format(channels, bits, rate)?;
    if data_len % alignment != 0 { return Err(invalid("Audio contains incomplete sample frames")); }
    let frames = data_len / alignment;
    if declared_frames.map_or(false, |n| n != frames) { return Err(invalid("Inconsistent AIFF frame count")); }
    Ok(Audio { channels, bits, rate, frames, data_offset, data_len })
}

// AU stores signed linear PCM samples in big-endian order. An all-ones
// data size denotes the remaining bytes of the file, as allowed by AU.
fn parse_au<R: Read + Seek>(reader: &mut R, length: u64, cancel: &AtomicBool) -> io::Result<Audio> {
    if cancel.load(Ordering::Relaxed) { return Err(cancelled()); }
    if length < 24 || length > MAX_INPUT { return Err(invalid("AU input must be between 24 bytes and 512 MiB")); }
    let mut header = [0; 24];
    reader.read_exact(&mut header)?;
    if &header[..4] != b".snd" { return Err(invalid("Invalid AU header")); }
    let offset = be32(&header[4..8]);
    let declared = be32(&header[8..12]);
    let bits = match be32(&header[12..16]) {
        2 => 8, 3 => 16,
        _ => return Err(invalid("Only uncompressed 8-bit or 16-bit linear PCM AU is supported")),
    };
    let rate = be32(&header[16..20]);
    let channels = u16::try_from(be32(&header[20..24])).map_err(|_| invalid("Unsupported AU channels"))?;
    let align = validate_format(channels, bits, rate)?;
    if offset < 24 || u64::from(offset) > length { return Err(invalid("Invalid AU data offset")); }
    let available = length - u64::from(offset);
    if declared != u32::MAX && u64::from(declared) != available { return Err(invalid("Inconsistent AU data size")); }
    let data_len = u32::try_from(available).map_err(|_| invalid("AU data is too large"))?;
    if data_len % align != 0 { return Err(invalid("Audio contains incomplete sample frames")); }
    Ok(Audio { channels, bits, rate, frames: data_len / align, data_offset: u64::from(offset), data_len })
}

fn au_header(info: Audio) -> Vec<u8> {
    let mut out = Vec::with_capacity(24);
    out.extend_from_slice(b".snd");
    out.extend_from_slice(&24u32.to_be_bytes());
    out.extend_from_slice(&info.data_len.to_be_bytes());
    out.extend_from_slice(&(if info.bits == 8 { 2u32 } else { 3u32 }).to_be_bytes());
    out.extend_from_slice(&info.rate.to_be_bytes());
    out.extend_from_slice(&u32::from(info.channels).to_be_bytes());
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format { Wav, Aiff, Au }

fn output_header(info: Audio, wav: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let padding = info.data_len % 2;
    if wav {
        out.extend_from_slice(b"RIFF"); out.extend_from_slice(&(36 + info.data_len + padding).to_le_bytes()); out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&info.channels.to_le_bytes()); out.extend_from_slice(&info.rate.to_le_bytes());
        let align = info.channels * (info.bits / 8);
        out.extend_from_slice(&(info.rate * u32::from(align)).to_le_bytes()); out.extend_from_slice(&align.to_le_bytes()); out.extend_from_slice(&info.bits.to_le_bytes());
        out.extend_from_slice(b"data"); out.extend_from_slice(&info.data_len.to_le_bytes());
    } else {
        out.extend_from_slice(b"FORM"); out.extend_from_slice(&(46 + info.data_len + padding).to_be_bytes()); out.extend_from_slice(b"AIFFCOMM");
        out.extend_from_slice(&18u32.to_be_bytes()); out.extend_from_slice(&info.channels.to_be_bytes()); out.extend_from_slice(&info.frames.to_be_bytes());
        out.extend_from_slice(&info.bits.to_be_bytes()); out.extend_from_slice(&encode_rate(info.rate));
        out.extend_from_slice(b"SSND"); out.extend_from_slice(&(8 + info.data_len).to_be_bytes()); out.extend_from_slice(&[0u8; 8]);
    }
    out
}

/// Convert WAV to AIFF when `wav_to_aiff` is true, otherwise AIFF to WAV.
/// Streams audio, never overwrites, and cleans the staged file on ordinary error.
pub fn convert<F: FnMut(u64)>(input: &Path, output: &Path, wav_to_aiff: bool,
    cancel: &AtomicBool, mut progress: F) -> io::Result<u64> {
    convert_formats(input, output, if wav_to_aiff { Format::Wav } else { Format::Aiff },
        if wav_to_aiff { Format::Aiff } else { Format::Wav }, cancel, &mut progress)
}

/// Convert restricted linear PCM WAV to AU, or AU to WAV. AU annotation is omitted.
pub fn convert_au<F: FnMut(u64)>(input: &Path, output: &Path, wav_to_au: bool,
    cancel: &AtomicBool, mut progress: F) -> io::Result<u64> {
    convert_formats(input, output, if wav_to_au { Format::Wav } else { Format::Au },
        if wav_to_au { Format::Au } else { Format::Wav }, cancel, &mut progress)
}

fn convert_formats<F: FnMut(u64)>(input: &Path, output: &Path, from: Format, to: Format,
    cancel: &AtomicBool, mut progress: F) -> io::Result<u64> {
    if cancel.load(Ordering::Relaxed) { return Err(cancelled()); }
    let mut source = crate::viewer::open_regular(input)?;
    let length = source.metadata()?.len();
    let info = match from {
        Format::Au => parse_au(&mut source, length, cancel)?,
        Format::Wav | Format::Aiff => parse(&mut source, length, from == Format::Wav, cancel)?,
    };
    source.seek(SeekFrom::Start(info.data_offset))?;
    let (temp, file) = crate::converter::staged_output(output)?;
    let mut writer = BufWriter::new(file);
    let header = if to == Format::Au { au_header(info) } else { output_header(info, to == Format::Wav) };
    writer.write_all(&header)?;
    let mut buffer = [0u8; 65536];
    let mut processed = 0u64;
    while processed < u64::from(info.data_len) {
        if cancel.load(Ordering::Relaxed) { return Err(cancelled()); }
        let take = (u64::from(info.data_len) - processed).min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..take])?;
        if info.bits == 8 { for byte in &mut buffer[..take] { *byte ^= 0x80; } }
        else { for sample in buffer[..take].chunks_exact_mut(2) { sample.swap(0, 1); } }
        writer.write_all(&buffer[..take])?;
        processed += take as u64;
        progress(info.data_offset + processed);
    }
    if to != Format::Au && info.data_len % 2 != 0 { writer.write_all(&[0])?; }
    writer.flush()?;
    drop(writer);
    if cancel.load(Ordering::Relaxed) { return Err(cancelled()); }
    fs::hard_link(&temp.0, output)?;
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { Self(crate::test_support::scratch_dir("zfm-audio")) } }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn fixture(channels: u16, bits: u16, rate: u32, samples: &[u8]) -> Vec<u8> {
        let info = Audio { channels, bits, rate, frames: samples.len() as u32 / (u32::from(channels) * u32::from(bits / 8)), data_offset: 44, data_len: samples.len() as u32 };
        let mut result = output_header(info, true);
        result.extend_from_slice(samples);
        if samples.len() % 2 != 0 { result.push(0); }
        result
    }
    #[test]
    fn known_extended_sample_rates() {
        assert_eq!(encode_rate(44100), [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
        for rate in [1, 8000, 44100, 48000, 192000] { assert_eq!(decode_rate(&encode_rate(rate)).unwrap(), rate); }
        let mut fractional = encode_rate(44100);
        fractional[9] = 1;
        assert!(decode_rate(&fractional).is_err());
    }
    #[test]
    fn roundtrips_pcm_and_padding() {
        let t = Temp::new();
        let cancel = AtomicBool::new(false);
        for (index, (channels, bits, rate, samples)) in [
            (1, 8, 8000, vec![0, 128, 255]),
            (2, 8, 44100, vec![0, 128, 255, 127]),
            (1, 16, 48000, vec![0, 128, 255, 127, 52, 18]),
            (2, 16, 192000, vec![0, 0, 255, 127, 0, 128, 52, 18]),
        ].into_iter().enumerate() {
            let wav = t.0.join(format!("{index}.wav"));
            let aiff = t.0.join(format!("{index}.aiff"));
            let result = t.0.join(format!("{index}-out.wav"));
            let source = fixture(channels, bits, rate, &samples);
            fs::write(&wav, &source).unwrap();
            assert_eq!(convert(&wav, &aiff, true, &cancel, |_| {}).unwrap(), samples.len() as u64);
            assert_eq!(convert(&aiff, &result, false, &cancel, |_| {}).unwrap(), samples.len() as u64);
            assert_eq!(fs::read(&result).unwrap(), source);
        }
    }
    #[test]
    fn rejects_bad_input_conflicts_and_cancel_cleans_up() {
        let t = Temp::new();
        let input = t.0.join("input.wav"); let output = t.0.join("output.aiff");
        let cancel = AtomicBool::new(false);
        let mut bytes = fixture(1, 16, 44100, &[1, 2, 3, 4]);
        bytes[20] = 3; // IEEE float, not PCM
        fs::write(&input, &bytes).unwrap();
        assert!(convert(&input, &output, true, &cancel, |_| {}).is_err());
        assert!(!output.exists());
        bytes[20] = 1;
        fs::write(&input, &bytes[..bytes.len()-1]).unwrap();
        assert!(convert(&input, &output, true, &cancel, |_| {}).is_err());
        fs::write(&input, &bytes).unwrap();
        fs::write(&output, b"keep").unwrap();
        assert!(convert(&input, &output, true, &cancel, |_| {}).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"keep");
        fs::remove_file(&output).unwrap();
        let mut progress = |_| cancel.store(true, Ordering::Relaxed);
        assert_eq!(convert(&input, &output, true, &cancel, &mut progress).unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!output.exists());
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }
    #[test]
    fn rejects_inconsistent_aiff_frame_count_and_duplicate_chunks() {
        let t = Temp::new(); let input = t.0.join("i.wav"); let aiff = t.0.join("x.aiff"); let output = t.0.join("o.wav");
        let cancel = AtomicBool::new(false);
        fs::write(&input, fixture(1, 16, 44100, &[1,2,3,4])).unwrap();
        convert(&input, &aiff, true, &cancel, |_| {}).unwrap();
        let mut bytes = fs::read(&aiff).unwrap();
        bytes[25] = 3; // COMM declares 3 frames; data contains 2
        fs::write(&aiff, &bytes).unwrap();
        assert!(convert(&aiff, &output, false, &cancel, |_| {}).is_err());
        assert!(!output.exists());
        let mut bytes = fixture(1, 8, 8000, &[1,2]);
        bytes.extend_from_slice(b"data\x02\x00\x00\x00\x01\x02");
        let size = bytes.len() as u32 - 8;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        fs::write(&input, &bytes).unwrap();
        assert!(convert(&input, &output, true, &cancel, |_| {}).is_err());
    }
    #[test]
    fn skips_wav_metadata_and_honours_aiff_sample_offset() {
        let t = Temp::new();
        let input = t.0.join("input.wav"); let aiff = t.0.join("output.aiff"); let output = t.0.join("output.wav");
        let cancel = AtomicBool::new(false);
        let original = fixture(1, 8, 8000, &[0, 128, 255]);
        let mut with_metadata = original.clone();
        with_metadata.extend_from_slice(b"LIST\x03\x00\x00\x00abc\x00");
        let size = with_metadata.len() as u32 - 8;
        with_metadata[4..8].copy_from_slice(&size.to_le_bytes());
        fs::write(&input, &with_metadata).unwrap();
        convert(&input, &aiff, true, &cancel, |_| {}).unwrap();
        let mut data = fs::read(&aiff).unwrap();
        // AIFF SSND starts at byte 38; insert two bytes of offset before samples.
        let length = be32(&data[42..46]);
        data[42..46].copy_from_slice(&(length + 2).to_be_bytes());
        data[46..50].copy_from_slice(&2u32.to_be_bytes());
        data.splice(54..54, [0u8, 0]);
        let size = data.len() as u32 - 8;
        data[4..8].copy_from_slice(&size.to_be_bytes());
        fs::write(&aiff, data).unwrap();
        convert(&aiff, &output, false, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(output).unwrap(), original);
    }
    #[test]
    fn interoperates_with_python_audio_readers_when_available() {
        use std::process::Command;
        if !Command::new("python3").args(["-c", "import aifc, wave"]).status().map_or(false, |s| s.success()) {
            eprintln!("Skipping independent audio interoperability check: Python aifc is unavailable"); return;
        }
        let t = Temp::new(); let input = t.0.join("input.wav"); let aiff = t.0.join("written.aiff");
        let external = t.0.join("external.aiff"); let output = t.0.join("converted.wav");
        let input8 = t.0.join("input8.wav"); let aiff8 = t.0.join("written8.aiff");
        fs::write(&input, fixture(1, 16, 44100, &[0, 128, 255, 127, 52, 18])).unwrap();
        fs::write(&input8, fixture(1, 8, 8000, &[0, 128, 255])).unwrap();
        let cancel = AtomicBool::new(false);
        convert(&input, &aiff, true, &cancel, |_| {}).unwrap();
        convert(&input8, &aiff8, true, &cancel, |_| {}).unwrap();
        let script = r#"
import aifc, sys, wave
with aifc.open(sys.argv[1], 'rb') as f:
    assert (f.getnchannels(), f.getsampwidth(), f.getframerate(), f.getnframes()) == (1, 2, 44100, 3)
    assert f.readframes(3) == bytes([128, 0, 127, 255, 18, 52])
with aifc.open(sys.argv[3], 'rb') as f:
    assert (f.getnchannels(), f.getsampwidth(), f.getframerate(), f.getnframes()) == (1, 1, 8000, 3)
    assert f.readframes(3) == bytes([128, 0, 127])
with aifc.open(sys.argv[2], 'wb') as f:
    f.aiff()
    f.setnchannels(1); f.setsampwidth(2); f.setframerate(44100)
    f.writeframes(bytes([128, 0, 127, 255, 18, 52]))
"#;
        let status = Command::new("python3").arg("-c").arg(script).arg(&aiff).arg(&external).arg(&aiff8).status().unwrap();
        assert!(status.success());
        convert(&external, &output, false, &cancel, |_| {}).unwrap();
        let status = Command::new("python3").arg("-c").arg("import wave,sys; f=wave.open(sys.argv[1], 'rb'); assert (f.getnchannels(),f.getsampwidth(),f.getframerate(),f.getnframes()) == (1,2,44100,3); assert f.readframes(3) == bytes([0,128,255,127,52,18])").arg(&output).status().unwrap();
        assert!(status.success());
    }
    #[test]
    fn au_roundtrips_pcm_and_signedness() {
        let t = Temp::new(); let cancel = AtomicBool::new(false);
        for (i, (channels, bits, rate, samples, expected)) in [
            (1, 8, 8000, vec![0, 128, 255], vec![128, 0, 127]),
            (2, 8, 44100, vec![0, 128, 255, 127], vec![128, 0, 127, 255]),
            (1, 16, 48000, vec![0, 128, 255, 127], vec![128, 0, 127, 255]),
            (2, 16, 192000, vec![1, 2, 3, 4], vec![2, 1, 4, 3]),
        ].into_iter().enumerate() {
            let wav = t.0.join(format!("{i}.wav")); let au = t.0.join(format!("{i}.au"));
            let back = t.0.join(format!("{i}-back.wav"));
            let source = fixture(channels, bits, rate, &samples);
            fs::write(&wav, &source).unwrap();
            convert_au(&wav, &au, true, &cancel, |_| {}).unwrap();
            let bytes = fs::read(&au).unwrap();
            assert_eq!(&bytes[..4], b".snd");
            assert_eq!(&bytes[24..], expected.as_slice()); // AU has no pad byte
            assert_eq!(be32(&bytes[8..]), samples.len() as u32);
            convert_au(&au, &back, false, &cancel, |_| {}).unwrap();
            assert_eq!(fs::read(back).unwrap(), source);
        }
    }
    #[test]
    fn au_rejects_bad_formats_and_handles_annotation_and_unknown_size() {
        let t = Temp::new(); let input = t.0.join("in.au"); let output = t.0.join("out.wav");
        let cancel = AtomicBool::new(false);
        let info = Audio { channels: 1, bits: 8, rate: 8000, frames: 3, data_offset: 0, data_len: 3 };
        let mut bytes = au_header(info);
        bytes[4..8].copy_from_slice(&28u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        bytes.extend_from_slice(b"note"); bytes.extend_from_slice(&[128, 0, 127]);
        fs::write(&input, &bytes).unwrap();
        convert_au(&input, &output, false, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(&output).unwrap(), fixture(1, 8, 8000, &[0, 128, 255]));
        fs::remove_file(&output).unwrap();
        for (offset, value) in [(4, 23u32), (4, 99), (8, 4), (12, 1), (16, 0), (20, 3)] {
            let mut invalid_data = bytes.clone();
            invalid_data[offset..offset+4].copy_from_slice(&value.to_be_bytes());
            fs::write(&input, invalid_data).unwrap();
            assert!(convert_au(&input, &output, false, &cancel, |_| {}).is_err());
            assert!(!output.exists());
        }
        let mut incomplete = bytes.clone();
        incomplete[12..16].copy_from_slice(&3u32.to_be_bytes());
        fs::write(&input, incomplete).unwrap();
        assert!(convert_au(&input, &output, false, &cancel, |_| {}).is_err());
        fs::write(&input, &bytes).unwrap();
        fs::write(&output, b"keep").unwrap();
        assert!(convert_au(&input, &output, false, &cancel, |_| {}).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"keep");
        fs::remove_file(&output).unwrap();
        let mut progress = |_| cancel.store(true, Ordering::Relaxed);
        assert_eq!(convert_au(&input, &output, false, &cancel, &mut progress).unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!output.exists());
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }
    #[test]
    fn au_interoperates_with_python_sunau_when_available() {
        use std::process::Command;
        if !Command::new("python3").args(["-c", "import sunau"]).status().map_or(false, |s| s.success()) { return; }
        let t = Temp::new(); let wav = t.0.join("in.wav"); let au = t.0.join("out.au");
        let external = t.0.join("external.au"); let output = t.0.join("external.wav");
        let source = fixture(1, 16, 8000, &[0, 128, 255, 127]);
        fs::write(&wav, &source).unwrap();
        let cancel = AtomicBool::new(false);
        convert_au(&wav, &au, true, &cancel, |_| {}).unwrap();
        let script = r#"
import sunau, sys
with sunau.open(sys.argv[1], 'rb') as f:
    assert (f.getnchannels(), f.getsampwidth(), f.getframerate(), f.getnframes()) == (1, 2, 8000, 2)
    assert f.readframes(2) == bytes([128, 0, 127, 255])
with sunau.open(sys.argv[2], 'wb') as f:
    f.setnchannels(1); f.setsampwidth(2); f.setframerate(8000); f.setcomptype('NONE', 'not compressed')
    f.writeframes(bytes([128, 0, 127, 255]))
"#;
        assert!(Command::new("python3").arg("-c").arg(script).arg(&au).arg(&external).status().unwrap().success());
        convert_au(&external, &output, false, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(output).unwrap(), source);
    }
    #[cfg(unix)]
    #[test]
    fn input_symlink_is_rejected() {
        let t = Temp::new(); let input = t.0.join("input.wav"); let link = t.0.join("link.wav");
        fs::write(&input, fixture(1, 8, 8000, &[0,1])).unwrap();
        std::os::unix::fs::symlink(&input, &link).unwrap();
        assert!(convert(&link, &t.0.join("out.aiff"), true, &AtomicBool::new(false), |_| {}).is_err());
    }
}
