//! Generate a Win32 VERSIONINFO resource without requiring the Windows SDK's rc.exe.
//! The .res format is accepted directly by the MSVC linker and can be converted
//! to COFF by MinGW windres. All fields are explicitly little-endian.
use std::convert::TryFrom;

fn word(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn dword(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn wide(out: &mut Vec<u8>, text: &str) {
    for c in text.encode_utf16().chain(std::iter::once(0)) {
        word(out, c);
    }
}
fn align(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
fn block(key: &str, kind: u16, value: &[u8], value_len: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0];
    word(&mut out, value_len);
    word(&mut out, kind);
    wide(&mut out, key);
    align(&mut out);
    out.extend_from_slice(value);
    for child in children {
        align(&mut out);
        out.extend_from_slice(child);
    }
    let len = u16::try_from(out.len()).expect("version resource is too large");
    out[..2].copy_from_slice(&len.to_le_bytes());
    out
}
fn string(key: &str, value: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    wide(&mut bytes, value);
    block(
        key,
        1,
        &bytes,
        u16::try_from(bytes.len() / 2).expect("version string is too long"),
        &[],
    )
}
fn version_info(version: &str) -> Vec<u8> {
    // Numeric fields use the first three release-tag components; the full
    // release tag (including any prerelease suffix) is retained in the strings.
    let parts: Vec<u16> = version
        .trim_start_matches('v')
        .split(|c| c == '.' || c == '-' || c == '+')
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let number = |i: usize| u32::from(*parts.get(i).unwrap_or(&0));
    let ms = number(0) << 16 | number(1);
    let ls = number(2) << 16;
    let mut fixed = Vec::new();
    for n in [
        0xfeef04bd, 0x00010000, ms, ls, ms, ls, 0x3f, 0, 0x00040004, 1, 0, 0, 0,
    ] {
        dword(&mut fixed, n);
    }
    let strings = [
        ("CompanyName", "Zelvior Labs"),
        ("FileDescription", "ZFM Setup"),
        ("FileVersion", version),
        ("InternalName", "zfm-setup"),
        ("OriginalFilename", "zfm-setup.exe"),
        ("ProductName", "ZFM - Zelvior File Manager"),
        ("ProductVersion", version),
    ]
    .iter()
    .map(|(k, v)| string(k, v))
    .collect::<Vec<_>>();
    let table = block("040904b0", 1, &[], 0, &strings);
    let string_info = block("StringFileInfo", 1, &[], 0, &[table]);
    let var = block("Translation", 0, &[0x09, 0x04, 0xb0, 0x04], 4, &[]);
    let var_info = block("VarFileInfo", 1, &[], 0, &[var]);
    block(
        "VS_VERSION_INFO",
        0,
        &fixed,
        fixed.len() as u16,
        &[string_info, var_info],
    )
}
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, lang: u16, data: &[u8]) {
    align(out);
    dword(out, u32::try_from(data.len()).expect("resource too large"));
    dword(out, 32);
    word(out, 0xffff);
    word(out, kind);
    word(out, 0xffff);
    word(out, name);
    dword(out, 0);
    word(out, if kind == 0 { 0 } else { 0x0030 });
    word(out, lang);
    dword(out, 0);
    dword(out, 0);
    out.extend_from_slice(data);
    align(out);
}
pub fn resources(version: &str, manifest: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    // The resource file starts with the conventional null resource header.
    entry(&mut out, 0, 0, 0, &[]);
    entry(&mut out, 16, 1, 0x0409, &version_info(version));
    if let Some(xml) = manifest {
        entry(&mut out, 24, 1, 0x0409, xml);
    }
    out
}
