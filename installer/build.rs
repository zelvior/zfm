//! Prepare the embedded installer payload and the build metadata.
//!
//! `ZFM_APP_BINARY` must name the release `zfm.exe` for the target
//! architecture; `ZFM_VERSION` carries the release tag. When the executable
//! has not been built the payload is a placeholder, so `cargo test` and
//! `cargo check` still work on any host.

#[path = "resources/version.rs"]
mod version_resource;

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let root = manifest
        .parent()
        .expect("installer lives in the repository")
        .to_path_buf();
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

    // Preserve the pinned Rust distribution's upstream notices in the
    // installed README without adding new managed paths to existing installs.
    for (name, env_key) in [
        ("COPYRIGHT", "ZFM_RUST_COPYRIGHT_PATH"),
        ("LICENSE-MIT", "ZFM_RUST_MIT_PATH"),
        ("LICENSE-APACHE", "ZFM_RUST_APACHE_PATH"),
    ] {
        let path = root.join("LICENSES/rust-1.77.2").join(name);
        if !path.is_file() {
            panic!("missing upstream Rust notice: {}", path.display());
        }
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rustc-env={}={}", env_key, path.display());
    }

    let version = env::var("ZFM_VERSION").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=ZFM_VERSION={}", version);

    let manifest = root.join("installer/resources/setup.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed=resources/version.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let target = env::var("TARGET").expect("target");
        let is_msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
        let xml = fs::read(&manifest).expect("read setup manifest");
        let res = out.join("setup.res");
        fs::write(
            &res,
            version_resource::resources(&version, if is_msvc { None } else { Some(&xml) }),
        )
        .expect("write setup resources");
        if is_msvc {
            // Let LINK/mt merge the explicit manifest with the default CRT manifest.
            println!("cargo:rustc-link-arg-bin=zfm-setup=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg-bin=zfm-setup=/MANIFESTINPUT:{}",
                manifest.display()
            );
            println!("cargo:rustc-link-arg-bin=zfm-setup={}", res.display());
        } else if target.ends_with("-windows-gnu") {
            // GNU ld cannot consume a .res file directly; convert using the
            // matching MinGW resource compiler installed with the linker.
            let windres = if target.starts_with("i686-") {
                "i686-w64-mingw32-windres"
            } else {
                "x86_64-w64-mingw32-windres"
            };
            let object = out.join("setup-res.o");
            let status = std::process::Command::new(windres)
                .args(["-J", "res", "-O", "coff", "-i"])
                .arg(&res)
                .arg("-o")
                .arg(&object)
                .status()
                .expect("install the matching MinGW windres to build Windows GNU setup");
            assert!(
                status.success(),
                "windres could not compile setup resources"
            );
            println!("cargo:rustc-link-arg-bin=zfm-setup={}", object.display());
        } else {
            panic!("unsupported Windows installer target: {}", target);
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
