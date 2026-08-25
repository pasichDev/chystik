#!/usr/bin/env bash
# Assemble the common FHS tree used by AppImage, Debian and RPM builders.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

if [[ "$#" -ne 1 ]]; then
    echo "usage: $0 <empty-staging-directory>" >&2
    exit 2
fi

stage_linux_root "$1"
