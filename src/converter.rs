//! Offline, streaming CSV/JSON conversion with string values and no type inference.
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Conversion cancelled")
}

pub(crate) struct TempFile(pub(crate) PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn staged_output(output: &Path) -> io::Result<(TempFile, File)> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if output.file_name().is_none() {
        return Err(invalid("Choose an output file"));
    }
    (0..100)
        .find_map(|_| {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".zfm-{}-{id}.tmp", std::process::id()));
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(f) => Some(Ok((TempFile(path), f))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Could not allocate temporary file",
            ))
        })
}

/// Convert UTF-8 CSV to a JSON array of objects, with the first row as keys.
/// Never replaces an existing destination. The output is staged in the target
/// directory and published with a no-replace hard link after successful flush.
/// `progress` receives input bytes processed (not a percentage).
pub fn csv_to_json<F: FnMut(u64)>(
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
        return Err(invalid("Input must be a regular file"));
    }
    let (temp, file) = staged_output(output)?;
    let mut writer = BufWriter::new(file);
    let mut reader = CsvReader::new(BufReader::new(source));
    let mut header = reader
        .record(cancel, &mut progress)?
        .ok_or_else(|| invalid("CSV needs a header row"))?;
    if let Some(first) = header.first_mut() {
        *first = first.trim_start_matches('\u{feff}').to_owned();
    }
    let mut seen = HashSet::new();
    if header
        .iter()
        .any(|h| h.is_empty() || !seen.insert(h.clone()))
    {
        return Err(invalid("CSV headers must be non-empty and unique"));
    }
    writer.write_all(b"[\n")?;
    let mut rows = 0u64;
    while let Some(row) = reader.record(cancel, &mut progress)? {
        if row.len() != header.len() {
            return Err(invalid(
                "CSV row has a different number of fields from the header",
            ));
        }
        if rows != 0 {
            writer.write_all(b",\n")?;
        }
        writer.write_all(b"  {")?;
        for (i, (key, value)) in header.iter().zip(row.iter()).enumerate() {
            if i != 0 {
                writer.write_all(b",")?;
            }
            json_string(&mut writer, key)?;
            writer.write_all(b":")?;
            json_string(&mut writer, value)?;
        }
        writer.write_all(b"}")?;
        rows += 1;
    }
    writer.write_all(b"\n]\n")?;
    writer.flush()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    // A hard link is atomic and fails if output exists, including symlinks.
    // If the filesystem doesn't support hard links, fail safely instead.
    fs::hard_link(&temp.0, output)?;
    Ok(rows)
}

fn json_string<W: Write>(out: &mut W, value: &str) -> io::Result<()> {
    out.write_all(b"\"")?;
    for ch in value.chars() {
        match ch {
            '"' => out.write_all(b"\\\"")?,
            '\\' => out.write_all(b"\\\\")?,
            '\n' => out.write_all(b"\\n")?,
            '\r' => out.write_all(b"\\r")?,
            '\t' => out.write_all(b"\\t")?,
            c if c < ' ' => write!(out, "\\u{:04x}", c as u32)?,
            c => {
                let mut b = [0; 4];
                out.write_all(c.encode_utf8(&mut b).as_bytes())?;
            }
        }
    }
    out.write_all(b"\"")
}

#[derive(PartialEq)]
enum State {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}
struct CsvReader<R> {
    bytes: std::io::Bytes<R>,
    count: u64,
    pending: Option<u8>,
}
impl<R: Read> CsvReader<R> {
    fn new(r: R) -> Self {
        Self {
            bytes: r.bytes(),
            count: 0,
            pending: None,
        }
    }
    fn next<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<Option<u8>> {
        if let Some(c) = self.pending.take() {
            return Ok(Some(c));
        }
        if self.count % 8192 == 0 && cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let byte = self.bytes.next().transpose()?;
        if byte.is_some() {
            self.count += 1;
            if self.count % (1024 * 1024) == 0 {
                progress(self.count);
            }
        }
        Ok(byte)
    }
    fn record<F: FnMut(u64)>(
        &mut self,
        cancel: &AtomicBool,
        progress: &mut F,
    ) -> io::Result<Option<Vec<String>>> {
        let mut state = State::Start;
        let mut fields = Vec::new();
        let mut field = Vec::new();
        let mut length = 0usize;
        let mut started = false;
        loop {
            let byte = self.next(cancel, progress)?;
            match byte {
                None => {
                    if state == State::Quoted {
                        return Err(invalid("Unclosed quoted CSV field"));
                    }
                    if !started {
                        return Ok(None);
                    }
                    push_field(&mut fields, field)?;
                    return Ok(Some(fields));
                }
                Some(b) => {
                    started = true;
                    length += 1;
                    if length > MAX_RECORD_BYTES {
                        return Err(invalid("CSV record exceeds the 8 MiB limit"));
                    }
                    match (state, b) {
                        (State::Start, b'"') => state = State::Quoted,
                        (State::Quoted, b'"') => state = State::AfterQuote,
                        (State::Quoted, b) => {
                            field.push(b);
                            state = State::Quoted;
                        }
                        (State::AfterQuote, b'"') => {
                            field.push(b'"');
                            state = State::Quoted;
                        }
                        (State::Start | State::Unquoted | State::AfterQuote, b',') => {
                            push_field(&mut fields, std::mem::take(&mut field))?;
                            state = State::Start;
                        }
                        (State::Start | State::Unquoted | State::AfterQuote, b'\n' | b'\r') => {
                            if b == b'\r' {
                                if let Some(next) = self.next(cancel, progress)? {
                                    if next != b'\n' {
                                        self.pending = Some(next);
                                    }
                                }
                            }
                            push_field(&mut fields, field)?;
                            return Ok(Some(fields));
                        }
                        (State::Start, b) => {
                            field.push(b);
                            state = State::Unquoted;
                        }
                        (State::Unquoted, b'"') => {
                            return Err(invalid("Unexpected quote in CSV field"))
                        }
                        (State::Unquoted, b) => {
                            field.push(b);
                            state = State::Unquoted;
                        }
                        (State::AfterQuote, _) => {
                            return Err(invalid("Unexpected character after closing CSV quote"))
                        }
                    }
                }
            }
        }
    }
}
fn push_field(fields: &mut Vec<String>, field: Vec<u8>) -> io::Result<()> {
    if fields.len() >= 4096 {
        return Err(invalid("CSV record exceeds 4096 fields"));
    }
    fields.push(String::from_utf8(field).map_err(|_| invalid("CSV is not valid UTF-8"))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-convert"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn go(data: &[u8]) -> io::Result<String> {
        let t = Temp::new();
        let input = t.0.join("a.csv");
        let output = t.0.join("a.json");
        fs::write(&input, data)?;
        csv_to_json(&input, &output, &AtomicBool::new(false), |_| {})?;
        fs::read_to_string(output)
    }
    #[test]
    fn csv_quotes_newlines_and_unicode() {
        assert_eq!(
            go("\u{feff}name,note\r\nZoë,\"hello,\n\"\"friend\"\"\"\r\n".as_bytes()).unwrap(),
            "[\n  {\"name\":\"Zoë\",\"note\":\"hello,\\n\\\"friend\\\"\"}\n]\n"
        );
    }
    #[test]
    fn empty_and_invalid_csv() {
        for data in [
            b"".as_slice(),
            b"a,a\n1,2",
            b"a,b\n1",
            b"a\n\"unterminated",
            b"a\n\xff",
            b"a\n\"x\"z",
        ] {
            assert!(go(data).is_err(), "{data:?}");
        }
        assert_eq!(go(b"a,b\n").unwrap(), "[\n\n]\n");
    }
    #[test]
    fn conflicts_and_failures_leave_existing_files_untouched() {
        let t = Temp::new();
        let src = t.0.join("in.csv");
        let out = t.0.join("out.json");
        fs::write(&src, "a\n1\n").unwrap();
        fs::write(&out, "keep").unwrap();
        assert_eq!(
            csv_to_json(&src, &out, &AtomicBool::new(false), |_| {})
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(&out).unwrap(), "keep");
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 2);
        assert!(csv_to_json(&src, &src, &AtomicBool::new(false), |_| {}).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_destination_is_not_overwritten() {
        let t = Temp::new();
        let src = t.0.join("in.csv");
        let target = t.0.join("target");
        let out = t.0.join("out.json");
        fs::write(&src, "a\n1").unwrap();
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, &out).unwrap();
        assert!(csv_to_json(&src, &out, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }
    #[test]
    fn cancel_and_bad_input_clean_temp_files() {
        let t = Temp::new();
        let src = t.0.join("in.csv");
        let out = t.0.join("out.json");
        fs::write(&src, "a\n\"broken").unwrap();
        assert!(csv_to_json(&src, &out, &AtomicBool::new(false), |_| {}).is_err());
        assert!(csv_to_json(&src, &out, &AtomicBool::new(true), |_| {}).is_err());
        assert!(!out.exists());
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    }
    #[test]
    fn cancellation_during_large_conversion_leaves_no_output() {
        let t = Temp::new();
        let src = t.0.join("in.csv");
        let out = t.0.join("out.json");
        let mut data = String::from("name,value\n");
        for _ in 0..100_000 {
            data.push_str("longer value,another value\n");
        }
        fs::write(&src, data).unwrap();
        let cancel = AtomicBool::new(false);
        let error = csv_to_json(&src, &out, &cancel, |_| {
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(!out.exists());
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    }
    #[test]
    fn rejects_excessive_records_and_columns() {
        let mut data = vec![b'x'; MAX_RECORD_BYTES + 1];
        data.push(b'\n');
        assert!(go(&data).is_err());
        assert!(go(format!("{}\n", "x,".repeat(4096)).as_bytes()).is_err());
    }
    #[test]
    fn csv_escapes_json_control_characters() {
        assert_eq!(
            go(b"value\n\"tab\t slash\\ \x01\"\n").unwrap(),
            "[\n  {\"value\":\"tab\\t slash\\\\ \\u0001\"}\n]\n"
        );
    }
}
