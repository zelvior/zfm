#!/usr/bin/env bash
# Build the pinned upstream standalone archive engine, with no network access.
set -euo pipefail
[ "$#" -eq 2 ] || { echo 'Usage: build-archive-engine.sh TARGET OUTPUT' >&2; exit 2; }
case "$1" in
    x86_64-unknown-linux-gnu) arch=-m64; machine='Advanced Micro Devices X86-64';;
    i686-unknown-linux-gnu) arch=-m32; machine='Intel 80386';;
    *) echo 'Unsupported Linux target' >&2; exit 2;;
esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source="$root/vendor/7zip/7zip-d3296e39.tar.gz"
echo 'adef737a73a2668d26c0733e496e1600662af2a8779e084e6475a0162e012576  '"$source" | sha256sum -c -
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tar -xzf "$source" -C "$tmp" --strip-components=1
make -C "$tmp/CPP/7zip/Bundles/Alone2" -f makefile.gcc -j"${ZFM_BUILD_JOBS:-2}" MY_ARCH="$arch" LDFLAGS_STATIC_2='-static-libstdc++ -static-libgcc'
engine="$tmp/CPP/7zip/Bundles/Alone2/_o/7zz"
actual=$(LC_ALL=C readelf -h "$engine" | sed -n 's/^[[:space:]]*Machine:[[:space:]]*//p')
[ "$actual" = "$machine" ] || { echo "Wrong engine architecture: $actual" >&2; exit 1; }
mkdir -p "$(dirname "$2")"
install -m 755 "$engine" "$2"
