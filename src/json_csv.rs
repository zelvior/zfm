//! Streaming JSON (array of string-valued objects) to CSV.
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
const LIMIT: u64 = 8 * 1024 * 1024;
fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Conversion cancelled")
}
struct Parser<R> {
    bytes: std::io::Bytes<R>,
    pending: Option<u8>,
    count: u64,
    row_start: Option<u64>,
}
impl<R: Read> Parser<R> {
    fn new(reader: R) -> Self {
        Self {
            bytes: reader.bytes(),
            pending: None,
            count: 0,
            row_start: None,
        }
    }
    fn next<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<Option<u8>> {
        if let Some(byte) = self.pending.take() {
            return Ok(Some(byte));
        }
        if self.count % 8192 == 0 && cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let byte = self.bytes.next().transpose()?;
        if byte.is_some() {
            self.count += 1;
            if self
                .row_start
                .map_or(false, |start| self.count - start > LIMIT)
            {
                return Err(bad("JSON object exceeds the 8 MiB limit"));
            }
            if self.count % (1024 * 1024) == 0 {
                progress(self.count);
            }
        }
        Ok(byte)
    }
    fn nonspace<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<Option<u8>> {
        loop {
            match self.next(cancel, progress)? {
                Some(b' ' | b'\n' | b'\r' | b'\t') => (),
                byte => return Ok(byte),
            }
        }
    }
    fn expect<F: FnMut(u64)>(
        &mut self,
        byte: u8,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<()> {
        if self.nonspace(cancel, progress)? == Some(byte) {
            Ok(())
        } else {
            Err(bad("Unexpected JSON syntax"))
        }
    }
    fn hex4<F: FnMut(u64)>(&mut self, cancel: &AtomicBool, progress: &mut F) -> io::Result<u16> {
        let mut result = 0u16;
        for _ in 0..4 {
            let b = self
                .next(cancel, progress)?
                .ok_or_else(|| bad("Truncated Unicode escape"))?;
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(bad("Invalid Unicode escape")),
            };
            result = (result << 4) | digit as u16;
        }
        Ok(result)
    }
    fn string<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<String> {
        self.expect(b'"', cancel, progress)?;
        let mut out = Vec::new();
        loop {
            match self
                .next(cancel, progress)?
                .ok_or_else(|| bad("Unterminated JSON string"))?
            {
                b'"' => return String::from_utf8(out).map_err(|_| bad("JSON is not valid UTF-8")),
                b'\\' => match self
                    .next(cancel, progress)?
                    .ok_or_else(|| bad("Truncated JSON escape"))?
                {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let first = self.hex4(cancel, progress)?;
                        let scalar = if (0xd800..=0xdbff).contains(&first) {
                            if self.next(cancel, progress)? != Some(b'\\')
                                || self.next(cancel, progress)? != Some(b'u')
                            {
                                return Err(bad("Missing low surrogate"));
                            }
                            let second = self.hex4(cancel, progress)?;
                            if !(0xdc00..=0xdfff).contains(&second) {
                                return Err(bad("Invalid low surrogate"));
                            }
                            0x10000 + (((first as u32 - 0xd800) << 10) | (second as u32 - 0xdc00))
                        } else if (0xdc00..=0xdfff).contains(&first) {
                            return Err(bad("Unexpected low surrogate"));
                        } else {
                            first as u32
                        };
                        let ch =
                            char::from_u32(scalar).ok_or_else(|| bad("Invalid Unicode scalar"))?;
                        let mut encoded = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
                    }
                    _ => return Err(bad("Invalid JSON escape")),
                },
                0..=31 => return Err(bad("Unescaped control character in JSON")),
                b => out.push(b),
            }
        }
    }
    fn object<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<Vec<(String, String)>> {
        self.row_start = Some(self.count);
        self.expect(b'{', cancel, progress)?;
        let mut entries = Vec::new();
        let mut keys = HashSet::new();
        loop {
            let key = self.string(cancel, progress)?;
            if key.is_empty() || !keys.insert(key.clone()) {
                return Err(bad("JSON object keys must be non-empty and unique"));
            }
            self.expect(b':', cancel, progress)?;
            let value = self.string(cancel, progress)?;
            entries.push((key, value));
            if entries.len() > 4096 {
                return Err(bad("JSON object exceeds 4096 fields"));
            }
            match self.nonspace(cancel, progress)? {
                Some(b',') => (),
                Some(b'}') => break,
                _ => return Err(bad("Expected comma or closing brace")),
            }
        }
        self.row_start = None;
        Ok(entries)
    }
}
fn csv_cell<W: Write>(out: &mut W, value: &str) -> io::Result<()> {
    out.write_all(b"\"")?;
    for part in value.as_bytes() {
        if *part == b'"' {
            out.write_all(b"\"\"")?;
        } else {
            out.write_all(&[*part])?;
        }
    }
    out.write_all(b"\"")
}
fn csv_line<'a, W: Write>(out: &mut W, cells: impl Iterator<Item = &'a str>) -> io::Result<()> {
    for (i, cell) in cells.enumerate() {
        if i > 0 {
            out.write_all(b",")?;
        }
        csv_cell(out, cell)?;
    }
    out.write_all(b"\r\n")
}
/// Input is a non-empty JSON array of objects with identical, non-empty keys
/// and string values. Keys may appear in different orders. No type coercion.
pub fn json_to_csv<F: FnMut(u64)>(
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<u64> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let source = File::open(input)?;
    if !source.metadata()?.is_file() {
        return Err(bad("Input must be a regular file"));
    }
    let (temp, file) = crate::converter::staged_output(output)?;
    let mut out = BufWriter::new(file);
    let mut parser = Parser::new(BufReader::new(source));
    parser.expect(b'[', cancel, &mut progress)?;
    let first = parser.object(cancel, &mut progress)?;
    let header: Vec<String> = first.iter().map(|(k, _)| k.clone()).collect();
    csv_line(&mut out, header.iter().map(String::as_str))?;
    csv_line(&mut out, first.iter().map(|(_, v)| v.as_str()))?;
    let mut rows = 1u64;
    loop {
        match parser.nonspace(cancel, &mut progress)? {
            Some(b']') => break,
            Some(b',') => {
                let row = parser.object(cancel, &mut progress)?;
                if row.len() != header.len() {
                    return Err(bad("JSON objects have different keys"));
                }
                let values: HashMap<String, String> = row.into_iter().collect();
                if header.iter().any(|key| !values.contains_key(key)) {
                    return Err(bad("JSON objects have different keys"));
                }
                csv_line(&mut out, header.iter().map(|key| values[key].as_str()))?;
                rows += 1;
            }
            _ => return Err(bad("Expected comma or closing bracket")),
        }
    }
    if parser.nonspace(cancel, &mut progress)?.is_some() {
        return Err(bad("Extra data after JSON array"));
    }
    out.flush()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    drop(out);
    fs::hard_link(&temp.0, output)?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-json-csv"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn convert(json: &[u8]) -> io::Result<String> {
        let t = Temp::new();
        let i = t.0.join("in.json");
        let o = t.0.join("out.csv");
        fs::write(&i, json)?;
        json_to_csv(&i, &o, &AtomicBool::new(false), |_| {})?;
        fs::read_to_string(o)
    }
    #[test]
    fn unicode_quotes_newlines_and_key_reordering() {
        let result=convert(br#"[{"name":"Zo\u00eb","note":"hi,\n\"friend\" \ud83d\ude00"},{"note":"other","name":"B"}]"#).unwrap();
        assert_eq!(
            result,
            "\"name\",\"note\"\r\n\"Zoë\",\"hi,\n\"\"friend\"\" 😀\"\r\n\"B\",\"other\"\r\n"
        );
    }
    #[test]
    fn rejects_invalid_or_unsupported_values() {
        for data in [
            b"[]".as_slice(),
            b"[{}]",
            b"[{\"a\":1}]",
            b"[{\"a\":\"1\",\"a\":\"2\"}]",
            b"[{\"a\":\"1\"},{\"b\":\"2\"}]",
            b"[{\"a\":\"\\uD800\"}]",
            b"[{\"a\":\"x\"},]",
            b"[{\"a\":\"x\"}] garbage",
        ] {
            assert!(convert(data).is_err(), "{:?}", data);
        }
    }
    #[test]
    fn roundtrip_and_conflicts() {
        let t = Temp::new();
        let csv = t.0.join("data.csv");
        let json = t.0.join("data.json");
        let output = t.0.join("out.csv");
        fs::write(&csv, "name,note\nZoë,\"hello, world\"\n").unwrap();
        crate::converter::csv_to_json(&csv, &json, &AtomicBool::new(false), |_| {}).unwrap();
        json_to_csv(&json, &output, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "\"name\",\"note\"\r\n\"Zoë\",\"hello, world\"\r\n"
        );
        assert!(json_to_csv(&json, &output, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "\"name\",\"note\"\r\n\"Zoë\",\"hello, world\"\r\n"
        );
    }
    #[test]
    fn cancellation_removes_partial_output() {
        let t = Temp::new();
        let json = t.0.join("in.json");
        let output = t.0.join("out.csv");
        let data = format!("[{{\"x\":\"{}\"}}]", "z".repeat(2 * 1024 * 1024));
        fs::write(&json, data).unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            json_to_csv(&json, &output, &flag, |_| flag
                .store(true, Ordering::Relaxed))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    }
}
