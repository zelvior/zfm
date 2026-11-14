<!-- Modified: document persistent settings, preview and updated uninstall behavior. -->
# Windows development notes

The experimental Win32 browser includes multiple row selection, row/background/tab/sidebar context menus, common navigation and file shortcuts, clipboard copy/cut/paste for multiple items, rename and create dialogs, Properties, and Recycle Bin requests. Direct copy, Move to, Recycle Bin and archive creation also accept multiple items; most other file operations remain single-item. Copy accepts up to 1,024 regular files or bounded directory trees, stops on the first failure with earlier copies retained, rejecting links, reparse points and special files; move is same-volume and never overwrites. Plain Delete asks the Windows shell to recycle up to 1,024 non-overlapping items on fixed drives, one at a time; remote/removable drives and reparse points are rejected. The batch stops on the first failure or cancellation, does not restore earlier items, and an item may be partially processed by the shell. ZFM cannot interrupt an active shell operation. The shell may warn and ask for confirmation if recycling is unavailable. Shift+Delete confirms permanent deletion and only removes files or empty directories. Clipboard, dialogs and Recycle Bin behavior should still be tested on target Windows versions; Windows 7 has not been exercised in CI. Right-click selected files or folders and choose **Copy to folder…** (up to 1,024 items) or **Move to folder…** (up to 1,024 items); enter an existing absolute path. While an operation is running, the file-list context menu offers cooperative cancellation for supported copy, batch move, batch trash, conversion and archive jobs. Batch moves stop at the first error or cancellation and retain earlier results without rollback. Some operations cannot be cancelled and completed outputs remain.


Press Ctrl+B to bookmark the current folder; use the file-list context menu → **Bookmarks** to add, open or remove bookmarks. They are stored in `%APPDATA%\zfm\bookmarks.conf` and remain after uninstall. Bookmark paths must be absolute UTF-8 without control characters; the limit is 64 entries.

Enter a filename term and use Ctrl+Shift+F or the file-list context menu → **Search subfolders**. Results show relative paths and support **Open containing folder**. Search is case-insensitive; Clear exits search mode. See the README for limits and filesystem caveats.

The built-in converter offers uncompressed PCM WAV ↔ AIFF and WAV ↔ AU (8/16-bit mono/stereo only) from the file context menu; metadata is not preserved. See the README for limits.

## Automated builds

Run `scripts\build-windows.cmd` from the checkout. It downloads/updates Rust with rustup, tests the native target, and attempts 32-bit and 64-bit Windows MSVC builds. Install Visual Studio C++ Build Tools, its Desktop development with C++ workload, Windows SDK and x86/x64 tools first. System prerequisites are not installed automatically. PowerShell 5.1+ and network access to rustup/Rust distribution servers are required. The script has not been run on Windows yet.

Use `-NativeOnly` to build just the host target; `-SkipTests` to skip tests; `-Toolchain VERSION` to choose a Rustup toolchain; and `-IncludeLinux` to additionally invoke `build-linux.sh --linux-only` through an installed and configured WSL distribution. The Linux script uses its own Rust stable, its outputs are in a separate directory, and WSL requires its own compiler and multilib prerequisites. WSL is unavailable on Windows 7. See the README for output locations and limits.

From a native Windows Rust installation with the Microsoft C++ build tools installed:

```powershell
rustup target add x86_64-pc-windows-msvc i686-pc-windows-msvc
cargo test --locked --target x86_64-pc-windows-msvc
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo build --release --locked --target i686-pc-windows-msvc
```

From Linux, GNU cross-compilation additionally requires the matching MinGW compiler/linker and Rust target (for example `x86_64-w64-mingw32-gcc` and `x86_64-pc-windows-gnu`):

```sh
rustup target add x86_64-pc-windows-gnu i686-pc-windows-gnu
cargo build --release --locked --target x86_64-pc-windows-gnu
cargo build --release --locked --target i686-pc-windows-gnu
```

Use the appropriate MinGW toolchain for both architectures and configure Cargo linkers if they are not automatically found. The native Windows adapter must be linked and tested on each supported OS and architecture; modern Rust/toolchains may not support Windows 7. The Windows interface remains incomplete; see README for the exact limitations.

The old v0.0.1 release used Rust stable targeting Windows 10+. The revised release workflow pins Rust 1.77.2 and uses the static C runtime to target Windows 7 SP1+, but neither the revised artifact nor the GUI has yet been verified on an actual Windows 7 system.

## Installer

ZFM ships its own installer instead of using NSIS or another third-party framework. It lives in `installer/` (a separate `zfm-installer` package in the workspace) and is plain Rust with direct Win32 calls, like the application itself.

- The setup executable embeds the release `zfm.exe`, the renamed archive engine and its source, PDFium and its notices, `zfm.ico`, the license and the README at build time, so it needs no network access and no extra archive.
- The wizard uses ZFM's custom-rendered slate-and-metallic setup interface and offers welcome, folder, license, progress and finish pages. `/S` installs silently and `/D=<folder>` chooses the folder. A silent run never opens a dialog, not even for bad arguments: it reports through the exit code (0 success, 1 failure, 2 bad usage).
- Setup installs for the current user in `%LOCALAPPDATA%\Programs\ZFM` by default, writes the Start Menu shortcuts and the Programs and Features entry under `HKCU`, and copies itself to `Uninstall.exe` next to the application.
- Interactive uninstall opens a confirmation/progress wizard and removes managed files only from the registered folder. Other files are kept, and the folder is removed only when empty. A temporary copy performs removal; if files are locked, close ZFM and retry. Removal can be partial on error, and temporary copies may remain until reboot. `/S` started from the installed uninstaller returns after launching the temporary copy: its exit code reports handoff, not the final removal result. Verify files and registration afterwards.
- Setup refuses to replace unrelated files: reinstalling into the recorded folder upgrades in place, while any other folder that already contains a managed file is rejected. Identical installed files are not rewritten, so reinstalling the same version can succeed while ZFM is running.
- If a changed file is in use or access is denied, the wizard explains the possible causes. If ZFM from the target installation has an identifiable open window, setup asks permission to close it gracefully and retry; declining leaves it running. Setup never force-terminates processes and never closes ZFM installations from other paths. Other locks or permission errors offer Retry or Cancel. If closing ZFM does not help, check other programs, folder permissions or security software, or choose another writable folder. A silent `/S` run never closes ZFM or prompts and returns exit code 1 on failure; retry it after resolving the lock. Replacement files, including the uninstaller, are staged before publication, and a failed replacement leaves the prior file intact.

Build it after the application for the same architecture:

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
./scripts/build-archive-engine.ps1 -Target x86_64-pc-windows-msvc -Output target/zfm-archive-engine.exe
python ./scripts/prepare-pdfium.py x86_64-pc-windows-msvc target/pdfium
$env:ZFM_PDFIUM_BINARY = (Resolve-Path "target/pdfium/pdfium.dll").Path
$env:ZFM_PDFIUM_NOTICES = (Resolve-Path "target/pdfium/pdfium-notices.txt").Path
$env:ZFM_ENGINE_BINARY = (Resolve-Path "target/zfm-archive-engine.exe").Path
$env:ZFM_APP_BINARY = (Resolve-Path "target/x86_64-pc-windows-msvc/release/zfm.exe").Path
$env:ZFM_VERSION = "v0.0.3"
cargo build --release --locked --target x86_64-pc-windows-msvc -p zfm-installer
```

`ZFM_APP_BINARY`, `ZFM_ENGINE_BINARY`, `ZFM_PDFIUM_BINARY` and `ZFM_PDFIUM_NOTICES` are required when building a real installer; without the app the package builds a placeholder payload so `cargo test -p zfm-installer` still runs. The installer unit tests (`cargo test -p zfm-installer`) cover option parsing, folder selection, shortcut COM initialization on worker threads and the file install/remove rules. The Windows jobs of CI and of the release workflow build setup for both architectures and run `tests/installer-windows.ps1`, which exercises both silent setup and the interactive wizard pages, checks the files, the Start Menu shortcuts and the Programs and Features entry, upgrades in place, recovers an incomplete installation, uninstalls, verifies that user files survive and that an emptied folder is removed, and checks `/UNINSTALL`. Windows 7 SP1 has not yet been exercised in CI.

### Program Compatibility Assistant and publisher identity

The setup and uninstaller embed an `asInvoker` manifest with Windows 7 and later compatibility declarations. This is intended to avoid Windows 7's false “might not have installed correctly” installer-detection warning, without changing or disabling Windows security settings. The setup still reports real failures; a compatibility declaration is not proof of successful installation or of Windows 7 compatibility. Check that ZFM appears in Programs and Features and launches before selecting “This program installed correctly.” If setup failed, resolve that error and run setup again rather than selecting that option. Windows 7 testing is still needed to confirm the warning is resolved on the target system.

The executable's version metadata identifies the product as ZFM Setup by Zelvior Labs, and Programs and Features lists Zelvior Labs as publisher. This is descriptive metadata, **not a digital signature**: an unsigned installer can still show “Unknown publisher” in security prompts. A trusted code-signing certificate and signing the release binaries would be required for a verified publisher. Existing downloaded installers cannot acquire these changes; they require a new release build. Building the installer for Windows GNU additionally requires the matching `i686-w64-mingw32-windres` or `x86_64-w64-mingw32-windres` on PATH.

## Built-in converter

Right-click a single CSV, JSON, TXT, BMP or ICO file and choose **Convert file (built-in)**. CSV converts to JSON, string-valued JSON objects convert to CSV, UTF-8 text converts to escaped HTML, and opaque 24-bit BMP and ICO images up to 256 × 256 convert in either direction. Multi-image, PNG-backed, transparent, paletted and other icon/bitmap variants are not supported. Enter a new filename in the source folder. The operation runs in the background and reports completion or errors in the status bar; it cannot currently be cancelled or show detailed progress on Windows. Existing output files are never intentionally replaced, and the destination filesystem must support hard links. See the [conversion matrix](development-plan.md) for exact scope. No FFmpeg or other external converter is integrated.

## Archive browser (partial)

Right-click one supported archive to **Extract here…**, **Extract to folder…**, or **Extract with password…**. Select files or folders and use **Compress to** to choose a format or encrypted 7z/ZIP. Standalone gzip, bzip2 and xz compression requires one regular file. The Windows destination prompt accepts an existing absolute folder path. The archive engine must be installed alongside ZFM.

Right-click a single file and choose **Open as archive…**. The browser uses the bundled, renamed upstream 7-Zip engine to list archive properties or test contents on a background worker, with a 2 GiB input limit, 2 MiB output limit and 120-second timeout. Use **Password…** to supply a password and retry the listing; it is also used for testing and extraction, and an empty password clears it. The password is passed to the engine over stdin rather than in arguments or environment variables, but is held in memory and is not guaranteed to be erased. The engine is not sandboxed; avoid untrusted archives. Listings do not verify content, and tests cannot prove authenticity. When the engine is absent, only uncompressed ustar TAR headers can be inspected.

Right-click selected regular files or folders and choose **Create archive from selection…** to create a .7z, .zip, .tar, .tar.gz, .tar.bz2 or .tar.xz in the current folder. A single regular file can also be compressed as .gz, .bz2 or .xz. **Create encrypted archive from selection…** creates 7z (encrypted headers) or AES-256 ZIP (filenames remain visible); ZIP requires an ASCII password. Passwords are limited to 1–512 UTF-8 bytes without control characters. The operation stages a copy of the selection, rejects links and special files, and limits the tree to 10,000 entries, 128 levels, 512 MiB per file and 1 GiB total. Publication requires hard-link support and never intentionally replaces an existing output. Staging can consume additional disk space; compressed TAR creation stages an intermediate TAR. Cancellation or failure may leave temporary files if cleanup fails, and a crash can leave a staging directory.

**Extract one file…** accepts the exact entry path shown in the report and an output filename in the archive folder. **Extract all…** asks for an existing absolute destination and streams validated regular files one by one into staged, no-replace outputs, creating folders as needed. Extract-all is limited to 1,024 entries, 512 MiB per file and 1 GiB total; partial results remain on error or cancellation. Links, special files and conflicting paths are rejected. Both actions require hard-link support. Use **Extract all…** for a standalone .gz, .bz2 or .xz stream; the output is named after the archive without its compression suffix and is limited to 512 MiB expanded. For compressed TAR, listing and testing stage the expanded TAR in the system temporary directory; extraction stages it in the destination. The expanded TAR is limited to 1,088 MiB and can remain after a crash or failed cleanup. Use only trusted archives and directories that untrusted users cannot modify concurrently; listing and filesystem checks are not a sandbox and do not eliminate races. These new operations have not been verified on Windows.

## Built-in preview

Right-click one file and choose Preview in ZFM. See the README for formats, size and dimension limits, external fallback and decoder security caveats. PDF pages render using bundled PDFium, with previous/next page and zoom controls. Password-protected PDFs prompt for a password; passwords are held in memory and not guaranteed to be erased. The experimental Office layout bridge is not packaged; layout rendering is not available in the distributed application. DOCX, PPTX, XLSX and ODT have a bounded text-only preview; formatting, graphics and other content are omitted. The PDFium builds declare a Windows 10 subsystem, so PDF preview compatibility on Windows 7 is unverified. The new preview and uninstaller changes have not yet been tested on Windows 7.

## Settings storage

Apply in the Settings window stores hidden-file visibility, sort direction, automatic folder refresh, volume-name display, startup compact view and the startup Computer expansion state in `%APPDATA%\zfm\settings.conf`; changes through other controls are session-only until applied in Settings. Uninstall preserves this user file. A malformed or unrecognized settings file is not overwritten; correct or remove it manually to save new settings. Concurrent instances may overwrite each other, and parent paths are not protected against hostile concurrent changes.

## Drive and folder updates

The Computer sidebar enumerates Windows logical drive letters, rather than a fixed set. It checks for drive changes approximately once per second and on device notifications. Mapped, virtual, optical and removable drives reported by Windows can appear even when media is missing or a mapping is disconnected. Scroll the sidebar to see drives below the visible area. Volume labels are queried in background workers and refreshed approximately every 30 seconds; failures or empty labels fall back to the letter. Querying mapped network drives may contact the remote system.

The current folder is polled in the background roughly every three seconds by default. It can be disabled in Settings; Reload still works. Selection and scroll position are preserved where possible. A slow or blocked filesystem can delay background updates, and a blocked poll may prevent later automatic folder scans until it returns. These updates are not guaranteed to be instantaneous. The new behavior should be verified on the target Windows system, including its removable and mapped drives.

Windows cut/paste moves up to 1,024 items on the same volume without replacement. A partial failure leaves earlier moves in place and clears the original cut clipboard if unchanged; reselect remaining items to retry. A clipboard changed by another app is left alone. If clearing fails, a warning is shown and ZFM blocks reusing that clipboard in the current process. See the README for limitations.

F2 or Rename on 2–100 selected regular files in the same folder opens a numbered batch-rename preview. See the README for preflight checks, no-overwrite and partial-result limitations.

**Calculate size…** in the file context menu scans selected files and folders on a cancellable worker; see the README for its limits, reparse-point handling and partial-result caveats.
