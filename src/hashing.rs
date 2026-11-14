//! Streaming integrity checks using OS cryptographic implementations.
//! MD5 is retained for legacy interoperability, not security decisions.
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, PartialEq, Eq)]
pub struct Hashes { pub md5: String, pub sha256: String, pub bytes: u64 }
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, message) }
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// Hash a regular file with bounded memory. Refuses a changed size or mtime.
/// The final component must not be a link; hostile concurrent parent directory
/// replacement is not prevented. Cancellation is checked between reads.
pub fn hash_file(path: &Path, cancel: &AtomicBool) -> io::Result<Hashes> {
    let mut file = crate::settings::open_existing(path)?;
    let before = file.metadata()?;
    let mut engine = imp::Engine::new()?;
    let mut buf = [0u8; 64 * 1024]; let mut bytes = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "Hashing cancelled")); }
        let count = file.read(&mut buf)?;
        if count == 0 { break; }
        engine.update(&buf[..count])?; bytes = bytes.checked_add(count as u64).ok_or_else(|| invalid("File is too large"))?;
    }
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? || bytes != after.len() {
        return Err(invalid("File changed while hashing; retry"));
    }
    let (md5, sha256) = engine.finish()?;
    Ok(Hashes { md5: hex(&md5), sha256: hex(&sha256), bytes })
}
/// Accepts exactly one bare MD5 or SHA-256 hexadecimal digest, case-insensitive.
pub fn verify(hashes: &Hashes, expected: &str) -> io::Result<bool> {
    let expected = expected.trim();
    if !expected.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(invalid("Expected a hexadecimal MD5 or SHA-256 digest")); }
    let actual = match expected.len() { 32 => &hashes.md5, 64 => &hashes.sha256, _ => return Err(invalid("Expected 32 MD5 or 64 SHA-256 hexadecimal characters")) };
    Ok(actual.eq_ignore_ascii_case(expected))
}
pub fn report(path: &Path, expected: Option<&str>, cancel: &AtomicBool) -> io::Result<String> {
    if let Some(value) = expected {
        let value = value.trim();
        if !matches!(value.len(), 32 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("Expected 32 MD5 or 64 SHA-256 hexadecimal characters"));
        }
    }
    let hashes = hash_file(path, cancel)?;
    let result = match expected { Some(value) => if verify(&hashes, value)? { "Verification: MATCH\n\n" } else { "Verification: MISMATCH\n\n" }, None => "" };
    Ok(format!("{result}File: {:?}\nBytes: {}\n\nSHA-256: {}\nMD5 (legacy, not collision-resistant): {}\n\nA matching hash alone does not establish authenticity. Concurrent changes can be missed.", path, hashes.bytes, hashes.sha256, hashes.md5))
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::ptr;
    type P = *mut c_void;
    #[link(name = "dl")]
    extern "C" { fn dlopen(name: *const c_char, flags: c_int) -> P; fn dlsym(handle: P, name: *const c_char) -> P; fn dlclose(handle: P) -> c_int; }
    struct Api { lib: P, new: unsafe extern "C" fn() -> P, free: unsafe extern "C" fn(P), init: unsafe extern "C" fn(P, P, P) -> c_int,
        update: unsafe extern "C" fn(P, *const c_void, usize) -> c_int, finish: unsafe extern "C" fn(P, *mut u8, *mut u32) -> c_int,
        md5: unsafe extern "C" fn() -> P, sha256: unsafe extern "C" fn() -> P }
    impl Drop for Api { fn drop(&mut self) { unsafe { dlclose(self.lib); } } }
    impl Api {
        unsafe fn load() -> io::Result<Self> {
            let mut lib = ptr::null_mut();
            for name in ["libcrypto.so.3", "libcrypto.so.1.1", "libcrypto.so"] {
                lib = dlopen(CString::new(name).unwrap().as_ptr(), 2);
                if !lib.is_null() { break; }
            }
            if lib.is_null() { return Err(io::Error::new(io::ErrorKind::NotFound, "OpenSSL libcrypto 1.1 or 3 is required for hashing")); }
            macro_rules! symbol { ($name:literal, $ty:ty) => {{
                let pointer = dlsym(lib, concat!($name, "\0").as_ptr() as *const c_char);
                if pointer.is_null() { dlclose(lib); return Err(invalid("Installed libcrypto lacks required hashing functions")); }
                std::mem::transmute::<P, $ty>(pointer)
            }} }
            Ok(Self { lib,
                new: symbol!("EVP_MD_CTX_new", unsafe extern "C" fn() -> P),
                free: symbol!("EVP_MD_CTX_free", unsafe extern "C" fn(P)),
                init: symbol!("EVP_DigestInit_ex", unsafe extern "C" fn(P, P, P) -> c_int),
                update: symbol!("EVP_DigestUpdate", unsafe extern "C" fn(P, *const c_void, usize) -> c_int),
                finish: symbol!("EVP_DigestFinal_ex", unsafe extern "C" fn(P, *mut u8, *mut u32) -> c_int),
                md5: symbol!("EVP_md5", unsafe extern "C" fn() -> P),
                sha256: symbol!("EVP_sha256", unsafe extern "C" fn() -> P) })
        }
    }
    pub struct Engine { api: Api, md5: P, sha: P }
    impl Drop for Engine { fn drop(&mut self) { unsafe { if !self.md5.is_null() { (self.api.free)(self.md5); } if !self.sha.is_null() { (self.api.free)(self.sha); } } } }
    impl Engine {
        pub fn new() -> io::Result<Self> { unsafe {
            let api = Api::load()?; let md5 = (api.new)(); let sha = (api.new)();
            let engine = Self { api, md5, sha };
            if md5.is_null() || sha.is_null() || (engine.api.init)(md5, (engine.api.md5)(), ptr::null_mut()) != 1 || (engine.api.init)(sha, (engine.api.sha256)(), ptr::null_mut()) != 1 {
                return Err(invalid("Could not initialize MD5 and SHA-256; system crypto policy may disable MD5"));
            }
            Ok(engine)
        } }
        pub fn update(&mut self, bytes: &[u8]) -> io::Result<()> { unsafe {
            if (self.api.update)(self.md5, bytes.as_ptr() as P, bytes.len()) != 1 || (self.api.update)(self.sha, bytes.as_ptr() as P, bytes.len()) != 1 { return Err(invalid("Hash update failed")); } Ok(())
        } }
        pub fn finish(&mut self) -> io::Result<([u8; 16], [u8; 32])> { unsafe {
            let mut md5 = [0; 16]; let mut sha = [0; 32]; let mut m = 0u32; let mut s = 0u32;
            if (self.api.finish)(self.md5, md5.as_mut_ptr(), &mut m) != 1 || (self.api.finish)(self.sha, sha.as_mut_ptr(), &mut s) != 1 || m != 16 || s != 32 { return Err(invalid("Hash finalization failed")); }
            Ok((md5, sha))
        } }
    }
}
#[cfg(windows)]
mod imp {
    use super::*;
    type H = usize;
    #[link(name = "advapi32")]
    extern "system" {
        fn CryptAcquireContextW(provider: *mut H, container: *const u16, name: *const u16, kind: u32, flags: u32) -> i32;
        fn CryptReleaseContext(provider: H, flags: u32) -> i32;
        fn CryptCreateHash(provider: H, alg: u32, key: H, flags: u32, hash: *mut H) -> i32;
        fn CryptHashData(hash: H, data: *const u8, length: u32, flags: u32) -> i32;
        fn CryptGetHashParam(hash: H, param: u32, data: *mut u8, length: *mut u32, flags: u32) -> i32;
        fn CryptDestroyHash(hash: H) -> i32;
    }
    pub struct Engine { provider: H, md5: H, sha: H }
    impl Drop for Engine { fn drop(&mut self) { unsafe { if self.md5 != 0 { CryptDestroyHash(self.md5); } if self.sha != 0 { CryptDestroyHash(self.sha); } if self.provider != 0 { CryptReleaseContext(self.provider, 0); } } } }
    impl Engine {
        pub fn new() -> io::Result<Self> { unsafe {
            let mut engine = Self { provider: 0, md5: 0, sha: 0 };
            if CryptAcquireContextW(&mut engine.provider, std::ptr::null(), std::ptr::null(), 24, 0xf000_0000) == 0
                || CryptCreateHash(engine.provider, 0x8003, 0, 0, &mut engine.md5) == 0
                || CryptCreateHash(engine.provider, 0x800c, 0, 0, &mut engine.sha) == 0 { return Err(io::Error::last_os_error()); }
            Ok(engine)
        } }
        pub fn update(&mut self, bytes: &[u8]) -> io::Result<()> { unsafe {
            if CryptHashData(self.md5, bytes.as_ptr(), bytes.len() as u32, 0) == 0 || CryptHashData(self.sha, bytes.as_ptr(), bytes.len() as u32, 0) == 0 { return Err(io::Error::last_os_error()); } Ok(())
        } }
        pub fn finish(&mut self) -> io::Result<([u8; 16], [u8; 32])> { unsafe {
            let mut md5 = [0; 16]; let mut sha = [0; 32]; let mut m = 16u32; let mut s = 32u32;
            if CryptGetHashParam(self.md5, 2, md5.as_mut_ptr(), &mut m, 0) == 0 || CryptGetHashParam(self.sha, 2, sha.as_mut_ptr(), &mut s, 0) == 0 || m != 16 || s != 32 { return Err(io::Error::last_os_error()); }
            Ok((md5, sha))
        } }
    }
}
#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::*;
    pub struct Engine;
    impl Engine {
        pub fn new() -> io::Result<Self> { Err(io::Error::new(io::ErrorKind::Unsupported, "Hashing is unavailable on this platform")) }
        pub fn update(&mut self, _: &[u8]) -> io::Result<()> { unreachable!() }
        pub fn finish(&mut self) -> io::Result<([u8; 16], [u8; 32])> { unreachable!() }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vectors_and_verification() {
        let root = crate::test_support::scratch_dir("hash"); let path = root.join("ü.txt"); std::fs::write(&path, b"abc").unwrap();
        let hashes = hash_file(&path, &AtomicBool::new(false)).unwrap();
        assert_eq!(hashes.md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hashes.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(verify(&hashes, &hashes.sha256.to_uppercase()).unwrap()); assert!(!verify(&hashes, &"0".repeat(64)).unwrap()); assert!(verify(&hashes, "oops").is_err());
        assert_eq!(hash_file(&path, &AtomicBool::new(true)).unwrap_err().kind(), io::ErrorKind::Interrupted);
        std::fs::write(&path, vec![0u8; 1024 * 1024]).unwrap();
        let large = hash_file(&path, &AtomicBool::new(false)).unwrap();
        assert_eq!(large.md5, "b6d81b360a5672d80c27430f39153e2c");
        assert_eq!(large.sha256, "30e14955ebf1352266dc2ff8067e68104607e750abb9d3b36582b8af909fcb58");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test] fn rejects_symlink() {
        let root = crate::test_support::scratch_dir("hash-link"); let file = root.join("file"); let link = root.join("link"); std::fs::write(&file, b"hello").unwrap(); std::os::unix::fs::symlink(file, &link).unwrap();
        assert!(hash_file(&link, &AtomicBool::new(false)).is_err()); std::fs::remove_dir_all(root).unwrap();
    }
}
