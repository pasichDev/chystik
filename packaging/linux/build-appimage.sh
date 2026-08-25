#!/usr/bin/env bash
# Build an x86_64 generic-Linux AppImage with pinned linuxdeploy tooling.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

linuxdeploy_version='1-alpha-20251107-1'
linuxdeploy_url="https://github.com/linuxdeploy/linuxdeploy/releases/download/$linuxdeploy_version/linuxdeploy-x86_64.AppImage"
linuxdeploy_sha256='c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d'
gtk_plugin_rev='7a3fbc31a9e5075073ff8790f26effbac5f84453'
gtk_plugin_url="https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/$gtk_plugin_rev/linuxdeploy-plugin-gtk.sh"
gtk_plugin_sha256='b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171'

require_command sha256sum
version="$(release_version)"
release_arch >/dev/null
xkbcommon_x11_library="$(system_library_path libxkbcommon-x11.so.0)"
dist_dir="${DIST_DIR:-$REPO_ROOT/dist}"
mkdir -p -- "$dist_dir"
artifact_name="Chystik-${version}-x86_64.AppImage"
output="$dist_dir/$artifact_name"
[[ ! -e "$output" ]] || {
    echo "refusing to overwrite existing artifact: $output" >&2
    exit 2
}

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
app_dir="$work_dir/AppDir"
mkdir "$app_dir"
stage_linux_root "$app_dir"

download_pinned() {
    local url="$1" expected="$2" destination="$3"
    require_command curl
    curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error "$url" -o "$destination"
    printf '%s  %s\n' "$expected" "$destination" | sha256sum -c -
    chmod +x "$destination"
}

linuxdeploy_bin="${LINUXDEPLOY_BIN:-$work_dir/linuxdeploy-x86_64.AppImage}"
if [[ -z "${LINUXDEPLOY_BIN:-}" ]]; then
    download_pinned "$linuxdeploy_url" "$linuxdeploy_sha256" "$linuxdeploy_bin"
fi
[[ -x "$linuxdeploy_bin" ]] || {
    echo "linuxdeploy is not executable: $linuxdeploy_bin" >&2
    exit 2
}

gtk_plugin="${LINUXDEPLOY_GTK_PLUGIN:-$work_dir/linuxdeploy-plugin-gtk.sh}"
if [[ -z "${LINUXDEPLOY_GTK_PLUGIN:-}" ]]; then
    download_pinned "$gtk_plugin_url" "$gtk_plugin_sha256" "$gtk_plugin"
fi
[[ -x "$gtk_plugin" ]] || {
    echo "linuxdeploy GTK plugin is not executable: $gtk_plugin" >&2
    exit 2
}

generated="$work_dir/$artifact_name"
APPIMAGE_EXTRACT_AND_RUN=1 \
DEPLOY_GTK_VERSION="${DEPLOY_GTK_VERSION:-3}" \
LDAI_OUTPUT="$generated" \
PATH="$(dirname "$gtk_plugin"):$PATH" \
"$linuxdeploy_bin" \
    --appdir "$app_dir" \
    --executable "$app_dir/usr/bin/chystik-gui" \
    --desktop-file "$REPO_ROOT/packaging/chystik.desktop" \
    --icon-file "$REPO_ROOT/assets/icons/chystik-256.png" \
    --library "$xkbcommon_x11_library" \
    --plugin gtk \
    --output appimage

[[ -f "$generated" ]] || {
    echo "linuxdeploy did not produce $artifact_name" >&2
    exit 1
}
install -Dm755 "$generated" "$output"
(cd "$dist_dir" && sha256sum "$artifact_name" > "$artifact_name.sha256")
echo "Built $output"
