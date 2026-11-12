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
# New tags select the explicitly supplied checkout commit; existing tags always
# win over that fallback, including annotated tags.
[ "$("$root/scripts/resolve-release-tag.sh" v3.0.0 --fallback "$first")" = "new $first" ]
[ "$("$root/scripts/resolve-release-tag.sh" v2.0.0 --fallback "$first")" = "existing $first" ]
if "$root/scripts/resolve-release-tag.sh" v3.0.0 --fallback invalid >/dev/null 2>&1; then echo 'Invalid fallback accepted' >&2; exit 1; fi

# Simulate GitHub's atomic create-ref API against the test remote.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] && [ "$2" = --method ] && [ "$3" = POST ]
[ "$4" = "repos/example/zfm/git/refs" ] && [ "$5" = -f ] && [ "$7" = -f ]
ref=${6#ref=} sha=${8#sha=}
[ "$ref" != "$6" ] && [ "$sha" != "$8" ]
git -C "$TEST_ORIGIN" update-ref "$ref" "$sha" 0000000000000000000000000000000000000000
MOCK
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH" GH_REPO=example/zfm TEST_ORIGIN="$tmp/origin"
[ "$("$root/scripts/ensure-release-tag.sh" v1.0.0 "$first" existing)" = "$first" ]
# New tags are created only at publication, and never replace existing refs.
"$root/scripts/ensure-release-tag.sh" v3.0.0 "$first" new >/dev/null
[ "$(git -C "$tmp/origin" rev-parse refs/tags/v3.0.0)" = "$first" ]
if "$root/scripts/ensure-release-tag.sh" v3.0.0 "$first" new >/dev/null 2>&1; then echo 'Existing tag was replaced' >&2; exit 1; fi
if "$root/scripts/ensure-release-tag.sh" v4.0.0 "$first" existing >/dev/null 2>&1; then echo 'Deleted tag was recreated' >&2; exit 1; fi
# A remote/network error must not be mistaken for a missing tag.
git remote set-url origin "$tmp/not-a-repository"
if "$root/scripts/resolve-release-tag.sh" v5.0.0 --fallback "$first" >/dev/null 2>&1; then echo 'Remote failure accepted' >&2; exit 1; fi
git remote set-url origin "$tmp/origin"
git -C "$tmp/origin" commit --quiet --allow-empty -m second
second=$(git -C "$tmp/origin" rev-parse HEAD)
git -C "$tmp/origin" update-ref refs/tags/v1.0.0 "$second"
if "$root/scripts/resolve-release-tag.sh" v1.0.0 "$first" >/dev/null 2>&1; then
    echo 'Moved tag was not rejected' >&2; exit 1
fi
if "$root/scripts/ensure-release-tag.sh" v1.0.0 "$first" existing >/dev/null 2>&1; then
    echo 'Publishing a moved tag was not rejected' >&2; exit 1
fi
[ "$("$root/scripts/resolve-release-tag.sh" v1.0.0 "$second")" = "$second" ]
echo 'Release tag validation tests passed.'
