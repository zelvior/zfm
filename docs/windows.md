# Windows development notes

The application has an experimental native Win32 browser painted in the same black-and-white palette as the Linux interface (white surfaces, near-black ink, solid black primary buttons and a solid black list selection) with navigation, tabs, filename filtering, hidden-file toggle, file/folder creation, rename and confirmed permanent deletion of files or empty folders. Enter a single name in the New name field for creation or rename; operations run in a worker, do not intentionally replace existing destinations, and permanent deletion has no trash fallback. Clipboard, copying, moving, trash, archives, converter UI and many advanced features are still absent. Windows MSVC 32-bit and 64-bit builds, core tests and GUI startup smoke tests passed on GitHub's Windows Server 2022 runners for v0.0.2. The black-and-white interface and the rewritten installer remain unverified on Windows. These commands require a suitable toolchain and do not establish compatibility.

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
- The wizard uses the ZFM black-and-white interface and offers welcome, folder, license, progress and finish pages. `/S` installs silently and `/D=<folder>` chooses the folder. A silent run never opens a dialog, not even for bad arguments: it reports through the exit code (0 success, 1 failure, 2 bad usage).
- Setup installs for the current user in `%LOCALAPPDATA%\Programs\ZFM` by default, writes the Start Menu shortcuts and the Programs and Features entry under `HKCU`, and copies itself to `Uninstall.exe` next to the application.
- Uninstall removes exactly the files setup created. Other files in the folder are kept, and the folder itself is removed only when empty. Windows cannot delete a running executable, so the uninstaller copies itself to `%TEMP%`, closes immediately and lets the copy remove the rest, including the original uninstaller; when that is impossible the file is deleted at the next restart.
- Setup refuses to replace unrelated files: reinstalling into the recorded folder upgrades in place, while any other folder that already contains a managed file is rejected.

Build it after the application for the same architecture:

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
$env:ZFM_APP_BINARY = (Resolve-Path "target/x86_64-pc-windows-msvc/release/zfm.exe").Path
$env:ZFM_VERSION = "v0.0.3"
cargo build --release --locked --target x86_64-pc-windows-msvc -p zfm-installer
```

`ZFM_APP_BINARY` is required at build time; without it the package builds a placeholder payload so `cargo test -p zfm-installer` still runs. The installer unit tests (`cargo test -p zfm-installer`) cover option parsing, folder selection, shortcut COM initialization on worker threads and the file install/remove rules. The Windows jobs of CI and of the release workflow build setup for both architectures and run `tests/installer-windows.ps1`, which exercises both silent setup and the interactive wizard pages, checks the files, the Start Menu shortcuts and the Programs and Features entry, upgrades in place, recovers an incomplete installation, uninstalls, verifies that user files survive and that an emptied folder is removed, and checks `/UNINSTALL`. Windows 7 SP1 has not yet been exercised in CI.
