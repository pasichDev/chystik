#!/usr/bin/env bash
# Verifies the FHS tree shared by every Linux package format.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
stage_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir"' EXIT

if [[ ! -x "$repo_root/target/release/chystik-gui" || ! -x "$repo_root/target/release/chystik" ]]; then
    cargo build --release --package chystik-gui --package chystik-cli --manifest-path "$repo_root/Cargo.toml"
fi

PACKAGING_SKIP_BUILD=1 "$repo_root/packaging/linux/stage.sh" "$stage_dir"

test -x "$stage_dir/usr/bin/chystik-gui"
test -x "$stage_dir/usr/bin/chystik"
test -f "$stage_dir/usr/share/man/man1/chystik.1"
test -f "$stage_dir/usr/share/bash-completion/completions/chystik"
test -f "$stage_dir/usr/share/zsh/site-functions/_chystik"
test -f "$stage_dir/usr/share/fish/vendor_completions.d/chystik.fish"
test -f "$stage_dir/usr/share/powershell/Modules/Chystik/chystik.ps1"
test -f "$stage_dir/usr/share/applications/chystik.desktop"
test -f "$stage_dir/usr/share/icons/hicolor/128x128/apps/chystik.png"
test -f "$stage_dir/usr/share/icons/hicolor/scalable/apps/chystik.svg"
test -f "$stage_dir/usr/share/licenses/chystik/LICENSE"

grep -qx 'Exec=chystik-gui' "$stage_dir/usr/share/applications/chystik.desktop"
grep -Fq 'chystik\-clean(1)' "$stage_dir/usr/share/man/man1/chystik.1"
grep -Fq 'native Trash' "$stage_dir/usr/share/man/man1/chystik.1"
grep -q 'chystik' "$stage_dir/usr/share/bash-completion/completions/chystik"
echo "Linux package staging layout is valid"
