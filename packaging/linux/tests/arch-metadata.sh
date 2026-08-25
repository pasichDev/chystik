#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
pkgbuild="$repo_root/packaging/arch/PKGBUILD"

grep -qx "pkgname=chystik" "$pkgbuild"
grep -qx "arch=('x86_64')" "$pkgbuild"
grep -qx "depends=('gtk3' 'libxkbcommon' 'libxkbcommon-x11' 'libx11' 'mesa')" "$pkgbuild"
grep -q "cargo build --release --locked --package chystik-gui" "$pkgbuild"
grep -q 'chystik.desktop' "$pkgbuild"
grep -q 'assets/icons/chystik-\*.png' "$pkgbuild"
echo "Arch package metadata is valid"
