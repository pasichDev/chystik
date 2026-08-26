#!/usr/bin/env bash
# Render the static site from the one authoritative workspace version.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_html="$repo_root/site/index.html"
output_dir="${1:-$repo_root/target/site}"

workspace_version="$({
    awk '
        /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
        /^\[/ { in_workspace_package = 0 }
        in_workspace_package && /^version[[:space:]]*=/ {
            match($0, /"[^"]+"/)
            print substr($0, RSTART + 1, RLENGTH - 2)
            exit
        }
    ' "$repo_root/Cargo.toml"
})"

if [[ ! "$workspace_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]]; then
    echo "could not read a workspace package version from Cargo.toml" >&2
    exit 1
fi

if ! grep -q '{{VERSION}}' "$source_html"; then
    echo "site template is missing {{VERSION}}" >&2
    exit 1
fi

mkdir -p "$output_dir/assets"
cp -R "$repo_root/site/assets/." "$output_dir/assets/"

awk -v version="$workspace_version" -v tag="v$workspace_version" '
    {
        gsub(/\{\{VERSION\}\}/, version)
        gsub(/\{\{TAG\}\}/, tag)
        print
    }
' "$source_html" > "$output_dir/index.html"

if grep -nE '\{\{[A-Z_]+\}\}' "$output_dir/index.html"; then
    echo "site rendering left an unresolved placeholder" >&2
    exit 1
fi

printf 'Rendered Chystik %s site to %s\n' "$workspace_version" "$output_dir"
