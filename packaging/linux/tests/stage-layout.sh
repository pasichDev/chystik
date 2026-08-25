#!/usr/bin/env bash
# Verifies the FHS tree shared by every Linux package format.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
stage_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir"' EXIT

if [[ ! -x "$repo_root/target/release/chystik-gui" ]]; then
    cargo build --release --package chystik-gui --manifest-path "$repo_root/Cargo.toml"
fi

PACKAGING_SKIP_BUILD=1 "$repo_root/packaging/linux/stage.sh" "$stage_dir"

test -x "$stage_dir/usr/bin/chystik-gui"
test -f "$stage_dir/usr/share/applications/chystik.desktop"
test -f "$stage_dir/usr/share/icons/hicolor/128x128/apps/chystik.png"
test -f "$stage_dir/usr/share/icons/hicolor/scalable/apps/chystik.svg"
test -f "$stage_dir/usr/share/licenses/chystik/LICENSE"

grep -qx 'Exec=chystik-gui' "$stage_dir/usr/share/applications/chystik.desktop"
echo "Linux package staging layout is valid"
