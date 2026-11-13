#!/usr/bin/env bash
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# Assemble tiny ELF executables without depending on 32-bit system libraries.
printf '.global _start\n_start: ret\n' > "$tmp/fixture.s"
as --32 "$tmp/fixture.s" -o "$tmp/32.o"
ld -m elf_i386 "$tmp/32.o" -o "$tmp/32"
as --64 "$tmp/fixture.s" -o "$tmp/64.o"
ld -m elf_x86_64 "$tmp/64.o" -o "$tmp/64"
for spec in '32 i686-unknown-linux-gnu i386' '64 x86_64-unknown-linux-gnu amd64'; do
    read -r bits target arch <<< "$spec"
    "$root/scripts/package-deb.sh" "$tmp/$bits" "$target" v1.2.3-rc_1 "$tmp/$arch.deb" "$tmp/$bits" "$tmp/$bits" "$root/LICENSE"
    [ "$(dpkg-deb -f "$tmp/$arch.deb" Architecture)" = "$arch" ]
    [ "$(dpkg-deb -f "$tmp/$arch.deb" Maintainer)" = "Zelvior Labs" ]
    [ "$(dpkg-deb -f "$tmp/$arch.deb" Version)" = '1.2.3~rc.1' ]
    dpkg-deb -f "$tmp/$arch.deb" Depends | grep -q 'libgtk-3-0 (>= 3.22)'
    dpkg-deb -x "$tmp/$arch.deb" "$tmp/unpacked-$arch"
    cmp "$tmp/$bits" "$tmp/unpacked-$arch/usr/bin/zfm"
    test -x "$tmp/unpacked-$arch/usr/bin/zfm"
    grep -q '^Exec=zfm %f$' "$tmp/unpacked-$arch/usr/share/applications/zfm.desktop"
    test -s "$tmp/unpacked-$arch/usr/share/icons/hicolor/scalable/apps/zfm.svg"
    cmp "$root/LICENSE" "$tmp/unpacked-$arch/usr/share/doc/zfm/copyright"
    grep -q 'Upstream Rust 1.77.2 COPYRIGHT' "$tmp/unpacked-$arch/usr/share/doc/zfm/README"
    grep -q 'The Rust Project is dual-licensed' "$tmp/unpacked-$arch/usr/share/doc/zfm/README"
    python3 - "$root" "$tmp/unpacked-$arch/usr/share/doc/zfm/README" <<'PYTEST'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
readme = pathlib.Path(sys.argv[2]).read_bytes()
for name in ('COPYRIGHT', 'LICENSE-MIT', 'LICENSE-APACHE'):
    assert (root / 'LICENSES' / 'rust-1.77.2' / name).read_bytes() in readme, name
PYTEST
done
if "$root/scripts/package-deb.sh" "$tmp/64" i686-unknown-linux-gnu v1.0 "$tmp/wrong.deb" "$tmp/64" "$tmp/64" "$root/LICENSE"; then
    echo 'Architecture mismatch was not rejected' >&2; exit 1
fi
if "$root/scripts/package-deb.sh" "$tmp/64" x86_64-unknown-linux-gnu 'v1 bad' "$tmp/bad.deb" "$tmp/64" "$tmp/64" "$root/LICENSE"; then
    echo 'Invalid tag was not rejected' >&2; exit 1
fi
echo 'Debian package tests passed.'
