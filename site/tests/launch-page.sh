#!/usr/bin/env bash
# Keep the public landing page release-oriented and free of stale asset names.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
page="$repo_root/site/index.html"

if grep -Fq '0.1.0' "$page"; then
    echo 'landing page contains a stale 0.1.0 asset name' >&2
    exit 1
fi

grep -Fq 'https://github.com/pasichDev/chystik/releases/latest' "$page"
grep -Fq 'attestation verify' "$page"
grep -Fq 'SHA256SUMS' "$page"
grep -Fq 'Native Trash' "$page"

echo "Launch page contract is valid"
