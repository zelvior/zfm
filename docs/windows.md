# Windows development notes

The application has an experimental native Win32 browser with navigation, tabs, filename filtering, hidden-file toggle, file/folder creation, rename and confirmed permanent deletion of files or empty folders. Enter a single name in the New name field for creation or rename; operations run in a worker, do not intentionally replace existing destinations, and permanent deletion has no trash fallback. Clipboard, copying, moving, trash, archives, converter UI and many advanced features are still absent. Windows MSVC 32-bit and 64-bit builds, core tests and GUI startup smoke tests passed on GitHub's Windows Server 2022 runners for v0.0.2. The newer interface and installer remain unverified on Windows. These commands require a suitable toolchain and do not establish compatibility.

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

The workflow can build per-user NSIS setup executables for each architecture. They add a Start Menu shortcut and an uninstall entry; they install only into the selected folder and do not delete unrelated files during uninstall. The setup downloads no external runtime. Its build and install/uninstall smoke tests have not yet run on GitHub or Windows 7. The existing v0.0.2 release has portable binaries only.
