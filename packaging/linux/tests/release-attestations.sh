#!/usr/bin/env bash
# Release assets must carry GitHub-hosted SLSA provenance before publication.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
workflow="$repo_root/.github/workflows/linux-release.yml"

grep -Eq '^[[:space:]]*contents:[[:space:]]*write$' "$workflow"
grep -Eq '^[[:space:]]*id-token:[[:space:]]*write$' "$workflow"
grep -Eq '^[[:space:]]*attestations:[[:space:]]*write$' "$workflow"

verify_line="$(grep -n 'Verify complete asset set and create checksums' "$workflow" | cut -d: -f1)"
attest_line="$(grep -n 'Attest release assets' "$workflow" | cut -d: -f1)"
publish_line="$(grep -n 'Publish release assets' "$workflow" | cut -d: -f1)"
test -n "$verify_line"
test -n "$attest_line"
test -n "$publish_line"
test "$verify_line" -lt "$attest_line"
test "$attest_line" -lt "$publish_line"

grep -Fq 'uses: actions/attest@v4' "$workflow"
grep -Fq 'subject-path: |' "$workflow"
grep -Fq 'chystik-release/*.AppImage' "$workflow"
grep -Fq 'chystik-release/*.deb' "$workflow"
grep -Fq 'chystik-release/*.rpm' "$workflow"
grep -Fq 'chystik-release/*-windows-x86_64.zip' "$workflow"
grep -Fq 'chystik-release/*-windows-aarch64.zip' "$workflow"

echo "Release artifact attestation contract is valid"
