# Privacy: observed design and unresolved operator duties

**NEEDS REVIEW.** This describes the inspected source, not a blanket guarantee for every build or external service. No explicit analytics, telemetry, remote update client, or network API was identified in the application source reviewed. This does not prove absence of all network traffic: network-mounted paths and OS facilities can communicate, and third-party software acts independently.

| Data / action | Destination and retention |
|---|---|
| User-selected files, filenames, metadata and directory paths | Read or modified on selected local or mounted storage; archive/conversion outputs and temporary staging files may remain after a crash/failure. User controls paths; no universal deletion schedule. |
| Navigation history | In process memory (`src/navigation.rs`); OS, desktop or applications may separately retain histories or logs. |
| Clipboard | File paths and copy/cut metadata or text go to the OS clipboard (`file_clipboard_ui.rs`, `windows_actions.rs`); clipboard managers/other applications may access or retain them. |
| Open/trash | `xdg-open` or Windows shell and the associated app receive selected paths; Linux trash/Windows recycle bin is managed by system components. Review their policies. |
| Installer | Windows writes application files, HKCU registration and shortcuts, and launches a temporary uninstaller copy; OS security logs and backups are outside project control. Linux package manager records installations. |
| Build/test | Rust build scripts may download and execute rustup; apt, GitHub Actions and package managers contact third parties. Tests may invoke Python, tar and image tools. |
| Support | GitHub issues, any future private reporting channel and attachments go to their operator; public issues are publicly visible. Never post sensitive data unnecessarily. |

**ACTION REQUIRED:** identify the actual operator/controller and verified contact, purposes and lawful bases if applicable, retention/access/deletion process and applicable rights; assess Pakistan and target jurisdictions with counsel. Do not claim zero collection, universal GDPR compliance, or control over OS/third-party practices. Avoid sending files or sensitive paths in reports. Privacy policy needs revision if telemetry, services, accounts or hosted distribution change.
