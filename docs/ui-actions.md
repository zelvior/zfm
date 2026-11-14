# UI action map

ZFM exposes its implemented end-user operations in its native Linux and Windows interfaces; this is not a claim of full Explorer parity. The toolbar menu is **Organize** on Linux and **Actions** on Windows (Ctrl+Shift+O). For file-specific actions, select one or more rows first; on Linux, open **Selected items** in Organize. Right-clicking a row opens the same selection-aware item actions. Menu entries depend on the selection and supported format, and long operations run in the background unless noted below.

| Feature | Linux | Windows |
|---|---|---|
| Browse, back/forward/up, tabs, search, sort and hidden files | Toolbar, sidebar, keyboard, row/background and tab menus | Toolbar, sidebar, keyboard, row/background and tab menus |
| Search subfolders; case sensitivity | Organize / Ctrl+Shift+F; **Case** toggle | Actions / Ctrl+Shift+F; **Case-sensitive search** in Actions (not saved between sessions) |
| Bookmarks | Sidebar Bookmarks menu, Ctrl+B | Actions → Bookmarks, Ctrl+B |
| Create, copy, cut, paste, move, duplicate, rename, trash, permanent delete | Organize and selected-item menu, plus shortcuts | Actions, selection menu, toolbar and shortcuts |
| Preview and properties | Selected-item menu, Ctrl+P / Alt+Enter | Actions or selection menu, Ctrl+P / Alt+Enter |
| Calculate selected size; cancel work | Selected-item menu / Ctrl+Shift+I; Organize → Cancel operation | Actions / Ctrl+Shift+I; Actions → Cancel current operation while busy |
| Built-in file conversion | Selected supported file menu, or Converter sidebar/Organize window with mode selector | Select a supported file → Actions → Convert; WAV has separate AIFF and AU options |
| Archive browse, test, create and extract | Selected archive/item menu and Archives sidebar/Archive manager | Select archive/item → Actions; archive browser has test, extract one and extract all |
| Settings | Organize / Ctrl+, | Actions / Ctrl+, |
| Package uninstall | Organize → Uninstall ZFM (Debian package via system tools) | Installed uninstaller via Windows settings/Start Menu |

The conversion matrix and archive, preview, cancellation, clipboard and filesystem safety limitations are detailed in the [README](../README.md). Availability depends on platform, file type and installed/bundled components. For example, Office text preview is limited, the experimental layout bridge is not packaged, and external tools may be required. Low-level libraries are not independently exposed as UI commands, nor are unimplemented features such as drag-and-drop, cross-volume moves or general undo. Do not treat this map as a compatibility or completeness guarantee.
