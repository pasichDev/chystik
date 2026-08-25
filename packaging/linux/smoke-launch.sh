#!/usr/bin/env bash
# Prove a packaged GUI starts against an empty fixture home without cleaning it.
set -euo pipefail

if [[ "$#" -ne 1 || ! -x "$1" ]]; then
    echo "usage: $0 <packaged-chystik-gui>" >&2
    exit 2
fi

command -v xvfb-run >/dev/null 2>&1 || {
    echo 'missing required command: xvfb-run' >&2
    exit 127
}
command -v timeout >/dev/null 2>&1 || {
    echo 'missing required command: timeout' >&2
    exit 127
}

fixture_home="$(mktemp -d)"
log_file="$(mktemp)"
trap 'rm -rf "$fixture_home" "$log_file"' EXIT

set +e
env -u CHYSTIK_AUTOSCAN \
    APPIMAGE_EXTRACT_AND_RUN=1 \
    HOME="$fixture_home" \
    XDG_CONFIG_HOME="$fixture_home/.config" \
    XDG_CACHE_HOME="$fixture_home/.cache" \
    timeout 8s xvfb-run -a "$1" >"$log_file" 2>&1
status=$?
set -e

if [[ "$status" -ne 124 ]]; then
    cat "$log_file" >&2
    echo "packaged GUI exited unexpectedly with status $status" >&2
    exit 1
fi
echo "Packaged GUI stayed alive in an empty fixture home"
