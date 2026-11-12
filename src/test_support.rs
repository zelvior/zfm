//! Scratch directories for the unit tests.
//!
//! The name combines the process id, a process-wide counter and the clock, so
//! tests running in parallel never share a folder, even on systems whose clock
//! has a coarse resolution.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a new, empty folder below the system temporary directory.
pub(crate) fn scratch_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "{prefix}-{}-{}-{nanos}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).expect("create a scratch directory");
    path
}
