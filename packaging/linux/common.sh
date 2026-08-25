#!/usr/bin/env bash
# Shared, deliberately small contract for Linux package builders.
set -euo pipefail

PACKAGING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$PACKAGING_DIR/../.." && pwd)"

release_version() {
    local version
    version="$(sed -n 's/^version = "\([0-9][0-9A-Za-z.+~-]*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n 1)"
    [[ -n "$version" ]] || {
        echo 'could not read [workspace.package].version from Cargo.toml' >&2
        return 1
    }
    printf '%s\n' "$version"
}

release_tag() {
    printf 'v%s\n' "$(release_version)"
}

verify_release_tag() {
    local tag="$1" version
    version="$(release_version)"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
        echo "release packaging supports stable MAJOR.MINOR.PATCH versions only; found $version" >&2
        return 2
    }
    [[ "$tag" == "v$version" ]] || {
        echo "release tag $tag does not match Cargo.toml version $version (expected v$version)" >&2
        return 2
    }
}

release_arch() {
    case "$(uname -m)" in
        x86_64|amd64) printf '%s\n' 'x86_64' ;;
        *)
            echo "Linux release packaging currently supports x86_64 only; found $(uname -m)" >&2
            return 1
            ;;
    esac
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "missing required command: $1" >&2
        return 127
    }
}

system_library_path() {
    local soname="$1" path
    require_command ldconfig
    path="$(ldconfig -p 2>/dev/null | awk -v soname="$soname" '$1 == soname { print $NF; exit }')"
    [[ -n "$path" && -f "$path" ]] || {
        echo "required shared library $soname is not installed" >&2
        return 1
    }
    printf '%s\n' "$path"
}

build_gui() {
    local target_dir="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
    local binary="$target_dir/release/chystik-gui"
    if [[ "${PACKAGING_SKIP_BUILD:-0}" == '1' ]]; then
        test -x "$binary" || {
            echo "PACKAGING_SKIP_BUILD=1 requires $binary" >&2
            return 1
        }
        return
    fi
    cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --release --package chystik-gui --locked
}

gui_binary_path() {
    printf '%s\n' "${CARGO_TARGET_DIR:-$REPO_ROOT/target}/release/chystik-gui"
}

ensure_empty_directory() {
    local destination="$1"
    [[ -n "$destination" ]] || {
        echo 'staging destination must not be empty' >&2
        return 2
    }
    destination="$(realpath -m -- "$destination")"
    [[ "$destination" != '/' && "$destination" != "$REPO_ROOT" ]] || {
        echo "refusing to stage into $destination" >&2
        return 2
    }
    mkdir -p -- "$destination"
    if find "$destination" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
        echo "staging destination must be empty: $destination" >&2
        return 2
    fi
}

stage_linux_root() {
    local destination="$1"
    local icon size

    release_arch >/dev/null
    build_gui
    ensure_empty_directory "$destination"
    destination="$(realpath -m -- "$destination")"

    install -Dm755 "$(gui_binary_path)" "$destination/usr/bin/chystik-gui"
    install -Dm644 "$REPO_ROOT/packaging/chystik.desktop" "$destination/usr/share/applications/chystik.desktop"
    install -Dm644 "$REPO_ROOT/LICENSE" "$destination/usr/share/licenses/chystik/LICENSE"
    for icon in "$REPO_ROOT"/assets/icons/chystik-*.png; do
        size="${icon##*-}"
        size="${size%.png}"
        install -Dm644 "$icon" "$destination/usr/share/icons/hicolor/${size}x${size}/apps/chystik.png"
    done
    install -Dm644 "$REPO_ROOT/assets/icon.svg" \
        "$destination/usr/share/icons/hicolor/scalable/apps/chystik.svg"
}
