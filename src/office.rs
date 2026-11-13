//! Limited, read-only text preview for DOCX, PPTX, XLSX and ODT containers.
//! This is not a layout engine: fonts, formatting, images, tables, charts,
//! macros and many other features are not rendered. Only known XML members are
//! streamed from the bundled archive engine; archive paths are never extracted.
use crate::archive_engine;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

const MAX_XML: u64 = 4 * 1024 * 1024;
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

fn member_bytes(path: &Path, member: &str, deadline: Instant, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut command = Command::new(archive_engine::engine_path()?);
    command.args(["x", "-so", "-bd", "-p-", "-spd", "--"]).arg(path).arg(member)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("Could not read document data")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_XML + 1).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let status = loop {
        if Instant::now() >= deadline || cancel.load(Ordering::Relaxed) {
            let _ = child.kill(); let _ = child.wait();
            return Err("Document preview timed out.".into());
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(e) => { let _ = child.kill(); let _ = child.wait(); return Err(e.to_string()); }
        }
    };
    let bytes = receiver.recv_timeout(Duration::from_secs(2)).map_err(|_| "Document data read failed")?.map_err(|e| e.to_string())?;
    if !status.success() || bytes.len() as u64 > MAX_XML { return Err("Document data is encrypted, invalid, or exceeds the preview limit.".into()); }
    Ok(bytes)
}

fn entity_text(input: &str, output: &mut String) -> Result<(), String> {
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        output.push_str(&rest[..index]);
        rest = &rest[index + 1..];
        let end = rest.find(';').ok_or("Invalid XML entity")?;
        let entity = &rest[..end];
        let character = match entity {
            "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'',
            s if s.starts_with("#x") => char::from_u32(u32::from_str_radix(&s[2..], 16).map_err(|_| "Invalid XML entity")?).ok_or("Invalid XML character")?,
            s if s.starts_with('#') => char::from_u32(s[1..].parse().map_err(|_| "Invalid XML entity")?).ok_or("Invalid XML character")?,
            _ => return Err("Unsupported XML entity".into()),
        };
        if character == '\0' || (character.is_control() && !matches!(character, '\t' | '\n' | '\r')) { return Err("Invalid control character in XML".into()); }
        output.push(character);
        rest = &rest[end + 1..];
        if output.len() > MAX_OUTPUT { return Err("Document preview exceeds the text limit".into()); }
    }
    output.push_str(rest);
    if output.len() > MAX_OUTPUT { return Err("Document preview exceeds the text limit".into()); }
    Ok(())
}

// A small non-validating XML text reader. It does not expand external entities,
// parse a DTD, execute macros, or interpret embedded objects as code.
fn xml_text(bytes: &[u8], odf: bool) -> Result<String, String> {
    let xml = std::str::from_utf8(bytes).map_err(|_| "Only UTF-8 document XML is supported")?;
    let mut out = String::new();
    let mut pos = 0;
    let mut in_text = false;
    let mut odf_depth = 0usize;
    while pos < xml.len() {
        let tail = &xml[pos..];
        if !tail.starts_with('<') {
            let end = tail.find('<').unwrap_or(tail.len());
            if in_text || odf_depth > 0 { entity_text(&tail[..end], &mut out)?; }
            pos += end;
            continue;
        }
        if tail.starts_with("<!--") {
            pos += tail.find("-->").ok_or("Invalid XML comment")? + 3;
            continue;
        }
        if tail.starts_with("<![CDATA[") {
            let end = tail.find("]]>").ok_or("Invalid XML CDATA")?;
            if in_text || odf_depth > 0 { out.push_str(&tail[9..end]); }
            pos += end + 3;
            continue;
        }
        if tail.starts_with("<!") { return Err("XML declarations and DTDs are not supported".into()); }
        let mut quote = None;
        let mut end = None;
        for (i, ch) in tail.char_indices().skip(1) {
            if let Some(q) = quote { if ch == q { quote = None; } }
            else if ch == '\'' || ch == '"' { quote = Some(ch); }
            else if ch == '>' { end = Some(i); break; }
        }
        let end = end.ok_or("Unterminated XML tag")?;
        let body = tail[1..end].trim();
        pos += end + 1;
        if body.starts_with('?') { continue; }
        let closing = body.starts_with('/');
        let empty = body.ends_with('/');
        let name = body.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if odf {
            if matches!(name, "text:p" | "text:h") {
                if closing { odf_depth = odf_depth.saturating_sub(1); out.push('\n'); }
                else if !empty { odf_depth = odf_depth.saturating_add(1); }
            } else if odf_depth > 0 && !closing && name == "text:tab" { out.push('\t'); }
            else if odf_depth > 0 && !closing && name == "text:line-break" { out.push('\n'); }
            else if odf_depth > 0 && !closing && name == "text:s" { out.push(' '); }
        } else {
            if matches!(name, "w:t" | "a:t") { in_text = !closing && !empty; }
            if closing && matches!(name, "w:p" | "a:p" | "w:tr") { out.push('\n'); }
            if closing && name == "w:tc" { out.push('\t'); }
            if !closing && name == "w:tab" { out.push('\t'); }
            if !closing && matches!(name, "w:br" | "a:br") { out.push('\n'); }
        }
        if out.len() > MAX_OUTPUT { return Err("Document preview exceeds the text limit".into()); }
    }
    Ok(out)
}

/// Best-effort plain-text Office preview. This is not document layout rendering.
pub fn preview(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 64 * 1024 * 1024 { return Err("Select a regular document no larger than 64 MiB.".into()); }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return Err("Document must not be a reparse point.".into()); }
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let deadline = Instant::now() + Duration::from_secs(30);
    let lines = archive_engine::run_engine_with_timeout(path, false, cancel, Duration::from_secs(30))?;
    let names: Vec<(String, String)> = lines.iter().filter_map(|l| l.strip_prefix("Path = ").map(|raw| (raw.to_owned(), raw.replace('\\', "/")))).collect();
    let unique: HashSet<_> = names.iter().map(|(_, normalized)| normalized.to_ascii_lowercase()).collect();
    if unique.len() != names.len() { return Err("Document contains duplicate members.".into()); }
    let mut items: Vec<(usize, String)> = match ext.as_str() {
        "docx" => names.iter().filter(|(_, n)| n == "word/document.xml").map(|(raw, _)| (0, raw.clone())).collect(),
        "odt" => names.iter().filter(|(_, n)| n == "content.xml").map(|(raw, _)| (0, raw.clone())).collect(),
        "pptx" => names.iter().filter_map(|(raw, n)| {
            let number = n.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse::<usize>().ok()?;
            if number == 0 { None } else { Some((number, raw.clone())) }
        }).collect(),
        "xlsx" => names.iter().filter_map(|(raw, n)| {
            let number = n.strip_prefix("xl/worksheets/sheet")?.strip_suffix(".xml")?.parse::<usize>().ok()?;
            if number == 0 { None } else { Some((number, raw.clone())) }
        }).collect(),
        _ => return Err("Unsupported Office format".into()),
    };
    if items.is_empty() || items.len() > 64 { return Err("Document has no supported content or exceeds the 64-slide/sheet preview limit.".into()); }
    items.sort_by_key(|x| x.0);
    let mut result = String::from("TEXT-ONLY PREVIEW — layout, images and formatting are not rendered.\n\n");
    let shared = if ext == "xlsx" {
        match names.iter().find(|(_, n)| n == "xl/sharedStrings.xml") {
            Some((raw, _)) => shared_strings(&member_bytes(path, raw, deadline, cancel)?)?,
            None => Vec::new(),
        }
    } else { Vec::new() };
    for (number, name) in items {
        if ext == "pptx" { result.push_str(&format!("--- Slide {number} ---\n")); }
        if ext == "xlsx" { result.push_str(&format!("--- Sheet {number} ---\n")); }
        let content = member_bytes(path, &name, deadline, cancel)?;
        result.push_str(&if ext == "xlsx" { sheet_text(&content, &shared)? } else { xml_text(&content, ext == "odt")? });
        result.push('\n');
        if result.len() > MAX_OUTPUT { return Err("Document preview exceeds the text limit.".into()); }
    }
    let after = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() != after.len() || meta.modified().ok() != after.modified().ok() { return Err("Document changed during preview; try again.".into()); }
    Ok(result)
}

// Minimal bounded SpreadsheetML support for cell values. This is deliberately
// text-only and does not evaluate formulas or reproduce workbook layout.
enum XmlEvent<'a> { Start(&'a str, &'a str, bool), End(&'a str), Text(&'a str) }
fn xml_events<F: FnMut(XmlEvent<'_>) -> Result<(), String>>(bytes: &[u8], mut visit: F) -> Result<(), String> {
    let xml = std::str::from_utf8(bytes).map_err(|_| "Only UTF-8 document XML is supported")?;
    let mut pos = 0;
    while pos < xml.len() {
        let tail = &xml[pos..];
        if !tail.starts_with('<') {
            let end = tail.find('<').unwrap_or(tail.len());
            visit(XmlEvent::Text(&tail[..end]))?;
            pos += end;
        } else if tail.starts_with("<!--") {
            pos += tail.find("-->").ok_or("Invalid XML comment")? + 3;
        } else if tail.starts_with("<![CDATA[") {
            // CDATA text is not entity-escaped; reject it here rather than
            // silently interpreting a literal ampersand as an entity.
            return Err("CDATA is not supported in spreadsheet preview".into());
        } else if tail.starts_with("<!") {
            return Err("XML declarations and DTDs are not supported".into());
        } else {
            let mut quote = None;
            let mut end = None;
            for (i, ch) in tail.char_indices().skip(1) {
                if let Some(q) = quote { if ch == q { quote = None; } }
                else if ch == '\'' || ch == '"' { quote = Some(ch); }
                else if ch == '>' { end = Some(i); break; }
            }
            let end = end.ok_or("Unterminated XML tag")?;
            let body = tail[1..end].trim();
            pos += end + 1;
            if body.starts_with('?') { continue; }
            let closing = body.starts_with('/');
            let empty = body.ends_with('/');
            let name = body.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            let local = name.rsplit(':').next().unwrap_or(name);
            if closing { visit(XmlEvent::End(local))?; }
            else { visit(XmlEvent::Start(local, body, empty))?; }
        }
    }
    Ok(())
}
fn attr<'a>(tag: &'a str, wanted: &str) -> Option<&'a str> {
    // Common SpreadsheetML attributes (e.g. t="s", r="A1"). Unusual
    // whitespace around '=' is not supported by this limited preview.
    for item in tag.split_whitespace().skip(1) {
        let (name, value) = item.split_once('=')?;
        if name == wanted {
            let value = value.trim_end_matches('/');
            if value.len() >= 2 && (value.starts_with('"') && value.ends_with('"') || value.starts_with('\'') && value.ends_with('\'')) {
                return Some(&value[1..value.len()-1]);
            }
        }
    }
    None
}
fn shared_strings(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    let mut in_text = false;
    let mut total = 0usize;
    xml_events(bytes, |event| {
        match event {
            XmlEvent::Start("si", _, empty) => {
                in_item = !empty; current.clear();
                if empty {
                    if result.len() >= 100_000 { return Err("Spreadsheet shared strings exceed the preview limit".into()); }
                    result.push(String::new());
                }
            }
            XmlEvent::Start("t", _, empty) if in_item => in_text = !empty,
            XmlEvent::End("t") => in_text = false,
            XmlEvent::Text(value) if in_item && in_text => entity_text(value, &mut current)?,
            XmlEvent::End("si") => {
                total = total.checked_add(current.len()).ok_or("Spreadsheet string overflow")?;
                if total > MAX_OUTPUT || result.len() >= 100_000 { return Err("Spreadsheet shared strings exceed the preview limit".into()); }
                result.push(std::mem::take(&mut current));
                in_item = false;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(result)
}
fn sheet_text(bytes: &[u8], shared: &[String]) -> Result<String, String> {
    let mut out = String::new();
    let mut value = String::new();
    let mut cell_type = String::new();
    let mut in_cell = false;
    let mut in_value = false;
    let mut column = 0usize;
    let mut next_column = 0usize;
    let mut rows = 0usize;
    xml_events(bytes, |event| {
        match event {
            XmlEvent::Start("row", _, _) => { column = 0; rows += 1; if rows > 10_000 { return Err("Spreadsheet exceeds the 10000-row preview limit".into()); } }
            XmlEvent::Start("c", tag, empty) => {
                in_cell = !empty; value.clear(); cell_type.clear();
                if let Some(t) = attr(tag, "t") { cell_type.push_str(t); }
                next_column = attr(tag, "r").and_then(|r| {
                    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                    if letters.is_empty() { return None; }
                    let mut n = 0usize;
                    for ch in letters.bytes() { n = n.checked_mul(26)?.checked_add((ch.to_ascii_uppercase() - b'A' + 1) as usize)?; }
                    n.checked_sub(1)
                }).unwrap_or(column);
                if next_column > 255 { return Err("Spreadsheet exceeds the 256-column preview limit".into()); }
            }
            XmlEvent::Start("v" | "t", _, empty) if in_cell => in_value = !empty,
            XmlEvent::End("v" | "t") => in_value = false,
            XmlEvent::Text(text) if in_cell && in_value => entity_text(text, &mut value)?,
            XmlEvent::End("c") if in_cell => {
                if next_column < column { return Err("Spreadsheet cells are out of order".into()); }
                if column > 0 { out.push('\t'); }
                while column < next_column { out.push('\t'); column += 1; }
                if cell_type == "s" {
                    let index: usize = value.parse().map_err(|_| "Invalid shared-string index")?;
                    out.push_str(shared.get(index).ok_or("Missing shared-string value")?);
                } else { out.push_str(&value); }
                column += 1; in_cell = false;
                if out.len() > MAX_OUTPUT { return Err("Spreadsheet preview exceeds the text limit".into()); }
            }
            XmlEvent::End("row") => out.push('\n'),
            _ => {}
        }
        Ok(())
    })?;
    Ok(out)
}
