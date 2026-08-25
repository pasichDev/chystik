#!/usr/bin/env bash
# The tag must publish only its own non-empty CHANGELOG section.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
notes_script="$repo_root/packaging/linux/release-notes.sh"
source "$repo_root/packaging/linux/common.sh"
version="$(release_version)"

notes="$($notes_script "v$version")"
printf '%s\n' "$notes" | grep -Fx '### Added' >/dev/null

fixture="$(mktemp)"
trap 'rm -f "$fixture"' EXIT
printf '%s\n' \
    '# Changelog' \
    '' \
    '## [Unreleased]' \
    '' \
    "## [$version] - 2026-08-25" \
    '### Added' \
    '- Release-specific entry.' \
    '' \
    '## [0.0.9] - 2026-08-24' \
    '### Fixed' \
    '- Older entry must not leak into the release.' \
    > "$fixture"

fixture_notes="$(CHANGELOG_PATH="$fixture" "$notes_script" "v$version")"
printf '%s\n' "$fixture_notes" | grep -Fx -- '- Release-specific entry.' >/dev/null
if printf '%s\n' "$fixture_notes" | grep -Fq -- 'Older entry'; then
    echo 'release notes included the preceding release section' >&2
    exit 1
fi

printf '%s\n' \
    '# Changelog' \
    '' \
    '## [Unreleased]' \
    > "$fixture"
if CHANGELOG_PATH="$fixture" "$notes_script" "v$version" >/dev/null 2>&1; then
    echo 'release notes accepted a missing version section' >&2
    exit 1
fi

echo "Release notes contract is valid for v$version"
