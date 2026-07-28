#!/usr/bin/env bash
# Print the changelog section for one version from a crate's CHANGELOG.md.
#
# Usage: extract-changelog.sh <crate-dir> <version>
#
# Matches the "## [<version>] - <date>" heading and prints everything up to the
# next "## " heading. Prints nothing (exit 0) if the file or section is absent —
# callers should substitute a default note in that case.
#
# Pre-release versions (0.2.0-rc.1) read "## [Unreleased]" instead: a release
# candidate ships the pending notes, so the prepare workflow leaves them under
# Unreleased and only rolls them into a dated section at the final release.
set -euo pipefail

dir="${1:?usage: extract-changelog.sh <crate-dir> <version>}"
version="${2:?usage: extract-changelog.sh <crate-dir> <version>}"
file="$dir/CHANGELOG.md"

case "$version" in
  *-*) version="Unreleased" ;;
esac

[ -f "$file" ] || exit 0

awk -v ver="$version" '
  /^## / {
    if (capturing) exit
    if (match($0, /\[[^]]*\]/)) {
      sec = substr($0, RSTART + 1, RLENGTH - 2)
      if (sec == ver) { capturing = 1; next }
    }
    next
  }
  capturing { print }
' "$file"
