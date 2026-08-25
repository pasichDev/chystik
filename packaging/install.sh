#!/usr/bin/env bash
# Install the Chystik desktop entry + icons into the current user's XDG dirs,
# then refresh the icon/desktop caches. Safe to re-run (idempotent overwrite).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

app_dir="${HOME}/.local/share/applications"
icon_root="${HOME}/.local/share/icons/hicolor"
bin_dir="${HOME}/.local/bin"

for binary in chystik chystik-gui; do
    source_binary="$repo_root/target/release/$binary"
    if [[ ! -x "$source_binary" ]]; then
        echo "missing $source_binary; run cargo build --release first" >&2
        exit 1
    fi
    install -Dm755 "$source_binary" "$bin_dir/$binary"
done

mkdir -p "$app_dir"
cp "$repo_root/packaging/chystik.desktop" "$app_dir/"

# One PNG per hicolor size directory. Dropping a single 128x128 file into
# hicolor/256x256 (as this script used to) leaves every other size to be
# guessed by the theme engine, which is what made the dock icon blurry.
for png in "$repo_root"/assets/icons/chystik-*.png; do
    size="$(basename "$png" .png)"
    size="${size##*-}"
    dir="${icon_root}/${size}x${size}/apps"
    mkdir -p "$dir"
    cp "$png" "$dir/chystik.png"
done

# Scalable original, preferred by themes that can render it.
if [ -f "$repo_root/assets/icon.svg" ]; then
    mkdir -p "${icon_root}/scalable/apps"
    cp "$repo_root/assets/icon.svg" "${icon_root}/scalable/apps/chystik.svg"
fi

command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$app_dir" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -q -t -f "$icon_root" || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 --noincremental || true

echo "Installed chystik CLI, chystik-gui, desktop entry and $(ls "$repo_root"/assets/icons/chystik-*.png | wc -l) icon sizes."
echo "If the launcher does not show it yet, log out and back in."
