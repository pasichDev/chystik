#!/usr/bin/env bash
# Verify that the generated site gets all release data from Cargo.toml.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
rendered_dir="$(mktemp -d "${TMPDIR:-/tmp}/chystik-site.XXXXXX")"
trap 'rm -rf "$rendered_dir"' EXIT

if grep -nE '(Version|Chystik-|chystik_)[[:space:]]*[0-9]+\.[0-9]+\.[0-9]+' "$repo_root/site/index.html"; then
    echo "site template must use version placeholders, not a hand-maintained release version" >&2
    exit 1
fi

bash "$repo_root/scripts/build-site.sh" "$rendered_dir"
version="$(awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version[[:space:]]*=/ {
        match($0, /"[^"]+"/)
        print substr($0, RSTART + 1, RLENGTH - 2)
        exit
    }
' "$repo_root/Cargo.toml")"

test -f "$rendered_dir/assets/icon.png"
grep -F "Version $version" "$rendered_dir/index.html" >/dev/null
grep -F "Chystik-$version-x86_64.AppImage" "$rendered_dir/index.html" >/dev/null
! grep -qE '\{\{[A-Z_]+\}\}' "$rendered_dir/index.html"
