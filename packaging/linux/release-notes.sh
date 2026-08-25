#!/usr/bin/env bash
# Extract one version's human-maintained release notes without leaking history.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

if [[ "$#" -gt 1 ]]; then
    echo "usage: $0 [vMAJOR.MINOR.PATCH]" >&2
    exit 2
fi

tag="${1:-$(release_tag)}"
verify_release_tag "$tag"
version="$(release_version)"
changelog="${CHANGELOG_PATH:-$REPO_ROOT/CHANGELOG.md}"

[[ -f "$changelog" ]] || {
    echo "missing changelog: $changelog" >&2
    exit 2
}

notes="$(awk -v version="$version" '
    {
        heading = "## [" version "] - "
        release_date = substr($0, length(heading) + 1)
    }
    index($0, heading) == 1 && release_date ~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/ {
        in_section = 1
        next
    }
    in_section && /^## / { exit }
    in_section { print }
' "$changelog")"

if [[ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]]; then
    echo "CHANGELOG.md has no non-empty section for $tag" >&2
    exit 2
fi

printf '%s\n' "$notes"
