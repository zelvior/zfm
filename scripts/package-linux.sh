#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/zfm"
cp target/release/zfm README.md LICENSE "$stage/zfm/"
archive="zfm-linux-$(uname -m).tar.gz"
tar -czf "$archive" -C "$stage" zfm
printf 'Created %s\n' "$archive"
