#!/usr/bin/env bash
# The release tag, Cargo workspace, and Arch source recipe must name one version.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "$repo_root/packaging/linux/common.sh"

version="$(release_version)"
bash "$repo_root/packaging/linux/verify-version.sh"
bash "$repo_root/packaging/linux/verify-version.sh" "v$version"

if bash "$repo_root/packaging/linux/verify-version.sh" 'v999.999.999'; then
    echo 'a mismatched release tag was accepted' >&2
    exit 1
fi

echo "Release version contract is valid for v$version"
