#!/usr/bin/env bash
# Create the git tag and GitHub Release for one crate, idempotently.
#
# Usage: tag-and-release.sh <crate> <version>
#
# <crate> is both the crate name and its directory — they are the same for all
# three workspace members, and the tag is "<crate>-v<version>".
#
# Requires: gh (with GH_TOKEN set), git remote 'origin' authenticated for push.
set -euo pipefail

name="${1:?usage: tag-and-release.sh <crate> <version>}"
version="${2:?usage: tag-and-release.sh <crate> <version>}"
dir="$name"
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

# "Latest" belongs to the library crate released from main — it is the repo's
# headline version. Left to gh's default ("automatic based on date and
# version") the three releases race for the badge, and a backport from a
# maintenance branch would take it from a newer line.
case "$version" in
  *-*) prerelease=true ;;
  *)   prerelease=false ;;
esac
latest=false
if [ "$prerelease" = false ] && [ "$name" = sgp4-predict ] && [ "${GITHUB_REF_NAME:-}" = main ]; then
  latest=true
fi

git tag "$tag"
git push origin "$tag"
gh release create "$tag" --title "${name} ${version}" --notes "$notes" \
  --latest="$latest" --prerelease="$prerelease"
echo "Created tag and GitHub Release ${tag} (latest=${latest}, prerelease=${prerelease})."
