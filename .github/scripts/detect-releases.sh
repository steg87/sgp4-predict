#!/usr/bin/env bash
# Detect which workspace crates need releasing.
#
# Usage: detect-releases.sh <pr|merge>
#
#   pr    - a crate is pending if its [package] version differs from BASE_REF
#           (default: origin/main). Exits non-zero if a version was *lowered*.
#   merge - a crate is pending if no git tag <name>-v<version> exists yet
#           (tag existence is the idempotent "already released" marker).
#
# Writes step outputs to $GITHUB_OUTPUT when set:
#   lib / cli / py            -> true|false (crate pending this run)
#   lib_version / cli_version / py_version
#   any                       -> true|false
#   summary_json              -> JSON array of pending crates
# and a human-readable table to stderr.
set -euo pipefail

MODE="${1:?usage: detect-releases.sh <pr|merge>}"
BASE_REF="${BASE_REF:-origin/main}"

# name | dir | registry | output-key
CRATES=(
  "sgp4-predict|sgp4-predict|crates|lib"
  "sgp4-predict-cli|sgp4-predict-cli|crates|cli"
  "sgp4-predict-py|sgp4-predict-py|pypi|py"
)

# Read a Cargo.toml on stdin, print the [package] table's version value.
pkg_version() {
  awk '
    /^\[/ { inpkg = ($0 == "[package]") }
    inpkg && /^[[:space:]]*version[[:space:]]*=/ {
      match($0, /"[^"]*"/); print substr($0, RSTART + 1, RLENGTH - 2); exit
    }'
}

emit() {
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"
  fi
}

summary='[]'
any=false

for entry in "${CRATES[@]}"; do
  IFS='|' read -r name dir registry key <<< "$entry"
  cur="$(pkg_version < "$dir/Cargo.toml")"
  [ -n "$cur" ] || { echo "ERROR: no [package] version in $dir/Cargo.toml" >&2; exit 1; }
  tag="${name}-v${cur}"
  pending=false

  # ONLY_CRATE (set by the manual-dispatch path) scopes the run to one crate.
  if [ -n "${ONLY_CRATE:-}" ] && [ "$name" != "$ONLY_CRATE" ]; then
    emit "$key" false
    emit "${key}_version" "$cur"
    printf '  skip     %-20s %-8s (not selected)\n' "$name" "$cur" >&2
    continue
  fi

  if [ "$MODE" = "pr" ]; then
    base="$(git show "${BASE_REF}:${dir}/Cargo.toml" 2>/dev/null | pkg_version || true)"
    if [ -n "$base" ] && [ "$cur" != "$base" ]; then
      smaller="$(printf '%s\n%s\n' "$cur" "$base" | sort -V | head -1)"
      if [ "$smaller" = "$cur" ]; then
        echo "ERROR: $name version $cur is lower than $BASE_REF ($base)" >&2
        exit 1
      fi
      pending=true
    fi
  elif [ "$MODE" = "merge" ]; then
    [ -z "$(git tag --list "$tag")" ] && pending=true
  else
    echo "ERROR: unknown mode '$MODE' (expected pr|merge)" >&2; exit 1
  fi

  emit "$key" "$pending"
  emit "${key}_version" "$cur"

  if [ "$pending" = true ]; then
    any=true
    summary="$(jq -c \
      --arg n "$name" --arg d "$dir" --arg v "$cur" --arg r "$registry" --arg t "$tag" \
      '. += [{name:$n, dir:$d, version:$v, registry:$r, tag:$t}]' <<< "$summary")"
    printf '  RELEASE  %-20s %-8s (%s)\n' "$name" "$cur" "$registry" >&2
  else
    printf '  skip     %-20s %-8s\n' "$name" "$cur" >&2
  fi
done

emit any "$any"
emit summary_json "$summary"
echo "$summary"
