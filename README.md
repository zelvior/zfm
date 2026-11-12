# ZFM — Zelvior File Manager

A small native file manager written in Rust. **This repository currently contains an early foundation and a limited set of file operations, not the complete file manager described in the project brief.** It is not yet recommended as a replacement for an established file manager.

## What works in this foundation

- Linux GTK 3 window with a black-and-white theme, directory list, sidebar, and status line.
- Enter a path and press Enter; double-click a folder to enter it or a file to open it with `xdg-open`. Back, forward, parent and refresh buttons; F5, Alt+Left/Right/Up and Ctrl+L shortcuts.
- Hidden-file toggle, folder-first case-insensitive name ordering, file sizes and modification dates. Broken links remain visible. Errors opening a directory are shown in the status bar.
- Directory enumeration and metadata run on a worker. New navigation cancels the *results* of earlier work; UI rows are added in batches on the GTK main loop. There is no persistent background service.
- A current-folder filename filter (including extension substrings), with an optional case-sensitive mode. Press Search or Enter to apply; Ctrl+F focuses it. It is not recursive, and the filter stays active while navigating until cleared.
- Single-item create file/folder, rename, duplicate regular file, copy a regular file to another folder, and move an item to another folder on the same filesystem. These run in a worker and refuse to overwrite. The Trash action uses GIO without a permanent-delete fallback. Permanent deletion of a file, symlink, or **empty** folder requires confirmation. With the file list focused, Delete moves the selection to trash, F2 renames and Shift+Delete requests permanent deletion.
- The archive manager browses and extracts TAR, ZIP, TAR.GZ/TGZ, TAR.BZ2 and TAR.XZ; ZIP and compressed TAR require the system libarchive runtime. It tests ZIP/compressed archives and creates a TAR or ZIP from one regular file. Operations run in the background with cancellation and no-overwrite behavior. See the limitations below before use.
- The offline converter supports CSV → JSON and a restricted JSON → CSV mode, on a worker with progress and cancellation. Select a CSV or JSON file first to pre-fill paths and direction.
- Linux tabs retain independent navigation histories; use Ctrl+T, Ctrl+W, or the folder context menu. File clipboard actions support one local item via GNOME copied-files and URI-list targets (Ctrl+C/X/V with the list focused). Copy is regular-file only; cut uses the same same-filesystem move operation as Move to.
- An initial native Win32 browser implements navigation, directory listing and opening files, without Linux feature parity. It has not yet been linked or run.
- No third-party Rust crates. GTK 3 is loaded at runtime through a small platform adapter; filesystem and navigation code are separate.

Sidebar locations currently use conventional subdirectories of `$HOME`, **not** customized XDG user directories. The file list supports a single selection. The list is batched but not virtualized: very large directories still consume memory for all entries and widgets. Directory sorting happens after enumeration; a slow network filesystem call cannot be interrupted until it returns. Displayed non-UTF-8 filenames are lossily decoded, although double-clicking uses the original path. Entering a non-UTF-8 path in the address bar is not supported. File opening depends on `xdg-open` and an installed application for the file type.

## File-operation limits

The file actions operate on the single selected item. The copy and duplicate actions stream regular files only on Linux x86, x86_64 and aarch64; they do not follow source symlinks, copy folders, or preserve metadata. Copies are created with restrictive permissions, not the source permissions. Copy cancellation is cooperative and may be too late once publication begins. Copies use a temporary file and a no-replace hard link in the destination directory, so filesystems without hard-link support fail rather than overwrite. A forced exit can leave `.zfm-copy-*.tmp` files. Do not use directories writable by untrusted users as staging destinations: the temporary-file publication is not protected against hostile concurrent changes in such directories.

Rename and move use Linux `renameat2(RENAME_NOREPLACE)`, requiring a kernel and filesystem that support it. Move is same-filesystem only; a cross-filesystem move fails and leaves the source in place. Rename and move are currently implemented for Linux x86, x86_64, and aarch64 only. The destination is never intentionally overwritten. Directory deletion is intentionally restricted to empty directories, does not recurse, and deleting a symlink removes the link rather than its target. Permanent deletion cannot be undone. Trash uses GIO and may be unavailable on some filesystems or mounts; failures are reported and never fall back to permanent deletion. Clipboard accepts one local item only; multiple-item or remote payloads are rejected. URI-list-only payloads are treated as copy (not cut), and clipboard contents may not persist after ZFM exits without a clipboard manager. There is no multi-select in the file view, directory copy, or cross-filesystem move. The new GTK controls and destructive-action confirmation have **not** been tested interactively in this environment; use caution and back up data before trying them.

## Conversion and archives: current scope

**Built in:** UTF-8 CSV → JSON and JSON → CSV. JSON → CSV accepts a nonempty array of objects whose nonempty unique keys are identical across objects and whose values are all strings; nested structures, numbers, booleans, null, and an empty array are rejected rather than coerced. It supports JSON Unicode escapes and key reordering. CSV → JSON: The first row supplies unique, non-empty object keys; every value is a JSON string (no inference). Quoted fields, escaped quotes, embedded newlines and a UTF-8 BOM are supported. Every row must have the same number of fields. To bound memory use, records are limited to 8 MiB and 4,096 fields. The input is streamed; output is staged in the destination directory and published with a no-replace hard link. Existing files, including symlinks, are never intentionally overwritten. The destination filesystem must support hard links; otherwise conversion fails. Cancellation is cooperative and cannot interrupt a blocked filesystem call; it may be too late once publication has begun. Normal errors and cancellations remove the staging file; a process crash or forced termination may leave a `.zfm-*.tmp` file.

**Archive support:** uncompressed POSIX ustar and simple GNU TAR, plus ZIP and gzip/bzip2/xz-compressed TAR via dynamically loaded `libarchive.so.13`. A compatible libarchive installation with the relevant built-in codecs is required for ZIP and compressed TAR. Archives may contain regular files and directories only. The Test button reads ZIP/compressed data and asks libarchive to validate available checksums; it cannot prove authenticity. TAR has no content checksum; testing uncompressed TAR validates its structure and header checksums only. Browsing alone does not verify file contents. The entire archive is inspected before extraction; absolute/traversal paths, duplicate names, links, devices and non-UTF-8 names are rejected. Uncompressed TAR also rejects PAX/GNU extension headers. Limits are 10,000 entries, 512 MiB per file and 1 GiB total. Linux extraction is implemented for x86, x86_64 and aarch64 using directory-relative no-follow operations, and never intentionally overwrites files. The destination must already exist and its final component cannot be a symlink. Select individual entries or all entries; selecting a directory alone does **not** select its contents. Destination directories are created with mode 0700 and files with 0600; timestamps, permissions and ownership are not restored. Extract only into directories that untrusted users cannot concurrently modify or move. Cancel or an error removes the current partial file, but previously extracted files and directories remain; a crash may leave a partial file. Cancellation cannot interrupt a blocked filesystem call. Archive contents can be dangerous even when path checks pass; do not run untrusted content.

TAR and ZIP creation are limited to one regular file with a safe UTF-8 basename no longer than 100 bytes, up to 512 MiB. It streams to a temporary file, then uses a no-replace hard link, so the output filesystem must support hard links. It does not preserve source permissions or timestamps. Cancellation normally cleans up the staging file; a crash may leave `.zfm-tar-*.tmp` or `.zfm-zip-*.tmp`. Do not create archives in directories writable by untrusted users because staging publication is not protected against hostile concurrent changes.

**Optional external tools:** none integrated; libarchive is a system library, not an external conversion process. **Unsupported:** all other requested conversions, batch conversion, multi-file or directory archive creation, creation of compressed TAR, standalone compression, passwords, and RAR/7Z. The archive and converter GTK controls and file choosers have not been exercised interactively in this environment. File chooser paths are displayed as UTF-8, so non-UTF-8 paths cannot be used reliably in this interface.

## Status and compatibility

| Target | Status |
| --- | --- |
| Linux x86_64 | Core tests and release build passed on Debian 12 with Rust 1.85. GTK window **not tested** in the available environment (no GTK runtime or display). |
| Linux Mint 19 / Ubuntu 18.04, x86 or x86_64 | Intended target, **not verified**. GTK 3.22+ and Rust 1.70 are the baseline chosen for this foundation; actual package, toolchain and GUI compatibility still require testing on each system. |
| Windows x86 / x86_64 | Initial native browser only; GNU targets type-checked with Rust 1.85 but neither linked nor run. File operations, tabs, clipboard, archives and converter UI are not implemented on Windows. Windows 7 support is not claimed. |

A Rust build produced on a newer distribution can require a newer glibc than Mint 19 provides. Build on the oldest supported distribution, or in a compatible build environment, and test the actual artifact on the target. Architecture support is not guaranteed merely by installing a Rust target.

## Linux build and run

### Automated builds (Linux host)

From a checkout, run:

```sh
./scripts/build-linux.sh
```

The script installs rustup in your user account if needed, installs/updates the **current Rust stable** toolchain, runs native tests, and attempts release builds for 64-bit and 32-bit Linux and Windows GNU. On an ARM64 Linux host it also builds the native ARM64 executable. Each successful executable is placed in a new `artifacts/build-*/<target>/` directory; Windows binaries are named `zfm.exe`. Missing prerequisites or failed targets are listed, successful binaries are retained, and the script exits nonzero if any requested target fails. It needs internet access to `sh.rustup.rs` and Rust's distribution servers. It does not edit your shell startup files.

On a Debian/Ubuntu-family **x86_64** host, to ask the script to install the compiler, multilib, MinGW cross-compilers, GTK 3, libarchive and related prerequisites using `apt-get` (and `sudo` if needed):

```sh
./scripts/build-linux.sh --install-deps
```

This changes system packages; it is never done without `--install-deps`. On other distributions or architectures, install the corresponding native C compiler, 32-bit Linux C toolchain and 32/64-bit MinGW GNU compilers yourself. For a native build only, use `./scripts/build-linux.sh --native` (optionally with `--install-deps` on supported hosts). Run `./scripts/build-linux.sh --help` for all options, including selecting a Rust toolchain version or skipping tests. If the latest Rust no longer runs on an older OS, use a compatible version with `--toolchain VERSION`; the intended minimum is 1.70 but remains unverified. Tests for archive features may be skipped when their libarchive or Python test prerequisites are absent.

To run the native Linux executable, use the full path printed under `Built:` (for example, `./artifacts/build-XXXXXXXX/x86_64-unknown-linux-gnu/zfm`; replace the directory and target with the values printed by your build). The files in `artifacts/` are local build outputs and are not committed to Git.

These are dynamically linked executables, not self-contained installers: Linux needs compatible libc, GTK 3 and `xdg-open`; ZIP and compressed archive features additionally need libarchive. Build on the oldest intended Linux release for the best chance of glibc compatibility. Cross-compiling does not test Windows, 32-bit systems or Windows 7; the Windows GUI remains experimental and incomplete. Back up files before testing.

### Manual build

On Linux Mint 19 / Ubuntu 18.04, install system prerequisites (package availability and 32-bit multiarch should be verified on the target):

```sh
sudo apt update
sudo apt install build-essential pkg-config libgtk-3-dev libgtk-3-0 libarchive13 xdg-utils
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

On Windows with PowerShell 5.1 or later and the [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) installed, open Command Prompt or PowerShell in the checkout and run:

```powershell
.\scripts\build-windows.cmd
```

The script installs rustup in the user account if necessary, installs/updates Rust stable, runs native tests and builds both `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. It places each `zfm.exe` in `artifacts/build-windows-*/<target>/` and exits nonzero if any requested build fails; successful binaries remain available. The launcher bypasses the execution policy only for that PowerShell process; it does not change the machine policy. Rustup requires internet access. Install the **Desktop development with C++** workload, including the Windows SDK and x86/x64 build tools; the script does not install Visual Studio automatically. A 32-bit host also needs the x64 cross-build tools to build both targets.

Use `-NativeOnly` for the host Windows architecture, `-Toolchain 1.85.0` (for example) to choose a Rustup toolchain instead of latest stable, or `-SkipTests` to skip tests. These can be passed to the `.cmd` launcher. Latest Rust and Visual Studio may not support Windows 7; a pinned version is not a compatibility guarantee.

For Linux executables **as well**, install Windows Subsystem for Linux (WSL) and a Linux distribution, set up Linux C compilers and multilib in that distribution, then run `.\scripts\build-windows.cmd -IncludeLinux`. It calls the Linux script in WSL with `--linux-only`: that script installs/updates its own Linux Rust stable, runs Linux tests, and builds 32-bit and 64-bit Linux binaries in a separate `artifacts/build-*/` directory. `-SkipTests` applies to both; `-Toolchain` applies to Windows only. On Debian/Ubuntu x86_64 in WSL, you can first run `./scripts/build-linux.sh --linux-only --install-deps` to install Linux prerequisites. WSL is not available on Windows 7. Builds on newer Linux may not run on older glibc systems.

Run the `zfm.exe` in the target subdirectory matching the Windows machine, using the exact `artifacts/build-windows-*/...` path printed by the script. The generated executables are local build outputs and are not committed to Git. The Windows GUI and Windows build script remain untested here; the GUI is an incomplete browser rather than a full file manager, and cross-building does not establish runtime compatibility. See [Windows notes](docs/windows.md).

## GitHub Actions and releases

The **Build and release** workflow runs on pull requests, pushes to `main`, version-tag pushes and manual runs. It tests and builds the 64-bit and 32-bit Linux GNU and Windows MSVC targets, and makes the four packaged builds available as workflow artifacts. Linux archive tests require both libarchive and Python; CI treats missing test prerequisites as a failure rather than silently skipping them. GUI interaction is not tested by this workflow. The workflow itself has not yet been run on GitHub.

To publish a release, first merge the intended source into `main`, then create and push a version tag (for example `v0.1.0`) pointing to the intended commit. The tag must already exist on GitHub. After **all four builds and tests pass**, the workflow creates a GitHub Release for that tag if necessary and uploads these assets:

- `zfm-x86_64-unknown-linux-gnu.tar.gz` and `zfm-i686-unknown-linux-gnu.tar.gz`
- `zfm-x86_64-pc-windows-msvc.zip` and `zfm-i686-pc-windows-msvc.zip`
- `SHA256SUMS.txt` for verifying the four archives

Each archive contains the executable, README and license. Re-running the workflow for the same tag replaces assets with the same names; tags containing a hyphen create prereleases. Pull requests and manual runs do **not** publish releases. Release publication requires GitHub Actions to be enabled and the workflow token to be allowed to write repository contents.

These builds use current stable Rust and hosted runners. Linux binaries require compatible system libraries (including GTK 3; libarchive for ZIP and compressed TAR); the Windows browser remains experimental and incomplete. The workflow does not establish Windows 7, Mint 19, or interactive GUI compatibility.

## Development roadmap

The foundation and added actions need interactive GTK testing and compatibility verification. Recursive file operations, properties, recursive search, bookmarks, recent files, multiple selection, drag-and-drop, alternative views and advanced features are **not implemented**. Delete alone moves to trash when the file list has focus; it never silently performs permanent deletion. Future phases should implement and test core operations before adding advanced tools, and profile real workloads before making performance claims.

Licensed under [Apache-2.0](LICENSE). Made by Zelvior.
