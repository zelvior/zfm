#!/usr/bin/env bash
# Runs as root in Ubuntu 18.04, invoked by the release workflow.
set -euxo pipefail
TARGET=$1
TAG=$2
case "$TARGET" in x86_64-unknown-linux-gnu|i686-unknown-linux-gnu) ;; *) echo 'Unexpected target' >&2; exit 2;; esac
export DEBIAN_FRONTEND=noninteractive
dpkg --add-architecture i386
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl build-essential gcc-multilib g++-multilib libc6-dev-i386 \
    libarchive13 libarchive13:i386 libglib2.0-0 libglib2.0-0:i386 \
    libgtk-3-0 libgtk-3-0:i386 librsvg2-common librsvg2-common:i386 \
    shared-mime-info adwaita-icon-theme python3 xvfb xauth xdotool xdg-utils
curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain 1.77.2
export PATH="/root/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=/tmp/zfm-target
rustup target add --toolchain 1.77.2 "$TARGET"
rustc +1.77.2 --version
cargo +1.77.2 --version
ZFM_REQUIRE_ARCHIVE_TESTS=1 cargo +1.77.2 test --locked --target "$TARGET"
cargo +1.77.2 build --release --locked --target "$TARGET"
binary="$CARGO_TARGET_DIR/$TARGET/release/zfm"
# Fail instead of shipping an executable requiring a glibc newer than Mint 19.
new_glibc=$(objdump -T "$binary" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | awk -F '[_.]' '$2 > 2 || ($2 == 2 && $3 > 27)')
if [ -n "$new_glibc" ]; then echo "Unsupported glibc requirements: $new_glibc" >&2; exit 1; fi
xvfb-run -a bash tests/gui-smoke-linux.sh "$binary"
mkdir -p "build-output/$TARGET"
cp "$binary" "build-output/$TARGET/zfm"
bash tests/package-deb.sh
engine="build-output/$TARGET/zfm-archive-engine"
bash scripts/build-archive-engine.sh "$TARGET" "$engine"
new_glibc=$(objdump -T "$engine" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | awk -F '[_.]' '$2 > 2 || ($2 == 2 && $3 > 27)')
if [ -n "$new_glibc" ]; then echo "Unsupported engine glibc requirements: $new_glibc" >&2; exit 1; fi
python3 scripts/prepare-pdfium.py "$TARGET" "build-output/$TARGET/pdfium"
bash scripts/package-deb.sh "$binary" "$TARGET" "$TAG" "build-output/$TARGET/zfm.deb" "$engine" "build-output/$TARGET/pdfium/libpdfium.so" "build-output/$TARGET/pdfium/pdfium-notices.txt"
# Exercise the real package's installation, launch, and uninstallation in the
# disposable container. Multiarch runtimes are installed above for i686.
dpkg -i "build-output/$TARGET/zfm.deb"
dpkg-query -W zfm
xvfb-run -a bash tests/gui-smoke-linux.sh /usr/bin/zfm
dpkg -r zfm
if [ -e /usr/bin/zfm ] || [ -e /usr/share/applications/zfm.desktop ]; then
    echo 'Package removal left installed files behind' >&2; exit 1
fi
