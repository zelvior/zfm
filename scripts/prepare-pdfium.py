#!/usr/bin/env python3
"""Copy a pinned PDFium library and its upstream notices from a local wheel."""
import hashlib
import pathlib
import sys
import zipfile

TARGETS = {
    'x86_64-unknown-linux-gnu': ('linux-x64.whl', 'dc31aea5b0fb9fefc9b537a65f4113980b2c40798445736a7e6565bc85161e37', 'pypdfium2_raw/libpdfium.so', 'libpdfium.so'),
    'i686-unknown-linux-gnu': ('linux-x86.whl', '397271721330866d1da291042b128595f44cb0314c4675bb439ebaf5c7349b8b', 'pypdfium2_raw/libpdfium.so', 'libpdfium.so'),
    'x86_64-pc-windows-msvc': ('windows-x64.whl', '74d1bace34fb01c660d0b64344d5a4b7d276942b7c6623c88ab59ea21c5acfbd', 'pypdfium2_raw/pdfium.dll', 'pdfium.dll'),
    'i686-pc-windows-msvc': ('windows-x86.whl', '4cfc5ae26b600f92c5be0a0dccb949c10bd8bd3c93e440b75a362f9c1453fd93', 'pypdfium2_raw/pdfium.dll', 'pdfium.dll'),
}

def main():
    if len(sys.argv) != 3 or sys.argv[1] not in TARGETS:
        raise SystemExit('Usage: prepare-pdfium.py TARGET OUTPUT_DIR')
    wheel, digest, member, library = TARGETS[sys.argv[1]]
    root = pathlib.Path(__file__).resolve().parent.parent
    content = (root / 'vendor' / 'pdfium' / wheel).read_bytes()
    if hashlib.sha256(content).hexdigest() != digest:
        raise SystemExit('PDFium wheel checksum mismatch')
    out = pathlib.Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(root / 'vendor' / 'pdfium' / wheel) as archive:
        # Read only known names; never extract arbitrary zip paths.
        lib = archive.read(member)
        if not 1024 < len(lib) < 30 * 1024 * 1024:
            raise SystemExit('Unexpected PDFium library size')
        notice_names = sorted(n for n in archive.namelist() if '.dist-info/licenses/' in n and not n.endswith('/'))
        if not notice_names:
            raise SystemExit('Missing PDFium wheel notices')
        notices = [b'pypdfium2 5.15.2 prebuilt PDFium library, build 8149.\n'
                   b'Source and provenance: https://github.com/pypdfium2-team/pypdfium2\n']
        for name in notice_names:
            data = archive.read(name)
            if len(data) > 1024 * 1024:
                raise SystemExit('Unexpectedly large license file')
            notices.append(('\n--- ' + name.split('.dist-info/licenses/', 1)[1] + ' ---\n').encode('utf-8'))
            notices.append(data)
            notices.append(b'\n')
    (out / library).write_bytes(lib)
    (out / 'pdfium-notices.txt').write_bytes(b''.join(notices))

if __name__ == '__main__':
    main()
