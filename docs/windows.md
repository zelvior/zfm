# Windows development notes

The application has an experimental native Win32 browser: directory listing, back/forward/up, address, refresh and opening files. It does not yet provide Linux feature parity (including operations, tabs, clipboard, archives or converter UI). The Windows GNU 32-bit and 64-bit targets have been type-checked only; neither executable has been linked or run. These commands require a suitable toolchain and do not establish compatibility.

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

Use the appropriate MinGW toolchain for both architectures and configure Cargo linkers if they are not automatically found. The native Windows adapter must be linked and tested on each supported OS and architecture; modern Rust/toolchains may not support Windows 7. Do not distribute this untested browser as a Windows release. A future release should include the executable, license, and any required runtime libraries, so end users do not need Rust.
