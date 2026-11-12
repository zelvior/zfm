#!/usr/bin/env bash
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git init --quiet "$tmp/origin"
git -C "$tmp/origin" config user.name 'ZFM Test'
git -C "$tmp/origin" config user.email 'test@example.invalid'
git -C "$tmp/origin" commit --quiet --allow-empty -m first
first=$(git -C "$tmp/origin" rev-parse HEAD)
git -C "$tmp/origin" tag v1.0.0
git -C "$tmp/origin" tag -a v2.0.0 -m annotated
git clone --quiet "$tmp/origin" "$tmp/work"
cd "$tmp/work"
for tag in v1.0.0 v2.0.0; do
    [ "$("$root/scripts/resolve-release-tag.sh" "$tag")" = "$first" ]
    [ "$("$root/scripts/resolve-release-tag.sh" "$tag" "$first")" = "$first" ]
done
for tag in '' latest 'v../escape' 'v1;echo evil' v-missing v3.0.0; do
    if "$root/scripts/resolve-release-tag.sh" "$tag" >/dev/null 2>&1; then
        echo "Unexpected valid tag: $tag" >&2; exit 1
    fi
done
git -C "$tmp/origin" commit --quiet --allow-empty -m second
second=$(git -C "$tmp/origin" rev-parse HEAD)
git -C "$tmp/origin" update-ref refs/tags/v1.0.0 "$second"
if "$root/scripts/resolve-release-tag.sh" v1.0.0 "$first" >/dev/null 2>&1; then
    echo 'Moved tag was not rejected' >&2; exit 1
fi
[ "$("$root/scripts/resolve-release-tag.sh" v1.0.0 "$second")" = "$second" ]
echo 'Release tag validation tests passed.'
