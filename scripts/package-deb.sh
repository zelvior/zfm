#!/usr/bin/env bash
# Package an already-built legacy-compatible executable. No downloads or root needed.
set -euo pipefail
if [ "$#" -ne 4 ]; then
    echo 'Usage: package-deb.sh BINARY TARGET VTAG OUTPUT.deb' >&2; exit 2
fi
binary=$1; target=$2; tag=$3; output=$4
case "$target" in
    x86_64-unknown-linux-gnu) arch=amd64; expected='Advanced Micro Devices X86-64' ;;
    i686-unknown-linux-gnu) arch=i386; expected='Intel 80386' ;;
    *) echo "Unsupported target: $target" >&2; exit 2 ;;
esac
[[ $tag =~ ^v[0-9A-Za-z][0-9A-Za-z._+-]*$ ]] || { echo 'Invalid vtag' >&2; exit 2; }
version=${tag#v}
version=${version//_/.}
version=${version//-/\~}
[[ $version =~ ^[0-9] ]] || version="0~$version"
dpkg --validate-version "$version"
actual=$(LC_ALL=C readelf -h "$binary" | sed -n 's/^[[:space:]]*Machine:[[:space:]]*//p')
[ "$actual" = "$expected" ] || { echo "Wrong executable architecture: expected $expected, got $actual" >&2; exit 1; }
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/applications" \
    "$stage/usr/share/icons/hicolor/scalable/apps" "$stage/usr/share/doc/zfm"
install -m 755 "$binary" "$stage/usr/bin/zfm"
install -m 644 "$root/packaging/zfm.desktop" "$stage/usr/share/applications/zfm.desktop"
install -m 644 "$root/packaging/zfm.svg" "$stage/usr/share/icons/hicolor/scalable/apps/zfm.svg"
install -m 644 "$root/README.md" "$stage/usr/share/doc/zfm/README"
install -m 644 "$root/LICENSE" "$stage/usr/share/doc/zfm/copyright"
cat > "$stage/DEBIAN/control" <<CONTROL
Package: zfm
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: Zelvior Labs
Depends: libc6 (>= 2.27), libgtk-3-0 (>= 3.22), libarchive13, xdg-utils
Description: lightweight native file manager
 ZFM is a native desktop file manager with navigation and file operations.
CONTROL
# dpkg-deb --root-owner-group is available in Ubuntu 18.04's dpkg 1.19.
mkdir -p "$(dirname "$output")"
dpkg-deb --root-owner-group --build "$stage" "$output"
