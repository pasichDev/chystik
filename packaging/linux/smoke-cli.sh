#!/usr/bin/env bash
# Read-only smoke for a staged CLI binary. It isolates policy paths so a
# package test never reads a developer or CI runner's real exclusions.
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    echo "usage: $0 <packaged-chystik-cli>" >&2
    exit 2
fi

binary="$1"
test -x "$binary"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

output="$(
    HOME="$fixture" \
    XDG_CONFIG_HOME="$fixture/config" \
    CHYSTIK_TEST_HOME="$fixture" \
    "$binary" scan "$fixture" --safe --format json
)"
grep -Fq '"kind": "scan"' <<<"$output"
grep -Fq '"schema_version": 1' <<<"$output"
echo "Packaged CLI read-only smoke passed: $binary"
