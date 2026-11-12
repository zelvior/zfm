//! Prepare the embedded installer payload and the build metadata.
//!
//! `ZFM_APP_BINARY` must name the release `zfm.exe` for the target
//! architecture; `ZFM_VERSION` carries the release tag. When the executable
//! has not been built the payload is a placeholder, so `cargo test` and
//! `cargo check` still work on any host.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let root = manifest.parent().expect("installer lives in the repository").to_path_buf();
    let out = PathBuf::from(env::var("OUT_DIR").expect("output directory"));

    println!("cargo:rerun-if-env-changed=ZFM_APP_BINARY");
    println!("cargo:rerun-if-env-changed=ZFM_VERSION");

    let payload = out.join("zfm-app.bin");
    match env::var_os("ZFM_APP_BINARY") {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_file() {
                panic!(
                    "ZFM_APP_BINARY does not name an existing file: {}",
                    path.display()
                );
            }
            println!("cargo:rerun-if-changed={}", path.display());
            fs::copy(&path, &payload).expect("could not copy the ZFM executable");
        }
        None => {
            fs::write(
                &payload,
                b"ZFM payload placeholder. Build zfm.exe and set ZFM_APP_BINARY.\n",
            )
            .expect("could not write the payload placeholder");
        }
    }
    println!("cargo:rustc-env=ZFM_PAYLOAD_PATH={}", payload.display());

    // The setup wizard embeds the same monochrome icon and logo as the
    // application so the installer window and its shortcuts look identical.
    let icon = root.join("packaging").join("zfm.ico");
    let logo = root.join("packaging").join("zfm-64.png");
    for path in [&icon, &logo] {
        if !path.is_file() {
            panic!("missing packaging asset: {}", path.display());
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rustc-env=ZFM_ICON_PATH={}", icon.display());
    println!("cargo:rustc-env=ZFM_LOGO_PATH={}", logo.display());

    for name in ["LICENSE", "README.md"] {
        let path = root.join(name);
        if !path.is_file() {
            panic!("missing packaging asset: {}", path.display());
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rustc-env=ZFM_LICENSE_PATH={}",
        root.join("LICENSE").display()
    );
    println!(
        "cargo:rustc-env=ZFM_README_PATH={}",
        root.join("README.md").display()
    );

    let version = env::var("ZFM_VERSION").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=ZFM_VERSION={}", version);
    println!("cargo:rerun-if-changed=build.rs");
}
