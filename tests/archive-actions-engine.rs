//! Optional round-trip coverage using the bundled engine next to the test executable.
//! Build it there first and run `cargo test --test archive-actions-engine -- --ignored`.
#![cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
use std::{fs, sync::atomic::AtomicBool};
use zfm::{archive_actions::Format, archive_engine};

#[test]
#[ignore = "requires the bundled archive engine alongside the test executable"]
fn format_actions_round_trip_without_overwriting() {
    let root = std::env::temp_dir().join(format!("zfm-archive-actions-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("input.txt");
    fs::write(&source, b"archive round trip\n").unwrap();
    let cancel = AtomicBool::new(false);
    for format in Format::ALL {
        for encrypted in [false, true] {
            if encrypted && !format.encrypted() { continue; }
            let archive = root.join(format!("archive-{}{}", encrypted, format.suffix()));
            let output = root.join(format!("output-{}-{}", encrypted, format.suffix().replace('.', "_")));
            fs::create_dir(&output).unwrap();
            let password = if encrypted { Some("testpass") } else { None };
            assert!(archive_engine::supports_creation(&archive, encrypted));
            archive_engine::create_items_password(&[source.clone()], &archive, &cancel, password).unwrap();
            assert_eq!(archive_engine::extract_all_linux(&archive, &output, &cancel, password).unwrap(), 1);
            let name = if format.single_file() { archive.file_stem().unwrap() } else { source.file_name().unwrap() };
            assert_eq!(fs::read(output.join(name)).unwrap(), b"archive round trip\n");
            assert!(archive_engine::extract_all_linux(&archive, &output, &cancel, password).is_err());
        }
    }
    fs::remove_dir_all(root).unwrap();
}
