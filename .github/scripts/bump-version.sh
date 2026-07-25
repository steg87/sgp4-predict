#!/usr/bin/env bash
# Bump a crate's [package] version and roll its CHANGELOG Unreleased section
# into a dated release section. Prints the new version to stdout.
#
# Usage: bump-version.sh <crate-dir> <patch|minor|major>
set -euo pipefail

dir="${1:?usage: bump-version.sh <crate-dir> <patch|minor|major>}"
kind="${2:?usage: bump-version.sh <crate-dir> <patch|minor|major>}"
manifest="$dir/Cargo.toml"

cur="$(awk '
  /^\[/ { inpkg = ($0 == "[package]") }
  inpkg && /^[[:space:]]*version[[:space:]]*=/ {
    match($0, /"[^"]*"/); print substr($0, RSTART + 1, RLENGTH - 2); exit
  }' "$manifest")"
[ -n "$cur" ] || { echo "ERROR: no [package] version in $manifest" >&2; exit 1; }

IFS=. read -r maj min pat <<< "$cur"
case "$kind" in
  major) maj=$((maj + 1)); min=0; pat=0 ;;
  minor) min=$((min + 1)); pat=0 ;;
  patch) pat=$((pat + 1)) ;;
  *) echo "ERROR: unknown bump kind '$kind' (expected patch|minor|major)" >&2; exit 1 ;;
esac
new="${maj}.${min}.${pat}"

# Rewrite only the [package] version line.
tmp="$(mktemp)"
awk -v new="$new" '
  /^\[/ { inpkg = ($0 == "[package]") }
  inpkg && !done && /^[[:space:]]*version[[:space:]]*=/ {
    sub(/"[^"]*"/, "\"" new "\""); done = 1
  }
  { print }
' "$manifest" > "$tmp" && mv "$tmp" "$manifest"

# Roll CHANGELOG: insert a new dated version heading directly under [Unreleased],
# so the accumulated Unreleased notes become this release's notes.
changelog="$dir/CHANGELOG.md"
if [ -f "$changelog" ]; then
  tmp="$(mktemp)"
  awk -v ver="$new" -v date="$(date +%F)" '
    /^## \[Unreleased\]/ && !done {
      print; print ""; print "## [" ver "] - " date; done = 1; next
    }
    { print }
  ' "$changelog" > "$tmp" && mv "$tmp" "$changelog"
fi

echo "$new"
