#!/usr/bin/env bash
# Build a Debian/Ubuntu package from the common Linux staging tree.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

require_command dpkg-deb
version="$(release_version)"
[[ "$version" =~ ^[0-9A-Za-z.+~:-]+$ ]] || {
    echo "unsupported Debian package version: $version" >&2
    exit 2
}

dist_dir="${DIST_DIR:-$REPO_ROOT/dist}"
mkdir -p -- "$dist_dir"
output="$dist_dir/chystik_${version}_amd64.deb"
[[ ! -e "$output" ]] || {
    echo "refusing to overwrite existing artifact: $output" >&2
    exit 2
}

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
root="$work_dir/root"
mkdir "$root"
stage_linux_root "$root"
mkdir -p "$root/DEBIAN"
printf '%s\n' \
    'Package: chystik' \
    "Version: $version" \
    'Section: utils' \
    'Priority: optional' \
    'Architecture: amd64' \
    'Maintainer: pasichDev <67899666+pasichDev@users.noreply.github.com>' \
    'Depends: libgtk-3-0, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libx11-6, libgl1' \
    'Description: Safety-first developer disk cleanup GUI and CLI' \
    ' Chystik classifies developer caches and only moves verified cleanup targets to trash.' \
    > "$root/DEBIAN/control"

dpkg-deb --build --root-owner-group "$root" "$output"
echo "Built $output"
