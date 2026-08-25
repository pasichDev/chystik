#!/usr/bin/env bash
# Fail closed when release metadata drifts away from the Cargo workspace version.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

if [[ "$#" -gt 1 ]]; then
    echo "usage: $0 [vMAJOR.MINOR.PATCH]" >&2
    exit 2
fi

version="$(release_version)"
arch_version="$(sed -n 's/^pkgver=\([0-9][0-9A-Za-z.+~-]*\)$/\1/p' "$REPO_ROOT/packaging/arch/PKGBUILD")"
[[ "$arch_version" == "$version" ]] || {
    echo "packaging/arch/PKGBUILD has pkgver=$arch_version; expected $version from Cargo.toml" >&2
    exit 2
}

if [[ "$#" -eq 1 ]]; then
    verify_release_tag "$1"
fi

echo "Release metadata is aligned at v$version"
