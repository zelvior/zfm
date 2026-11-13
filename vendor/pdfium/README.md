# PDFium library builds

The four wheels are unmodified PyPI artifacts for pypdfium2 5.15.2. Each
contains PDFium build 8149, built by the pypdfium2 maintainers. ZFM uses only
`pypdfium2_raw/libpdfium.so` or `pypdfium2_raw/pdfium.dll`, not Python or the
pypdfium2 bindings. `scripts/prepare-pdfium.py` verifies pinned SHA-256 values,
extracts the appropriate library and collects the accompanying upstream
notices for distribution. All four wheels retain their original metadata and
license files here. Source and build information:
https://github.com/pypdfium2-team/pypdfium2 and https://pdfium.googlesource.com/pdfium/.

The current Windows libraries declare Windows 10 as their minimum subsystem;
PDF preview compatibility on Windows 7 has not been verified. Linux wheels target glibc
2.17. These prebuilt libraries and every third-party license still require
release review; see LEGAL/THIRD_PARTY.md.
