<!-- Modified: document locally persisted preferences. -->
# Data flow map

**VERIFIED (source-level only).** User selects paths -> ZFM reads filesystem metadata/content -> requested copy/move/trash, archive or conversion writes to user-selected locations or OS trash. Path copying and file copy/cut use the system clipboard. Opening a file invokes the platform shell or `xdg-open`, which passes it to another application. Network-mounted paths may contact remote storage independently of ZFM's own APIs. Windows setup writes user install files, HKCU keys and shortcuts; its uninstaller uses a temporary copy. Build scripts and package managers can download code/packages. See the retention and third-party caveats in [PRIVACY.md](PRIVACY.md); this map does not prove absence of traffic or tracking by the OS, external programs or modified builds.

Settings Apply writes hidden-file visibility and sort direction, plus Windows folder-refresh, drive-label, startup-view and sidebar preferences, to a per-user configuration file, retained across restarts and uninstall until removed by the user. It does not store document content or navigation history. User-selected folder bookmarks are stored separately in the same per-user configuration directory, retained across restarts and uninstall until removed. Hex encoding is not encryption.

Recursive filename searches scan selected folder trees and hold matching paths and metadata in process memory. Searches can access network-mounted storage and are not persisted by ZFM.

On Windows, ZFM periodically queries logical drives and volume names and, by default, rescans the current folder in the background. OS calls on mapped network drives may contact remote storage. The resulting names and metadata are held in process memory.

Archive engine operations on either platform launch the packaged 7-Zip engine in a child process, passing selected paths. Optional passwords are sent through standard input rather than arguments or environment variables; passwords are held in process memory and are not guaranteed to be erased. Archive creation stages selected files on disk and extraction writes outputs. The engine does not intentionally send archive data over a network; filesystem and operating-system caveats above still apply.

PDF preview reads the selected file into memory and passes it and any supplied password to the bundled PDFium library for local rasterization. Passwords may remain in memory and are not guaranteed to be erased; this is not a sandbox and OS or filesystem access remains outside this description.

Text-only DOCX/PPTX/XLSX/ODT preview streams selected XML members from the bundled archive engine into memory; it does not render document layout or intentionally extract members to the filesystem.

The experimental, currently unpackaged Office layout bridge passes selected document bytes to a local Node/WASM process. It writes the input and resulting PDF into a temporary directory and attempts cleanup; crashes, forced termination or cleanup failures can leave files behind. The runtime is not a security sandbox and may access system fonts and the local environment.
