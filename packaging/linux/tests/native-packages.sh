#!/usr/bin/env bash
# Metadata checks for package builders. Set PACKAGING_TEST_RPM=1 where rpm tools exist.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
dist_dir="$(mktemp -d)"
trap 'rm -rf "$dist_dir"' EXIT

DIST_DIR="$dist_dir" "$repo_root/packaging/linux/build-deb.sh"
deb_file="$(find "$dist_dir" -maxdepth 1 -name '*.deb' -print -quit)"
test -n "$deb_file"
test "$(dpkg-deb --field "$deb_file" Package)" = 'chystik'
test "$(dpkg-deb --field "$deb_file" Architecture)" = 'amd64'
dpkg-deb --field "$deb_file" Depends | grep -Fq 'libxkbcommon-x11-0'

if [[ "${PACKAGING_TEST_RPM:-0}" == '1' ]]; then
    DIST_DIR="$dist_dir" "$repo_root/packaging/linux/build-rpm.sh"
    rpm_file="$(find "$dist_dir" -maxdepth 1 -name '*.rpm' -print -quit)"
    test -n "$rpm_file"
    test "$(rpm -qp --qf '%{NAME}' "$rpm_file")" = 'chystik'
    test "$(rpm -qp --qf '%{ARCH}' "$rpm_file")" = 'x86_64'
    rpm -qp --requires "$rpm_file" | grep -Fxq 'libxkbcommon-x11'
fi

echo "Linux native package metadata is valid"
