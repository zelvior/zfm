# ZFM — Zelvior File Manager

A small native file manager written in Rust. **This repository currently contains an early Phase 1 foundation, not the complete file manager described in the project brief.** It is not yet recommended as a replacement for an established file manager.

## What works in this foundation

- Linux GTK 3 window with a black-and-white theme, directory list, sidebar, and status line.
- Enter a path and press Enter; double-click a folder to enter it or a file to open it with `xdg-open`. Back, forward, parent and refresh buttons; F5, Alt+Left/Right/Up and Ctrl+L shortcuts.
- Hidden-file toggle, folder-first case-insensitive name ordering, file sizes and modification dates. Broken links remain visible. Errors opening a directory are shown in the status bar.
- Directory enumeration and metadata run on a worker. New navigation cancels the *results* of earlier work; UI rows are added in batches on the GTK main loop. There is no persistent background service.
- A built-in, offline CSV → JSON converter is available from the toolbar, sidebar, and the context menu on CSV files. The converter runs on a worker, reports bytes processed, and supports cancellation. Selecting a CSV first pre-fills the paths.
- No third-party Rust crates. GTK 3 is loaded at runtime through a small platform adapter; filesystem and navigation code are separate.

Sidebar locations currently use conventional subdirectories of `$HOME`, **not** customized XDG user directories. The interface uses a single selection and a single tab. The list is batched but not virtualized: very large directories still consume memory for all entries and widgets. Directory sorting happens after enumeration; a slow network filesystem call cannot be interrupted until it returns. Displayed non-UTF-8 filenames are lossily decoded, although double-clicking uses the original path. Entering a non-UTF-8 path in the address bar is not supported. File opening depends on `xdg-open` and an installed application for the file type.

## Conversion and archives: current scope

**Built in:** UTF-8 CSV → JSON only. The first row supplies unique, non-empty object keys; every value is a JSON string (no inference). Quoted fields, escaped quotes, embedded newlines and a UTF-8 BOM are supported. Every row must have the same number of fields. To bound memory use, records are limited to 8 MiB and 4,096 fields. The input is streamed; output is staged in the destination directory and published with a no-replace hard link. Existing files, including symlinks, are never intentionally overwritten. The destination filesystem must support hard links; otherwise conversion fails. Cancellation is cooperative and cannot interrupt a blocked filesystem call; it may be too late once publication has begun. Normal errors and cancellations remove the staging file; a process crash or forced termination may leave a `.zfm-*.tmp` file.

**Optional external tools:** none integrated. **Unsupported:** all other conversions in the brief, batch conversion and all archive management/extraction/creation. In particular, no archive is extracted by ZFM and no archive security or password support is claimed. Do not use this build as an archive manager. The converter's GTK controls, file chooser and context menu have not been exercised interactively in this environment. File chooser paths are displayed as UTF-8, so non-UTF-8 paths cannot be used reliably in this interface.

## Status and compatibility

| Target | Status |
| --- | --- |
| Linux x86_64 | Core tests and release build passed on Debian 12 with Rust 1.85. GTK window **not tested** in the available environment (no GTK runtime or display). |
| Linux Mint 19 / Ubuntu 18.04, x86 or x86_64 | Intended target, **not verified**. GTK 3.22+ and Rust 1.70 are the baseline chosen for this foundation; actual package, toolchain and GUI compatibility still require testing on each system. |
| Windows x86 / x86_64 | Shared core is portable in design; there is **no Windows GUI yet**. The executable reports this and exits. Windows 7 support is not claimed. |

A Rust build produced on a newer distribution can require a newer glibc than Mint 19 provides. Build on the oldest supported distribution, or in a compatible build environment, and test the actual artifact on the target. Architecture support is not guaranteed merely by installing a Rust target.

## Linux build and run

On Linux Mint 19 / Ubuntu 18.04, install system prerequisites (package availability and 32-bit multiarch should be verified on the target):

```sh
sudo apt update
sudo apt install build-essential pkg-config libgtk-3-dev libgtk-3-0 xdg-utils
```

Install a Rust toolchain that supports the host, e.g. Rust 1.70 using rustup. Rust is needed to **build**, not to run a packaged executable. We specify Rust 1.70 as the intended minimum; it has not been verified here. GTK headers are useful for development but this runtime-loaded adapter only needs the GTK 3 shared library when running. Start from a checkout:

```sh
cargo test --locked
cargo build --locked
cargo run --locked -- /path/to/folder
cargo build --release --locked
./target/release/zfm  # defaults to $HOME
```

Explicit 64-bit build, on a compatible host:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo build --release --locked --target x86_64-unknown-linux-gnu
```

For a **32-bit** Linux build, install a working i686 compiler/linker and matching 32-bit libc development files, then:

```sh
rustup target add i686-unknown-linux-gnu
cargo build --release --locked --target i686-unknown-linux-gnu
```

For example, on a 64-bit Debian-family host, `gcc-multilib` and `libc6-dev-i386` may supply the cross-linker and C library. The target machine also needs the **32-bit** GTK 3 runtime and its dependencies. Neither 32-bit setup nor Mint 19 has been tested here.

Create a simple tarball after a native release build:

```sh
./scripts/package-linux.sh
```

This contains the executable, README and license, not GTK or `xdg-open`. Recipients need compatible system runtime libraries and a graphical session; no Rust installation is required. See [Linux notes](docs/linux.md).

## Windows

There is currently no Windows interface or release package. Building the shared core on a Windows Rust toolchain is useful for development, but does not produce a usable Windows file manager. See [Windows notes](docs/windows.md) for target commands and constraints.

## Development roadmap

Phase 1 needs interactive GTK testing and compatibility verification before moving on. File operations, properties, search, tabs, bookmarks, recent files, multiple selection, general-purpose context menus, drag-and-drop, alternative views and advanced features are **not implemented**. In particular, there is no delete button or shortcut: no destructive operation is silently substituted. Future phases should implement and test core operations before adding advanced tools, and profile real workloads before making performance claims.

Licensed under [Apache-2.0](LICENSE). Made by Zelvior.
