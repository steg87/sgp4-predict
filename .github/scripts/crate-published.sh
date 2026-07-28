#!/usr/bin/env bash
# Exit 0 if <crate>@<version> already exists on the crates.io sparse index,
# non-zero otherwise. Used to make publishing idempotent (skip re-publishing a
# version that is already live, e.g. after a partial previous run).
#
# Usage: crate-published.sh <crate-name> <version>
set -euo pipefail

name="${1:?usage: crate-published.sh <crate-name> <version>}"
version="${2:?usage: crate-published.sh <crate-name> <version>}"

lname="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')"
case "${#lname}" in
  1) path="1/${lname}" ;;
  2) path="2/${lname}" ;;
  3) path="3/${lname:0:1}/${lname}" ;;
  *) path="${lname:0:2}/${lname:2:2}/${lname}" ;;
esac

body="$(curl -fsSL "https://index.crates.io/${path}" 2>/dev/null || true)"
[ -n "$body" ] || exit 1
# Any existing version (even yanked) counts — that version number can't be reused.
printf '%s\n' "$body" | jq -r '.vers' | grep -qx "$version"
