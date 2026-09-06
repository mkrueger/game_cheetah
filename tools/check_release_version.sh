#!/usr/bin/env bash
# Run from the repository root. Requires Cargo and jq (available on Ubuntu CI).
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 v<package-version>" >&2
    exit 1
fi

version=$(cargo metadata --locked --no-deps --format-version 1 |
    jq -er '.packages[] | select(.name == "game-cheetah") | .version')
expected="v$version"

if [[ "$1" != "$expected" ]]; then
    printf 'Release tag "%s" does not match Cargo package version "%s"; expected "%s".\n' "$1" "$version" "$expected" >&2
    exit 1
fi

printf 'Release tag verified: %s (Cargo package version %s)\n' "$1" "$version"
