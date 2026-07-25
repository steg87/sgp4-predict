#!/usr/bin/env bash
# Bump a crate's [package] version and roll its CHANGELOG Unreleased section
# into a dated release section. When the library crate (sgp4-predict) is bumped,
# also update the version requirement of the workspace dependents that pin it, so
# the workspace stays resolvable. Prints the new version to stdout.
#
# Usage: bump-version.sh <crate-dir> <patch|minor|major>
set -euo pipefail

dir="${1:?usage: bump-version.sh <crate-dir> <patch|minor|major>}"
kind="${2:?usage: bump-version.sh <crate-dir> <patch|minor|major>}"
manifest="$dir/Cargo.toml"

# Read a [package] string field (version / name) from a manifest.
pkg_field() {
  awk -v f="$2" '
    /^\[/ { inpkg = ($0 == "[package]") }
    inpkg && $0 ~ ("^[[:space:]]*" f "[[:space:]]*=") {
      match($0, /"[^"]*"/); print substr($0, RSTART + 1, RLENGTH - 2); exit
    }' "$1"
}

cur="$(pkg_field "$manifest" version)"
[ -n "$cur" ] || { echo "ERROR: no [package] version in $manifest" >&2; exit 1; }
if ! [[ "$cur" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "ERROR: version '$cur' is not plain MAJOR.MINOR.PATCH; pre-release/build metadata is not supported" >&2
  exit 1
fi

IFS=. read -r maj min pat <<< "$cur"
case "$kind" in
  major) maj=$((maj + 1)); min=0; pat=0 ;;
  minor) min=$((min + 1)); pat=0 ;;
  patch) pat=$((pat + 1)) ;;
  *) echo "ERROR: unknown bump kind '$kind' (expected patch|minor|major)" >&2; exit 1 ;;
esac
new="${maj}.${min}.${pat}"

# Rewrite only the first [package] version line of a manifest.
set_pkg_version() {
  local tmp; tmp="$(mktemp)"
  awk -v new="$2" '
    /^\[/ { inpkg = ($0 == "[package]") }
    inpkg && !done && /^[[:space:]]*version[[:space:]]*=/ {
      sub(/"[^"]*"/, "\"" new "\""); done = 1
    }
    { print }
  ' "$1" > "$tmp" && mv "$tmp" "$1"
}
set_pkg_version "$manifest" "$new"

# If the library crate was bumped, update dependents that pin its version
# (`sgp4-predict = { path = "...", version = "X" }`) so the workspace resolves.
if [ "$(pkg_field "$manifest" name)" = "sgp4-predict" ]; then
  ws_root="$(dirname "$dir")"
  for dep in "$ws_root/sgp4-predict-cli/Cargo.toml" "$ws_root/sgp4-predict-py/Cargo.toml"; do
    [ -f "$dep" ] || continue
    tmp="$(mktemp)"
    awk -v v="$new" '
      $1 == "sgp4-predict" && /version[[:space:]]*=/ {
        sub(/version[[:space:]]*=[[:space:]]*"[^"]*"/, "version = \"" v "\"")
      }
      { print }
    ' "$dep" > "$tmp" && mv "$tmp" "$dep"
  done
fi

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
