# Windows development notes

The current application does not have a Windows GUI; the non-Linux executable prints an unsupported message. No Windows build or runtime has been verified. The following commands build the current source, **not a usable Windows file manager**.

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

Use the appropriate MinGW toolchain for both architectures and configure Cargo linkers if they are not automatically found. A future native Windows adapter and its dependencies must be built and tested on each supported OS and architecture; modern Rust/toolchains may not support Windows 7. Do not distribute the present stub as a Windows release. A future release should include the executable, license, and any required runtime libraries, so end users do not need Rust.
