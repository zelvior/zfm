# Incremental development and compatibility plan

This document records the first increment towards the larger cross-platform
roadmap, **not completion**. The existing code and tests remain authoritative.
Do not publish a new release as though the full feature request were done.

## Baseline and risks

The workspace has native GTK 3 and Win32 frontends, shared Rust file operations,
a Linux-specific archive backend, and a separate Win32 installer. No third-party
Rust packages were added for this increment. Linux reads GTK and libarchive at
runtime. The archive backend's Linux no-follow directory-relative operations
cannot simply be reused on Windows; a Windows implementation needs its own
reviewed path and reparse-point protections. Recursive operations and external
converters need explicit conflict, cancellation, traversal, ownership and
partial-result policies before being exposed. Existing lists retain entries and
are not virtualized; a blocked filesystem call cannot always be cancelled.

Before changes, locally on Linux x86_64 with Rust 1.88, 49 core and 14 installer
unit tests passed, Linux release build passed, and Windows MSVC x86/x64 targets
passed `cargo check`. Build-script, Debian package and release-tag tests also
passed. These checks are not Windows runtime tests or legacy OS verification.
CI uses Rust 1.77.2, and tests real Windows builds and the GTK smoke workflow.

## Current conversion matrix

All entries below are built-in, offline and use no external process. Text input is
UTF-8; output must not already exist and the destination filesystem must support
hard links. Conversion stages output and cleans up on normal errors and
cancellation; forced termination can leave a temporary file. Avoid directories
writable by untrusted users. File content is parsed; a change of extension alone
is never presented as conversion.

| Input | Output | Linux UI | Windows UI | Constraints |
|---|---|---|---|---|
| CSV | JSON | Converter window or context menu | Context menu | Header keys unique/nonempty; all values are strings. |
| JSON | CSV | Converter window or context menu | Context menu | Nonempty array of objects, identical nonempty unique keys, string values only. |
| TXT | HTML | Converter window or context menu | Context menu | Plain text escaped into a `pre` element; valid UTF-8 and supported ASCII controls only; not a Markdown renderer. |
| BMP | ICO | Converter window or context menu | Context menu | Uncompressed bottom-up 24-bit BITMAPINFOHEADER only, 1–256 pixels per side; creates one opaque icon. |
| ICO | BMP | Converter window or context menu | Context menu | One opaque, 24-bit BMP-based icon with a zero AND mask; no PNG payloads, alpha, palettes or multiple images. |
| WAV | AIFF | Converter window or context menu | Context menu | PCM, 8/16-bit, mono/stereo, integer 1–192000 Hz; metadata omitted, input ≤512 MiB. |
| AIFF | WAV | Converter window or context menu | Context menu | Same PCM constraints; no AIFF-C, metadata omitted, input ≤512 MiB. |

The core is tested on Linux and in Windows CI for both supported MSVC
architectures. Linux has progress and cancellation. The Windows context menu
is a first increment: output is limited to a filename in the input folder and
there is no progress indicator; cancellation is available through the file-list context menu. Other formats are not
supported or advertised: PNG/JPEG/GIF/TIFF/WebP; Markdown/XML/PDF;
MP3/FLAC/OGG/AAC; MP4/MKV/AVI/WebM/MOV. FFmpeg integration is not yet
implemented. Other BMP and ICO variants are not supported; use an image editor to convert them to an opaque 24-bit BMP first. Image input is limited to 1 MiB. No universal bidirectional support is implied.

## Performance baseline procedure

On a Linux X11 desktop (or with `xvfb-run`, GTK 3 and `xdotool` installed):

```sh
cargo build --release --locked
xvfb-run -a python3 tests/benchmark-linux.py "$PWD/target/release/zfm"
```

The script creates 1,000 empty files and measures time to the first visible
window, process current and peak RSS, and CPU seconds across startup plus two
seconds. It is run in Linux CI and prints the measured values there. Repeat it
several times on the target machine and record OS, architecture, Rust version,
filesystem, window system and cold/warm cache conditions before comparing.
It is a narrow baseline, not a large-directory, long-running, or real-user
performance guarantee. Windows and Mint 19 measurements remain outstanding.

## Remaining phases

1. Expand converter formats only after selecting compatible implementations and
   documenting tested input/output pairs, resource bounds and optional tools.
2. Add archive formats and Windows support with equivalent security tests;
   multi-file/folder creation, conflict resolution, encryption, and standalone
   compression are not implemented yet.
3. Expand beyond bounded multi-item clipboard copying to bulk moves and deletes,
   cross-volume moves, batch rename, improved clipboard behavior, drag-and-drop, recent files, directory
   sizes and previews with cancellation and safe partial-failure handling.
4. Measure and address directory virtualization, worker limits and notifications.
5. Close UI parity gaps, test on actual Windows 7 and Mint 19 in both
   architectures, and validate packaging before preparing a release.

Run tests and maintain a working build after each increment. Do not infer
compatibility merely from cross-compilation or dependency declarations.
