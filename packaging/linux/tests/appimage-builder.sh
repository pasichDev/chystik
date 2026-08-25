#!/usr/bin/env bash
# Exercises the AppImage builder command contract without downloading tools.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fake_linuxdeploy="$work_dir/linuxdeploy"
fake_gtk_plugin="$work_dir/linuxdeploy-plugin-gtk.sh"
args_file="$work_dir/linuxdeploy.args"
gtk_version_file="$work_dir/gtk-version"
printf '%s\n' \
    '#!/usr/bin/env bash' \
    'printf "%s\\n" "$*" > "$LINUXDEPLOY_ARGS"' \
    'printf "%s\\n" "$DEPLOY_GTK_VERSION" > "$LINUXDEPLOY_GTK_VERSION"' \
    'touch "$LDAI_OUTPUT"' \
    > "$fake_linuxdeploy"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' > "$fake_gtk_plugin"
chmod +x "$fake_linuxdeploy" "$fake_gtk_plugin"

DIST_DIR="$work_dir/dist" \
LINUXDEPLOY_BIN="$fake_linuxdeploy" \
LINUXDEPLOY_GTK_PLUGIN="$fake_gtk_plugin" \
LINUXDEPLOY_ARGS="$args_file" \
LINUXDEPLOY_GTK_VERSION="$gtk_version_file" \
"$repo_root/packaging/linux/build-appimage.sh"

artifact="$(find "$work_dir/dist" -maxdepth 1 -name '*.AppImage' -print -quit)"
test -x "$artifact"
test -f "$artifact.sha256"
(cd "$(dirname "$artifact")" && sha256sum -c "$(basename "$artifact").sha256")
grep -q -- '--plugin gtk' "$args_file"
grep -Eq -- '--library .*/libxkbcommon-x11\.so\.0' "$args_file"
grep -q -- '--output appimage' "$args_file"
test "$(cat "$gtk_version_file")" = '3'
echo "AppImage builder command contract is valid"
