# Linux build and compatibility notes

The Linux frontend uses GTK 3's C ABI, dynamically loading `libgtk-3.so.0` and resolving the required symbols at startup. This allows core tests and builds without GTK development files, but a GTK 3 runtime and a usable display are required to run the GUI. Missing GTK or display produces a diagnostic and a nonzero exit. `xdg-open` is used to open files. There is no daemon.

See the README for installation, build, cross-architecture and packaging commands. The tarball is not a statically linked binary; the target still needs compatible libc, GTK 3, desktop integration and their transitive dependencies. Build and test on the oldest supported system to avoid inadvertently requiring newer glibc symbols. No claim is made that a binary built on Debian 12 runs on Mint 19.

The application creates a worker for each requested directory read. A generation counter discards stale results and stops iteration between entries, but cannot cancel a blocked kernel call. Rows are added 64 at a time to keep the main loop responsive; they are not virtualized, and reads, metadata and sorting can still be costly for large or remote directories. No profiling, GUI responsiveness, network drive or external drive testing has been performed yet.

File operations are deliberately limited: no-overwrite rename/move require Linux `renameat2` support, copying/duplicating is restricted to regular files, and deletion requires confirmation and handles only individual files, symlinks, or empty directories. Copy stages in the target directory, so a destination writable by an untrusted user is not safe against concurrent tampering. See the README for the full limitations. GUI actions have not been interactively tested.

Trash delegates to GIO and reports failures; it never falls back to permanent deletion. Built-in archive extraction is limited to uncompressed TAR on Linux x86/x86_64/aarch64. It rejects links and unsafe paths, uses no-follow directory-relative file operations, and applies resource limits. See the README for the exact supported TAR subset, partial-output semantics, and untrusted-directory restrictions.
