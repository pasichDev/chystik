#!/usr/bin/env bash
# Keep the in-tree Arch recipe ready to copy into an AUR source package.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
pkgbuild="$repo_root/packaging/arch/PKGBUILD"
srcinfo="$repo_root/packaging/arch/.SRCINFO"
source "$repo_root/packaging/linux/common.sh"

version="$(release_version)"
source_url="https://github.com/pasichDev/chystik/archive/refs/tags/v${version}.tar.gz"
source_hash="$(sed -n "s/^sha256sums=('\\([0-9a-f]\\{64\\}\\)')$/\\1/p" "$pkgbuild")"

test -n "$source_hash"
test -f "$srcinfo"
if grep -Fq 'SKIP' "$pkgbuild" "$srcinfo"; then
    echo 'AUR metadata must pin the release source checksum' >&2
    exit 1
fi

grep -Fx "pkgver=$version" "$pkgbuild" >/dev/null
grep -Fx 'source=("https://github.com/pasichDev/chystik/archive/refs/tags/v${pkgver}.tar.gz")' "$pkgbuild" >/dev/null
grep -Fx $'\tpkgver = '"$version" "$srcinfo" >/dev/null
grep -Fx $'\tsource = '"$source_url" "$srcinfo" >/dev/null
grep -Fx $'\tsha256sums = '"$source_hash" "$srcinfo" >/dev/null

if command -v makepkg >/dev/null 2>&1; then
    generated="$(mktemp)"
    trap 'rm -f "$generated"' EXIT
    (
        cd "$(dirname "$pkgbuild")"
        makepkg --printsrcinfo
    ) > "$generated"
    diff -u "$srcinfo" "$generated"
fi

echo "Arch AUR source metadata is valid"
