# Windows development notes

The experimental Win32 browser includes multiple row selection, row/background/tab/sidebar context menus, common navigation and file shortcuts, clipboard copy/paste for multiple items and single-item cut, rename and create dialogs, Properties, and Recycle Bin requests. Other file operations remain single-item and reject multiple selections. Copy accepts up to 1,024 regular files or bounded directory trees, stops on the first failure with earlier copies retained, rejecting links, reparse points and special files; move is same-volume and never overwrites. Plain Delete asks the Windows shell to recycle on fixed drives; remote/removable drives and reparse points are rejected. The shell may warn and ask for confirmation if recycling is unavailable. Shift+Delete confirms permanent deletion and only removes files or empty directories. Clipboard, dialogs and Recycle Bin behavior should still be tested on target Windows versions; Windows 7 has not been exercised in CI.

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

- The setup executable embeds the release `zfm.exe`, `zfm.ico`, the license and the README at build time, so it needs no network access and no extra archive.
- The wizard uses ZFM's custom-rendered slate-and-metallic setup interface and offers welcome, folder, license, progress and finish pages. `/S` installs silently and `/D=<folder>` chooses the folder. A silent run never opens a dialog, not even for bad arguments: it reports through the exit code (0 success, 1 failure, 2 bad usage).
- Setup installs for the current user in `%LOCALAPPDATA%\Programs\ZFM` by default, writes the Start Menu shortcuts and the Programs and Features entry under `HKCU`, and copies itself to `Uninstall.exe` next to the application.
- Uninstall removes exactly the files setup created. Other files in the folder are kept, and the folder itself is removed only when empty. Windows cannot delete a running executable, so the uninstaller copies itself to `%TEMP%`, closes immediately and lets the copy remove the rest, including the original uninstaller; when that is impossible the file is deleted at the next restart.
- Setup refuses to replace unrelated files: reinstalling into the recorded folder upgrades in place, while any other folder that already contains a managed file is rejected. Identical installed files are not rewritten, so reinstalling the same version can succeed while ZFM is running.
- If a changed file is in use or access is denied, the wizard explains the possible causes. If ZFM from the target installation has an identifiable open window, setup asks permission to close it gracefully and retry; declining leaves it running. Setup never force-terminates processes and never closes ZFM installations from other paths. Other locks or permission errors offer Retry or Cancel. If closing ZFM does not help, check other programs, folder permissions or security software, or choose another writable folder. A silent `/S` run never closes ZFM or prompts and returns exit code 1 on failure; retry it after resolving the lock. Replacement files, including the uninstaller, are staged before publication, and a failed replacement leaves the prior file intact.

Build it after the application for the same architecture:

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
$env:ZFM_APP_BINARY = (Resolve-Path "target/x86_64-pc-windows-msvc/release/zfm.exe").Path
$env:ZFM_VERSION = "v0.0.3"
cargo build --release --locked --target x86_64-pc-windows-msvc -p zfm-installer
```

`ZFM_APP_BINARY` is required at build time; without it the package builds a placeholder payload so `cargo test -p zfm-installer` still runs. The installer unit tests (`cargo test -p zfm-installer`) cover option parsing, folder selection, shortcut COM initialization on worker threads and the file install/remove rules. The Windows jobs of CI and of the release workflow build setup for both architectures and run `tests/installer-windows.ps1`, which exercises both silent setup and the interactive wizard pages, checks the files, the Start Menu shortcuts and the Programs and Features entry, upgrades in place, recovers an incomplete installation, uninstalls, verifies that user files survive and that an emptied folder is removed, and checks `/UNINSTALL`. Windows 7 SP1 has not yet been exercised in CI.

### Program Compatibility Assistant and publisher identity

The setup and uninstaller embed an `asInvoker` manifest with Windows 7 and later compatibility declarations. This is intended to avoid Windows 7's false “might not have installed correctly” installer-detection warning, without changing or disabling Windows security settings. The setup still reports real failures; a compatibility declaration is not proof of successful installation or of Windows 7 compatibility. Check that ZFM appears in Programs and Features and launches before selecting “This program installed correctly.” If setup failed, resolve that error and run setup again rather than selecting that option. Windows 7 testing is still needed to confirm the warning is resolved on the target system.

The executable's version metadata identifies the product as ZFM Setup by Zelvior Labs, and Programs and Features lists Zelvior Labs as publisher. This is descriptive metadata, **not a digital signature**: an unsigned installer can still show “Unknown publisher” in security prompts. A trusted code-signing certificate and signing the release binaries would be required for a verified publisher. Existing downloaded installers cannot acquire these changes; they require a new release build. Building the installer for Windows GNU additionally requires the matching `i686-w64-mingw32-windres` or `x86_64-w64-mingw32-windres` on PATH.

## Built-in converter

Right-click a single CSV, JSON, TXT, BMP or ICO file and choose **Convert file (built-in)**. CSV converts to JSON, string-valued JSON objects convert to CSV, UTF-8 text converts to escaped HTML, and opaque 24-bit BMP and ICO images up to 256 × 256 convert in either direction. Multi-image, PNG-backed, transparent, paletted and other icon/bitmap variants are not supported. Enter a new filename in the source folder. The operation runs in the background and reports completion or errors in the status bar; it cannot currently be cancelled or show detailed progress on Windows. Existing output files are never intentionally replaced, and the destination filesystem must support hard links. See the [conversion matrix](development-plan.md) for exact scope. No FFmpeg or other external converter is integrated.
