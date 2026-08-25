#!/usr/bin/env bash
# Build a Fedora/RHEL RPM from the common Linux staging tree.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

require_command rpmbuild
version="$(release_version)"
[[ "$version" =~ ^[0-9A-Za-z.+~]+$ ]] || {
    echo "unsupported RPM package version: $version" >&2
    exit 2
}

dist_dir="${DIST_DIR:-$REPO_ROOT/dist}"
mkdir -p -- "$dist_dir"
output="$dist_dir/chystik-${version}-1.x86_64.rpm"
[[ ! -e "$output" ]] || {
    echo "refusing to overwrite existing artifact: $output" >&2
    exit 2
}

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
for directory in BUILD BUILDROOT RPMS SOURCES SPECS SRPMS; do
    mkdir -p "$work_dir/$directory"
done
source_dir="$work_dir/SOURCES/chystik-$version"
mkdir "$source_dir"
stage_linux_root "$source_dir"
tar -C "$work_dir/SOURCES" -czf "$work_dir/SOURCES/chystik-$version.tar.gz" "chystik-$version"
sed "s/@VERSION@/$version/g" "$script_dir/chystik.spec.in" > "$work_dir/SPECS/chystik.spec"

rpmbuild --define "_topdir $work_dir" --target x86_64 -bb "$work_dir/SPECS/chystik.spec"
rpm_file="$(find "$work_dir/RPMS" -type f -name 'chystik-*.x86_64.rpm' -print -quit)"
[[ -n "$rpm_file" ]] || {
    echo 'rpmbuild did not produce an x86_64 chystik RPM' >&2
    exit 1
}
cp "$rpm_file" "$output"
echo "Built $output"
