<!-- Modified: document preview, session settings and uninstall scope and limits. -->
<!-- Modified: describe the compact file-manager interface. -->
# ZFM — Zelvior File Manager

<img src="packaging/zfm-wordmark.svg" alt="ZFM — Zelvior File Manager" width="320">

A small native file manager written in Rust with a compact, dark details-view desktop interface. **This repository currently contains an early foundation and a limited set of file operations, not the complete file manager described in the project brief.** It is not yet recommended as a replacement for an established file manager.

## What works in this foundation

- Linux GTK 3 window with a compact dark theme, 4-column sortable directory table (`Name`, `Date`, `Type`, `Size`), grouped sidebar shortcuts, and selection details. The toolbar’s Organize menu retains file operations including trash, copy, move, duplicate and cancellation. Search and path entry remain available in the navigation bar.
- Enter a path and press Enter; double-click a folder to enter it or a file to open it with `xdg-open`. Back, forward, parent and refresh buttons; F5, Alt+Left/Right/Up and Ctrl+L shortcuts.
- Hidden-file toggle, folder-first case-insensitive name ordering, file sizes and modification dates. Broken links remain visible. Errors opening a directory are shown in the status bar.
- Directory enumeration and metadata run on a worker. New navigation cancels the *results* of earlier work; UI rows are added in batches on the GTK main loop. There is no persistent background service.
- A current-folder filename filter (including extension substrings), with an optional case-sensitive mode. Press Search or Enter to apply; Ctrl+F focuses it. It is not recursive, and the filter stays active while navigating until cleared.
- Single-item create file/folder, rename, duplicate a file or folder, copy a file or folder to another folder, and move an item to another folder on the same filesystem. These run in a worker and refuse to overwrite. The Trash action uses GIO without a permanent-delete fallback. Permanent deletion of a file, symlink, or **empty** folder requires confirmation. With the file list focused, Delete moves the selection to trash, F2 renames and Shift+Delete requests permanent deletion.
- The archive manager browses and extracts TAR, ZIP, TAR.GZ/TGZ, TAR.BZ2 and TAR.XZ; ZIP and compressed TAR require the system libarchive runtime. It tests ZIP/compressed archives and creates a TAR or ZIP from one regular file. Operations run in the background with cancellation and no-overwrite behavior. See the limitations below before use.
- The built-in offline converter supports CSV → JSON, restricted JSON → CSV, UTF-8 plain text → escaped HTML, and restricted 24-bit BMP ↔ ICO. Linux has a converter window with progress and cancellation; Windows offers a file context-menu conversion, running in a worker.
- Linux tabs retain independent navigation histories; use Ctrl+T/W, Ctrl+Tab or Ctrl+PageUp/PageDown, Ctrl/Alt+1–9, or the tab and folder context menus. The clipboard can copy and paste multiple local files and folders via GNOME copied-files and URI-list targets (Ctrl+C/X/V with the list focused). Cut remains single-item and uses the same same-filesystem move operation as Move to.
- The native Win32 browser uses the same dark-slate desktop palette, sortable columns, sidebar and tab histories. It supports selection, context menus, common keyboard shortcuts, multi-item clipboard copying, single-item cut and move, rename, duplicate, Recycle Bin and confirmed permanent deletion. Empty name fields open an input dialog; file changes run on a worker and refuse replacement. The Windows UI remains experimental.
- Linux row and background context menus support item and folder actions; right-clicking an already-selected row preserves the selection. Middle-click a folder or sidebar location to open a tab, or a tab to close it. Ctrl+H toggles hidden files; Ctrl+Shift+C copies a path.
- Both file lists support multi-selection and row/background, sidebar and tab context menus. Clipboard copy supports multiple selected items; other file actions require one item at a time and reject multiple selections. On Windows, Delete requests the Recycle Bin and Shift+Delete asks for confirmation before permanent deletion.
- No third-party Rust crates. GTK 3 is loaded at runtime through a small platform adapter; filesystem and navigation code are separate.

Sidebar locations currently use conventional subdirectories of `$HOME`, **not** customized XDG user directories. Both lists support multiple selection for clipboard copying, but other file actions still require one selected item. The list is batched but not virtualized: very large directories still consume memory for all entries and widgets. Directory sorting happens after enumeration; a slow network filesystem call cannot be interrupted until it returns. Displayed non-UTF-8 filenames are lossily decoded, although double-clicking uses the original path. Entering a non-UTF-8 path in the address bar is not supported. File opening depends on `xdg-open` and an installed application for the file type.

## Built-in preview and removal

Right-click one regular file and choose **Preview in ZFM…**, or focus the list and press Ctrl+P. The read-only viewer accepts UTF-8 or BOM-marked UTF-16 text (up to 4 MiB) for TXT, Markdown, LOG, CSV, TSV, JSON, XML, HTML, CSS, JS, TS, Rust, Python, C/C++, TOML, YAML, INI, CONF, SH and PS1 extensions. HTML and Markdown are displayed as source, not rendered. PNG, JPEG, GIF and BMP files up to 16 MiB and 40 million pixels can be previewed using the system image decoder; image zoom is limited and animated images show a static frame. Decoder availability and supported subformats vary by system. GTK scales images on load; Windows uses a conservative header check before decoding. These checks are not a decoder sandbox: avoid previewing untrusted files. Reading and decoding currently happen on the GUI thread and may temporarily block it. Final-component links and special files are rejected; path components are not protected against hostile concurrent changes.

This is **not** a universal document viewer: PDF, Office documents, ebooks and other formats are not rendered. Use **Open** to launch the OS default application for unsupported files (or if a preview fails); opening continues to use the external application, not the built-in viewer.

On Windows, the installed uninstaller opens an installer-style confirmation and progress wizard. It removes the registered installation's managed files and shortcuts, preserving other files. A temporary copy performs removal so the installed executable can be deleted. If it cannot finish, close ZFM and retry; partial changes are possible. Silent removal (`Uninstall.exe /S`) returns after handing off to the temporary process, so a zero exit code from the original process does **not** prove removal succeeded; check the installed files and registration. Temporary copies may remain until reboot. On Linux, **Organize → Uninstall ZFM…** offers to remove the installed Debian package through `pkexec` and `dpkg --remove`, with authentication and package-manager dependency checks. It does not manually remove package-owned files or personal files. If PolicyKit is unavailable or this is not a Debian-package installation, use your package manager or the original installation method instead. The new viewer and uninstall UIs need further platform testing; Windows 7 and Mint 19 are not verified.

## Settings

Open **Settings…** from Linux’s Organize menu or the Windows file-list background menu, or press Ctrl+, on either platform. The dialog controls hidden-file visibility and the current sort direction. Apply changes them for the current session; Cancel leaves them unchanged. Preferences are not yet saved across restarts.

## Mouse and keyboard

Click a row to select it; Ctrl/Shift-click selects multiple rows and clicking empty list space clears the selection. Double-click opens an item, while middle-clicking a folder or sidebar location opens a new tab; middle-click a tab to close it. Right-click rows, empty space, tabs or sidebar items for context actions. Shift+F10 or the Menu key opens the list context menu. Ctrl+double-click a folder opens it in a new tab; Alt+double-click shows properties.

| Shortcut | Action |
| --- | --- |
| Enter / Backspace (list) | Open selected item / parent folder |
| Alt+Left / Right / Up | Back / forward / parent |
| F5 or Ctrl+R | Refresh |
| Ctrl+L or Alt+D / Ctrl+F | Focus address / filter |
| Ctrl+T / Ctrl+W | New / close tab |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PageDown / PageUp | Cycle tabs |
| Alt+1–9 or Ctrl+1–9 | Select a tab |
| F2 / Delete / Shift+Delete | Rename / trash / confirmed permanent delete |
| Ctrl+C / X / V | Copy selection / cut one item / paste |
| Ctrl+Shift+C | Copy selected or current path as text |
| Ctrl+D / Ctrl+N / Ctrl+Shift+N | Duplicate / new file / new folder |
| Ctrl+H / Alt+Enter / Escape | Toggle hidden / properties / clear selection and focus list |
| Ctrl+, / Ctrl+P | Open session settings / preview one selected file |

## File-operation limits

Clipboard copy handles up to 1,024 selected top-level items; other file actions operate on one selected item. Copy and duplicate handle regular files and directory trees on Windows and Linux x86, x86_64 and aarch64. Folder copies include empty directories, but reject symbolic links, Windows reparse points and special files anywhere in the tree, and reject a destination inside the source. Trees are limited to 100,000 entries and 128 directory levels. They do not preserve permissions, timestamps, ownership or other metadata. Linux copies use restrictive permissions; Windows copies inherit destination permissions. Copy runs on a worker; GTK offers cooperative cancellation, while Win32 has no copy cancellation control yet. Cancellation cannot interrupt a blocked filesystem call and may be too late once publication begins.

Copies stage in the destination parent and publish without replacement. Linux file copies require hard-link support; folder publication requires `renameat2(RENAME_NOREPLACE)` support. Windows uses a no-replace atomic move. On errors or cancellation the staging tree is removed when possible; cleanup errors are reported, and a crash or forced exit may leave `.zfm-copy-*.tmp` files or folders. Path checks do not prevent hostile concurrent modification of source or destination directory components: only copy from and into directories that untrusted users cannot concurrently change or move. The source is not a snapshot and may change during a copy.

On Linux, rename and move use `renameat2(RENAME_NOREPLACE)`, requiring a kernel and filesystem that support it. Move is same-filesystem only; a cross-filesystem move fails and leaves the source in place. Linux rename and move are currently implemented for x86, x86_64, and aarch64 only. Windows rename and move use `MoveFileExW` without replacement or cross-volume copying. The destination is never intentionally overwritten. Directory deletion is intentionally restricted to empty directories, does not recurse, and deleting a symlink removes the link rather than its target. Permanent deletion cannot be undone. Linux Trash uses GIO and may be unavailable on some filesystems or mounts; failures are reported without a permanent-delete fallback. Windows Recycle Bin requests are limited to fixed drives and reject reparse points; Windows may show its own warning if recycling is unavailable, including a request to permanently delete. Clipboard copy stops at the first error or cancellation and reports the completed count and failed item; earlier copies remain, and later items are not attempted. There is no rollback. When pasting in the original folder, a non-conflicting copy name is suggested; elsewhere existing names are never replaced. Cut/paste supports one item only, including for external clipboard payloads. Linux rejects remote URI payloads and limits clipboard data to 64 KiB; Windows limits it to 1 MiB of path data. URI-list-only payloads are treated as copy (not cut), and Linux clipboard contents may not persist after ZFM exits without a clipboard manager. There is no bulk move/delete or cross-filesystem move. The new GTK controls and destructive-action confirmation have **not** been tested interactively in this environment; use caution and back up data before trying them.

## Conversion and archives: current scope

**Built in on Linux and Windows:** UTF-8 CSV → JSON, JSON → CSV, plain text → HTML, and restricted BMP ↔ ICO. On Windows, right-click one CSV, JSON, TXT, BMP or ICO file and choose “Convert file (built-in)”; choose an output name in the same folder. Windows shows a working/completion status but has no conversion progress or cancellation control yet. On Linux, use the Converter window or the file context menu; output paths can be chosen there. Text → HTML escapes text as literal content, supports valid UTF-8 and rejects unsupported ASCII controls; it does not interpret Markdown. BMP ↔ ICO only handles uncompressed, bottom-up opaque 24-bit images from 1 to 256 pixels per side; ICO input must contain one BMP-based image with a zero AND mask. PNG-backed icons, alpha/transparency, palettes and multi-image icons are rejected rather than flattened. BMP/ICO inputs are capped at 1 MiB. Other image, audio and video conversions and external tools are not integrated.

JSON → CSV accepts a nonempty array of objects whose nonempty unique keys are identical across objects and whose values are all strings; nested structures, numbers, booleans, null, and an empty array are rejected rather than coerced. It supports JSON Unicode escapes and key reordering. CSV → JSON: The first row supplies unique, non-empty object keys; every value is a JSON string (no inference). Quoted fields, escaped quotes, embedded newlines and a UTF-8 BOM are supported. Every row must have the same number of fields. To bound memory use, records are limited to 8 MiB and 4,096 fields. The input is streamed; output is staged in the destination directory and published with a no-replace hard link. Existing files, including symlinks, are never intentionally overwritten. The destination filesystem must support hard links; otherwise conversion fails. Cancellation is cooperative and cannot interrupt a blocked filesystem call; it may be too late once publication has begun. Normal errors and cancellations remove the staging file; a process crash or forced termination may leave a `.zfm-*.tmp` file.

**Archive support:** uncompressed POSIX ustar and simple GNU TAR, plus ZIP and gzip/bzip2/xz-compressed TAR via dynamically loaded `libarchive.so.13`. A compatible libarchive installation with the relevant built-in codecs is required for ZIP and compressed TAR. Archives may contain regular files and directories only. The Test button reads ZIP/compressed data and asks libarchive to validate available checksums; it cannot prove authenticity. TAR has no content checksum; testing uncompressed TAR validates its structure and header checksums only. Browsing alone does not verify file contents. The entire archive is inspected before extraction; absolute/traversal paths, duplicate names, links, devices and non-UTF-8 names are rejected. Uncompressed TAR also rejects PAX/GNU extension headers. Limits are 10,000 entries, 512 MiB per file and 1 GiB total. Linux extraction is implemented for x86, x86_64 and aarch64 using directory-relative no-follow operations, and never intentionally overwrites files. The destination must already exist and its final component cannot be a symlink. Select individual entries or all entries; selecting a directory alone does **not** select its contents. Destination directories are created with mode 0700 and files with 0600; timestamps, permissions and ownership are not restored. Extract only into directories that untrusted users cannot concurrently modify or move. Cancel or an error removes the current partial file, but previously extracted files and directories remain; a crash may leave a partial file. Cancellation cannot interrupt a blocked filesystem call. Archive contents can be dangerous even when path checks pass; do not run untrusted content.

TAR and ZIP creation are limited to one regular file with a safe UTF-8 basename no longer than 100 bytes, up to 512 MiB. It streams to a temporary file, then uses a no-replace hard link, so the output filesystem must support hard links. It does not preserve source permissions or timestamps. Cancellation normally cleans up the staging file; a crash may leave `.zfm-tar-*.tmp` or `.zfm-zip-*.tmp`. Do not create archives in directories writable by untrusted users because staging publication is not protected against hostile concurrent changes.

**Optional external tools:** none integrated; libarchive is a system library, not an external conversion process. **Unsupported:** all other requested conversions, batch conversion, multi-file or directory archive creation, creation of compressed TAR, standalone compression, passwords, and RAR/7Z. The archive and converter GTK controls and file choosers have not been exercised interactively in this environment. File chooser paths are displayed as UTF-8, so non-UTF-8 paths cannot be used reliably in this interface.

## Status and compatibility

| Target | Status |
| --- | --- |
| Linux x86 / x86_64 | Core tests and release builds passed on GitHub's Ubuntu 22.04 runners for both targets; x86_64 also passed locally on Debian 12 with Rust 1.85. GTK window **not tested interactively**. |
| Linux Mint 19 / Ubuntu 18.04, x86 or x86_64 | Intended target, **not verified**. GTK 3.22+ and Rust 1.70 are the baseline chosen for this foundation; actual package, toolchain and GUI compatibility still require testing on each system. |
| Windows x86 / x86_64 | MSVC builds, tests and GUI/installer smoke tests run on GitHub Windows Server 2022 runners. Windows archive UI is not implemented. Windows has a limited context-menu converter. Windows 7 compatibility is not verified. |

A Rust build produced on a newer distribution can require a newer glibc than Mint 19 provides. Build on the oldest supported distribution, or in a compatible build environment, and test the actual artifact on the target. Architecture support is not guaranteed merely by installing a Rust target.

## Installing ZFM

Releases contain **installers only**; there are no portable archives or loose executables to unpack. Download the installer matching your operating system and architecture from the GitHub Releases page. **The existing v0.0.2 release predates these installers.** They need a new release tag and have not yet been tested on Windows 7 or Mint 19.

- **Windows 7 SP1 or newer:** run `zfm-i686-pc-windows-msvc-setup.exe` for 32-bit Windows, or `zfm-x86_64-pc-windows-msvc-setup.exe` for 64-bit Windows. Setup is ZFM's own installer, written in Rust and built from this repository; it does not use NSIS, Inno Setup or any other third-party installer framework. It installs for the current user, shows the native ZFM setup wizard, adds a Start Menu shortcut and an entry in Programs and Features, and writes an uninstaller next to the application. It also supports unattended `setup.exe /S /D=C:\Path\To\ZFM`. It does not install a service or require Rust. Choose a dedicated folder; unrelated files in that folder are preserved on uninstall. The setup executable is not code-signed, so Windows may display a security warning.
- **Linux Mint 19 / Ubuntu 18.04:** use the `i686` `.deb` for a 32-bit OS or the `x86_64` `.deb` for a 64-bit OS. Double-click the downloaded package to open your package installer, or run `sudo apt install ./zfm-i686-unknown-linux-gnu.deb` (use the x86_64 filename on 64-bit systems). The package declares the GTK 3, libarchive and `xdg-utils` dependencies and adds ZFM to the applications menu. Dependencies may require configured package repositories. Uninstall with the package manager or `sudo apt remove zfm`.

The Windows interface is still an incomplete file manager; see the limitations above. Back up important files before testing.

## Troubleshooting startup on older systems

If Windows setup reports **Access is denied** for `zfm.exe`, close every running ZFM window and retry setup. If it still fails, check that your user can write to the installation folder and that security software is not blocking the file, or choose another writable folder. In the updated installer, when the existing ZFM is running and blocks an upgrade, setup offers to close that installation gracefully and retry; it does so only with your permission and does not force-terminate it. Identical files are not rewritten, and other access errors offer Retry or Cancel. Previously downloaded installers do not change: these fixes require a newly built release.

The old `v0.0.1` release was built on Ubuntu 22.04 with current Rust. It can fail before a window appears on Linux Mint 19 (glibc 2.27) or Windows 7. Download a newer release **after** the updated workflow has successfully built and published one; existing release files are not automatically replaced. For 32-bit systems use the `i686` installers, not `x86_64`. Windows needs Windows 7 SP1 or newer, although this has not yet been tested on a Windows 7 machine. On Linux, install the `.deb` with your package manager rather than running files by hand. Linux requires a graphical session and GTK 3; archive support additionally needs libarchive. A terminal will show startup errors if the desktop cannot display them.

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

For example, on a 64-bit Debian-family host, `gcc-multilib` and `libc6-dev-i386` may supply the cross-linker and C library. The target machine also needs the **32-bit** GTK 3 runtime and its dependencies. The 32-bit build and core tests have passed on the GitHub Ubuntu runner, but Mint 19 and interactive 32-bit GUI use remain unverified.

Release builds are distributed only as installable packages. To build the `.deb` for an already-built executable:

```sh
./scripts/package-deb.sh     target/release/zfm     x86_64-unknown-linux-gnu     v0.0.3     zfm-linux.deb
```

The package contains the executable, the applications-menu entry and the black-and-white ZFM icon, but not GTK or `xdg-open`; recipients need compatible system runtime libraries and a graphical session, and no Rust installation. See [Linux notes](docs/linux.md).

## Windows

On Windows with PowerShell 5.1 or later and the [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) installed, open Command Prompt or PowerShell in the checkout and run:

```powershell
.\scripts\build-windows.cmd
```

The script installs rustup in the user account if necessary, installs/updates Rust stable, runs native tests and builds both `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. It places each `zfm.exe` in `artifacts/build-windows-*/<target>/` and exits nonzero if any requested build fails; successful binaries remain available. The launcher bypasses the execution policy only for that PowerShell process; it does not change the machine policy. Rustup requires internet access. Install the **Desktop development with C++** workload, including the Windows SDK and x86/x64 build tools; the script does not install Visual Studio automatically. A 32-bit host also needs the x64 cross-build tools to build both targets.

Use `-NativeOnly` for the host Windows architecture, `-Toolchain 1.85.0` (for example) to choose a Rustup toolchain instead of latest stable, or `-SkipTests` to skip tests. These can be passed to the `.cmd` launcher. Latest Rust and Visual Studio may not support Windows 7; a pinned version is not a compatibility guarantee.

For Linux executables **as well**, install Windows Subsystem for Linux (WSL) and a Linux distribution, set up Linux C compilers and multilib in that distribution, then run `.\scripts\build-windows.cmd -IncludeLinux`. It calls the Linux script in WSL with `--linux-only`: that script installs/updates its own Linux Rust stable, runs Linux tests, and builds 32-bit and 64-bit Linux binaries in a separate `artifacts/build-*/` directory. `-SkipTests` applies to both; `-Toolchain` applies to Windows only. On Debian/Ubuntu x86_64 in WSL, you can first run `./scripts/build-linux.sh --linux-only --install-deps` to install Linux prerequisites. WSL is not available on Windows 7. Builds on newer Linux may not run on older glibc systems.

To build the setup program, build the executable for the same architecture first and then point the installer package at it:

```powershell
$env:ZFM_APP_BINARY = (Resolve-Path "target\release\zfm.exe").Path
$env:ZFM_VERSION = "v0.0.3"
cargo build --release -p zfm-installer
```

This writes `target\release\zfm-setup.exe`: a self-contained setup executable that embeds the application, the icon, the license and the README, and that can also act as the uninstaller. Run the plain `zfm.exe` in the target subdirectory matching the Windows machine, using the exact `artifacts/build-windows-*/...` path printed by the script. The generated executables are local build outputs and are not committed to Git. For Windows 7, current Rust stable binaries target Windows 10 or later; to build for Windows 7 try `-Toolchain 1.77.2` with compatible Visual C++ tools. This older-platform setup is not yet verified here. The Windows GUI and Windows build script remain untested interactively; the GUI is an incomplete browser rather than a full file manager, and cross-building does not establish runtime compatibility. See [Windows notes](docs/windows.md).

## GitHub Actions and releases

The **Build and release** workflow runs **only when manually requested**, and requires a version tag each time. Pushes, pull requests and tag pushes do not run it automatically. It tests and builds the 64-bit and 32-bit Linux GNU and Windows MSVC targets and makes the four installers available as workflow artifacts. Linux archive tests require both libarchive and Python; CI treats missing test prerequisites as a failure rather than silently skipping them. The workflow includes GUI startup smoke tests and installer install/uninstall checks; these are not a substitute for testing on the target desktop systems.

To publish a release:

1. Merge the intended source into `main` (or choose another branch containing the workflow and the intended source).
2. In GitHub, open **Actions → Build and release → Run workflow**, select that branch, and enter a `v`-prefixed version such as `v0.0.1` in the required **vtag** field. Alternatively run `gh workflow run build-release.yml --ref main -f vtag=v0.0.3 -f publish=true`.
3. Check **publish** if you want a release; leave it unchecked to test only. If the tag already exists, the workflow builds its commit. If it does not exist, it builds the selected branch's commit from the time the run was started. After **all four builds and tests pass**, it creates a missing tag at that exact commit, then creates or updates the GitHub Release and uploads these assets:

- `zfm-{x86_64,i686}-unknown-linux-gnu.deb` (Linux installers)
- `zfm-{x86_64,i686}-pc-windows-msvc-setup.exe` (Windows installers)
- `SHA256SUMS.txt` for verifying all four downloads

If a tag existed when the run began but points to a different commit or disappears, publication fails. If a new tag is created by someone else while the build runs, the run fails rather than replacing it; check the tag and retry. A tag created after successful builds may remain if a later release upload fails. Use protected, immutable release tags when possible.

Each setup executable contains the application, icon, license and README, and the `.deb` installs the same files with a package-manager entry. Older releases contain portable archives and loose executables; those assets are no longer produced. Re-running the workflow for the same tag replaces assets with the same names; tags containing a hyphen create prereleases. Manual runs default to test-only and still require a tag input; check **publish** to create or update the release after the tests pass. Test-only runs do not create tags or releases. No release is published by a push or pull request. Release publication requires GitHub Actions to be enabled and the workflow token to be allowed to write repository contents.

The release workflow uses Rust 1.77.2, builds Linux inside Ubuntu 18.04 (glibc 2.27), and statically links the Windows C runtime to target older systems. It builds each setup program with the matching embedded executable, checks that GUI windows appear on hosted runners, and installs, launches and uninstalls the result before publishing. The v0.0.2 GitHub run passed both architectures' builds and GUI startup smoke tests. The newer black-and-white interface and the rewritten installer have not yet been verified by a release workflow. Neither installer has been verified on an actual Mint 19 or Windows 7 system. Linux binaries require compatible system libraries (including GTK 3; libarchive for ZIP and compressed TAR); the Windows browser remains experimental and incomplete. The workflow does not establish Windows 7, Mint 19, or interactive GUI compatibility.

## Branding

The ZFM mark is a black-and-white badge: a paper-white rounded square, a solid black border and a black "Z" monogram. `packaging/zfm.svg` holds the mark, `packaging/zfm-wordmark.svg` the horizontal lockup, and `packaging/generate-icons.py` regenerates `packaging/zfm.ico` plus the 64 px and 256 px PNG files (Python 3 only, no third-party modules). The Windows executable, the Start Menu shortcuts and the setup wizard all use that icon.

## Development roadmap

The foundation and added actions need interactive GTK testing and compatibility verification. Recursive file operations, recursive search, bookmarks, recent files, drag-and-drop and advanced features are **not implemented**. Selection and basic properties are available; file operations still require one item. Delete alone moves to trash when the file list has focus; it never silently performs permanent deletion. Future phases should implement and test core operations before adding advanced tools, and profile real workloads before making performance claims.

Licensed under [Apache-2.0](LICENSE). Created by Zelvior at Zelvior Labs.

See [development phases and conversion matrix](docs/development-plan.md) for current scope, risks and reproducible baseline measurement.

## Legal, privacy and security

ZFM is distributed under [Apache License 2.0](LICENSE). Read the [release-readiness audit](LEGAL/RELEASE_READINESS.md), [third-party and runtime inventory](LEGAL/THIRD_PARTY.md), [privacy notes](LEGAL/PRIVACY.md), [operational terms](LEGAL/TERMS.md), and [security reporting policy](SECURITY.md). The [source-workspace SBOM](SBOM.spdx.json) is not a complete binary inventory. Ownership, trademark clearance, private security reporting and release-specific third-party obligations require further action; these materials are not legal advice or certification. Installed README copies include Rust 1.77.2 upstream reference notices; links to repository documents may require visiting the [source repository](https://github.com/zelvior/zfm).
