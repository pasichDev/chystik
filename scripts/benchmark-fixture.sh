#!/usr/bin/env bash
# Create a deterministic scan-only fixture. It never invokes Chystik cleanup.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 /absolute/new-fixture-directory" >&2
    exit 2
fi

fixture="$1"
if [[ "$fixture" != /* || "$fixture" == / || -e "$fixture" ]]; then
    echo "fixture must be a new absolute directory and must not be /" >&2
    exit 2
fi

parent="$(cd "$(dirname "$fixture")" && pwd)"
fixture="$parent/$(basename "$fixture")"

mkdir -p "$fixture/home/.cache/pip" \
    "$fixture/workspace/node_modules/left-pad" \
    "$fixture/workspace/target/debug/deps" \
    "$fixture/irrelevant/deep/one/two/three" \
    "$fixture/protected-looking/.ssh" \
    "$fixture/protected-looking/.git"

printf '[package]\nname = "benchmark-fixture"\nversion = "0.0.0"\n' > "$fixture/workspace/Cargo.toml"
printf '{"name":"benchmark-fixture"}\n' > "$fixture/workspace/package.json"
printf '{"lockfileVersion":3}\n' > "$fixture/workspace/package-lock.json"
printf 'not a credential\n' > "$fixture/protected-looking/.ssh/fixture-only.txt"
printf '[core]\nrepositoryformatversion = 0\n' > "$fixture/protected-looking/.git/config"

# Fixed names and sizes make the tree inspectable. The many irrelevant files
# exercise directory walking without relying on a developer's home directory.
dd if=/dev/zero of="$fixture/home/.cache/pip/wheel-a.whl" bs=1024 count=256 2>/dev/null
dd if=/dev/zero of="$fixture/home/.cache/pip/wheel-b.whl" bs=1024 count=128 2>/dev/null
dd if=/dev/zero of="$fixture/workspace/node_modules/left-pad/index.js" bs=1024 count=512 2>/dev/null
dd if=/dev/zero of="$fixture/workspace/target/debug/deps/fixture-artifact" bs=1024 count=1024 2>/dev/null
for index in $(seq 1 400); do
    printf 'irrelevant fixture entry %04d\n' "$index" \
        > "$fixture/irrelevant/deep/one/two/three/file-$index.txt"
done

printf 'Created deterministic scan fixture: %s\n' "$fixture"
