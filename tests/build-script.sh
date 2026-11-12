#!/usr/bin/env bash
# Fast integration tests: mock system toolchains, network and Cargo.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/project/scripts" "$tmp/bin" "$tmp/home"
cp "$repo/scripts/build-linux.sh" "$tmp/project/scripts/build-linux.sh"
for cmd in bash dirname mkdir mktemp rm cp env cat find wc grep; do ln -s "$(command -v "$cmd")" "$tmp/bin/$cmd"; done
cat > "$tmp/bin/uname" <<'MOCK'
#!/bin/sh
case "$1" in -s) echo Linux;; -m) echo x86_64;; esac
MOCK
cat > "$tmp/bin/cc" <<'MOCK'
#!/bin/sh
[ "${MOCK_NO_32:-0}" != 1 ] || [ "${1:-}" != -m32 ]
MOCK
for linker in x86_64-w64-mingw32-gcc i686-w64-mingw32-gcc; do ln -s cc "$tmp/bin/$linker"; done
cat > "$tmp/bin/rustup" <<'MOCK'
#!/bin/bash
set -eu
printf '%s\n' "$*" >> "$MOCK_LOG"
case "$1" in
    toolchain|target) exit 0 ;;
    run) shift 2 ;;
    *) exit 1 ;;
esac
case "$1" in
    rustc) echo 'rustc mock';;
    cargo)
        shift
        case "$1" in
            --version) echo 'cargo mock';;
            test) [ "${MOCK_TEST_FAIL:-0}" = 0 ] ;;
            build)
                target=''
                while [ "$#" -gt 0 ]; do if [ "$1" = --target ]; then shift; target=$1; fi; shift; done
                [ "${MOCK_FAIL_TARGET:-}" != "$target" ] || exit 1
                mkdir -p "$CARGO_TARGET_DIR/$target/release"
                name=zfm; case "$target" in *-windows-*) name=zfm.exe;; esac
                printf 'mock executable\n' > "$CARGO_TARGET_DIR/$target/release/$name" ;;
            *) exit 1;;
        esac ;;
    *) exit 1 ;;
esac
MOCK
chmod +x "$tmp/bin/uname" "$tmp/bin/cc" "$tmp/bin/rustup"
export PATH="$tmp/bin" HOME="$tmp/home" CARGO_HOME="$tmp/home/.cargo" MOCK_LOG="$tmp/log"
script="$tmp/project/scripts/build-linux.sh"
"$script" --native > "$tmp/native.log"
[ "$(find "$tmp/project/artifacts" -name zfm -type f | wc -l)" -eq 1 ]
# Verify target and requested toolchain are passed through.
"$script" --toolchain 1.85.0 --skip-tests > "$tmp/all.log"
grep -q 'toolchain install 1.85.0 --profile minimal' "$MOCK_LOG"
for target in x86_64-unknown-linux-gnu i686-unknown-linux-gnu x86_64-pc-windows-gnu i686-pc-windows-gnu; do
    grep -q "$target" "$tmp/all.log"
done
[ "$(find "$tmp/project/artifacts" -name zfm.exe -type f | wc -l)" -eq 2 ]
# Linux-only mode never requests a Windows target.
"$script" --linux-only --skip-tests > "$tmp/linux-only.log"
if grep -q 'pc-windows' "$tmp/linux-only.log"; then echo 'Unexpected Windows target' >&2; exit 1; fi
if "$script" --native --linux-only > /dev/null 2>&1; then echo 'Expected conflicting arguments failure' >&2; exit 1; fi
# A failed target must produce nonzero status, report failure, and keep successes.
if MOCK_NO_32=1 MOCK_FAIL_TARGET=x86_64-pc-windows-gnu "$script" --skip-tests > "$tmp/fail.log" 2>&1; then echo 'Expected partial failure' >&2; exit 1; fi
grep -q 'i686-unknown-linux-gnu (linker test failed)' "$tmp/fail.log"
grep -q 'x86_64-pc-windows-gnu (build failed)' "$tmp/fail.log"
grep -q 'i686-pc-windows-gnu/zfm.exe' "$tmp/fail.log"
# A failing test suite prevents publishing a new build.
count=$(find "$tmp/project/artifacts" -mindepth 1 -maxdepth 1 -type d | wc -l)
if MOCK_TEST_FAIL=1 "$script" --native > "$tmp/testfail.log" 2>&1; then echo 'Expected test failure' >&2; exit 1; fi
[ "$(find "$tmp/project/artifacts" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq "$count" ]
if "$script" --invalid > /dev/null 2>&1; then echo 'Expected invalid argument failure' >&2; exit 1; fi
echo 'Build script integration tests passed.'
