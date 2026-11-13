<!-- Modified: document local settings retention. -->
# Privacy: observed design and unresolved operator duties

**NEEDS REVIEW.** This describes the inspected source, not a blanket guarantee for every build or external service. No explicit analytics, telemetry, remote update client, or network API was identified in the application source reviewed. This does not prove absence of all network traffic: network-mounted paths and OS facilities can communicate, and third-party software acts independently.

| Data / action | Destination and retention |
|---|---|
| User-selected files, filenames, metadata and directory paths | Read or modified on selected local or mounted storage; archive/conversion outputs and temporary staging files may remain after a crash/failure. User controls paths; no universal deletion schedule. |
| User settings | Hidden-file visibility and sort direction are stored in a per-user configuration file when Apply is used. The file remains after uninstall; see the README for its location. |
| Navigation history | In process memory (`src/navigation.rs`); OS, desktop or applications may separately retain histories or logs. |
| Clipboard | File paths and copy/cut metadata or text go to the OS clipboard (`file_clipboard_ui.rs`, `windows_actions.rs`); clipboard managers/other applications may access or retain them. |
| PDF preview passwords | A supplied PDF password is held in application memory while the document is open and passed to PDFium; it is not guaranteed to be erased. |
| Text-only Office preview | Selected DOCX/PPTX/XLSX/ODT paths are passed to the bundled archive engine and selected XML data is read into memory. Formatting and many other elements are not rendered. |
| Experimental Office layout bridge | If a separately installed experimental runtime is present, document bytes and converted PDF are written to a temporary directory. Cleanup is attempted but a crash, forced cancellation, or cleanup failure can leave files behind. The runtime and PDFium are not sandboxed; only use trusted documents. The runtime is not included in the current packages. |
| PDF preview | The selected document is read locally into memory and passed to the bundled PDFium library; no network call is intentionally made by ZFM for preview. PDF rendering is not sandboxed. |
| Archive operations and passwords | Passes selected archive paths to the bundled 7-Zip engine in a child process. Optional passwords are sent via its standard input, not in command-line arguments or environment variables; they are held in process memory and are not guaranteed to be erased. Creation stages selected data on disk; extraction writes chosen outputs. |
| Open/trash | `xdg-open` or Windows shell and the associated app receive selected paths; Linux trash/Windows recycle bin is managed by system components. Review their policies. |
| Installer | Windows writes application files, HKCU registration and shortcuts, and launches a temporary uninstaller copy; OS security logs and backups are outside project control. Linux package manager records installations. |
| Build/test | Rust build scripts may download and execute rustup; apt, GitHub Actions and package managers contact third parties. Tests may invoke Python, tar and image tools. |
| Support | GitHub issues, any future private reporting channel and attachments go to their operator; public issues are publicly visible. Never post sensitive data unnecessarily. |

**ACTION REQUIRED:** identify the actual operator/controller and verified contact, purposes and lawful bases if applicable, retention/access/deletion process and applicable rights; assess Pakistan and target jurisdictions with counsel. Do not claim zero collection, universal GDPR compliance, or control over OS/third-party practices. Avoid sending files or sensitive paths in reports. Privacy policy needs revision if telemetry, services, accounts or hosted distribution change.
