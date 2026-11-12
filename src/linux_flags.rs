//! Linux open flags vary across architectures; only these audited targets are
//! supported for no-follow operations. In particular arm64 differs from x86.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const O_NOFOLLOW: i32 = 0x20000;
#[cfg(target_arch = "aarch64")]
pub const O_NOFOLLOW: i32 = 0x8000;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const O_DIRECTORY: i32 = 0x10000;
#[cfg(target_arch = "aarch64")]
pub const O_DIRECTORY: i32 = 0x4000;
