#!/usr/bin/env bash
# Resolve an origin tag; optionally verify it or select a commit for a new tag.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 3 ] || { [ "$#" -eq 3 ] && [ "$2" != '--fallback' ]; }; then
    echo 'Usage: resolve-release-tag.sh TAG [EXPECTED_COMMIT | --fallback COMMIT]' >&2
    exit 1
fi
tag=$1
if [[ ! "$tag" =~ ^v[0-9A-Za-z][0-9A-Za-z._+-]*$ ]] || ! git check-ref-format "refs/tags/$tag"; then
    echo 'Enter a valid v-prefixed tag, for example v0.1.0.' >&2
    exit 1
fi
if git ls-remote --exit-code --refs origin "refs/tags/$tag" >/dev/null; then
    if ! git fetch --quiet --no-tags origin "refs/tags/$tag"; then
        echo "Could not fetch existing tag $tag." >&2
        exit 1
    fi
    commit=$(git rev-parse --verify 'FETCH_HEAD^{commit}')
    if [ "$#" -eq 2 ] && [ "$commit" != "$2" ]; then
        echo 'The tag changed after the builds started; refusing to publish.' >&2
        exit 1
    fi
    if [ "$#" -eq 3 ]; then printf 'existing %s\n' "$commit"; else printf '%s\n' "$commit"; fi
else
    status=$?
    if [ "$status" -ne 2 ]; then
        echo 'Could not check remote tags; refusing to assume the tag is new.' >&2
        exit 1
    fi
    if [ "$#" -eq 3 ]; then
        fallback=$3
        if [[ ! "$fallback" =~ ^[0-9a-fA-F]{40}$ ]]; then echo 'Invalid fallback commit.' >&2; exit 1; fi
        commit=$(git rev-parse --verify "$fallback^{commit}")
        printf 'new %s\n' "$commit"
    else
        echo "Tag $tag does not exist on the remote." >&2
        exit 2
    fi
fi
