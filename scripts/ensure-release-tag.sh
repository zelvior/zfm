#!/usr/bin/env bash
# Called only after all builds pass. Never replace an existing Git tag.
set -euo pipefail
if [ "$#" -ne 3 ]; then echo 'Usage: ensure-release-tag.sh TAG COMMIT existing|new' >&2; exit 1; fi
tag=$1
commit=$2
source=$3
if [[ ! "$tag" =~ ^v[0-9A-Za-z][0-9A-Za-z._+-]*$ ]] || ! git check-ref-format "refs/tags/$tag" || [[ ! "$commit" =~ ^[0-9a-f]{40}$ ]]; then
    echo 'Invalid tag or commit.' >&2; exit 1
fi
case "$source" in
    existing) ;;
    new)
        : "${GH_REPO:?GH_REPO must name the GitHub repository}"
        echo "Creating tag $tag at $commit"
        # GitHub's create-ref API fails if another run has created the tag.
        # Never update an existing tag, even if it appeared during the build.
        gh api --method POST "repos/$GH_REPO/git/refs" -f "ref=refs/tags/$tag" -f "sha=$commit" >/dev/null
        ;;
    *) echo 'Invalid tag source.' >&2; exit 1 ;;
esac
"$(dirname "$0")/resolve-release-tag.sh" "$tag" "$commit"
