#!/usr/bin/env bash
# Create the git tag and GitHub Release for one crate, idempotently.
#
# Usage: tag-and-release.sh <crate-dir> <crate-name> <version>
#
# Requires: gh (with GH_TOKEN set), git remote 'origin' authenticated for push.
set -euo pipefail

dir="${1:?usage: tag-and-release.sh <crate-dir> <crate-name> <version>}"
name="${2:?usage: tag-and-release.sh <crate-dir> <crate-name> <version>}"
version="${3:?usage: tag-and-release.sh <crate-dir> <crate-name> <version>}"
tag="${name}-v${version}"

if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null 2>&1; then
  echo "Tag ${tag} already exists locally; skipping."
  exit 0
fi
if [ -n "$(git ls-remote --tags origin "refs/tags/${tag}")" ]; then
  echo "Tag ${tag} already exists on origin; skipping."
  exit 0
fi

notes="$(.github/scripts/extract-changelog.sh "$dir" "$version")"
[ -n "$notes" ] || notes="Release ${version}."

git tag "$tag"
git push origin "$tag"
gh release create "$tag" --title "${name} ${version}" --notes "$notes"
echo "Created tag and GitHub Release ${tag}."
