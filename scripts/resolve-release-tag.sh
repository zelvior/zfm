#!/usr/bin/env bash
# Resolve an existing origin tag to a commit; optionally verify it has not moved.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo 'Usage: resolve-release-tag.sh TAG [EXPECTED_COMMIT]' >&2
    exit 2
fi
tag=$1
if [[ ! "$tag" =~ ^v[0-9A-Za-z][0-9A-Za-z._+-]*$ ]] || ! git check-ref-format "refs/tags/$tag"; then
    echo 'Enter a valid v-prefixed tag, for example v0.1.0.' >&2
    exit 1
fi
if ! git fetch --quiet --no-tags origin "refs/tags/$tag"; then
    echo "Could not fetch tag $tag. Ensure it exists on the GitHub repository." >&2
    exit 1
fi
commit=$(git rev-parse --verify 'FETCH_HEAD^{commit}')
if [ "$#" -eq 2 ] && [ "$commit" != "$2" ]; then
    echo 'The tag changed after the builds started; refusing to publish.' >&2
    exit 1
fi
printf '%s\n' "$commit"
