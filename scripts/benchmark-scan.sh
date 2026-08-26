#!/usr/bin/env bash
# Measure scanning only. This script never calls `chystik clean`.
set -euo pipefail

usage() {
    echo "usage: $0 /absolute/fixture-directory [--runs N]" >&2
    exit 2
}

[[ $# -ge 1 ]] || usage
fixture="$1"
shift
runs=5
if [[ $# -gt 0 ]]; then
    [[ $# -eq 2 && "$1" == "--runs" && "$2" =~ ^[1-9][0-9]*$ ]] || usage
    runs="$2"
fi

[[ "$fixture" == /* && -d "$fixture" ]] || usage
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Build before timing. Otherwise a first `cargo run` would measure compilation,
# not scanning. The released binary is the thing a user downloads and runs.
cargo build --quiet --release -p chystik-cli
scan=("$repo_root/target/release/chystik" scan "$fixture" --format json --min-size 0)

run_once() {
    CHYSTIK_TEST_HOME="$fixture/home" \
    XDG_CACHE_HOME="$fixture/home/.cache" \
    "${scan[@]}" >/dev/null
}

if command -v hyperfine >/dev/null 2>&1; then
    quoted="$(printf '%q ' "${scan[@]}")"
    command="CHYSTIK_TEST_HOME=$(printf '%q' "$fixture/home") XDG_CACHE_HOME=$(printf '%q' "$fixture/home/.cache") $quoted >/dev/null"
    hyperfine --warmup 1 --runs "$runs" --shell=bash "$command"
    exit 0
fi

echo "hyperfine not found; using POSIX time for $runs scan-only runs" >&2
for run in $(seq 1 "$runs"); do
    echo "run $run/$runs" >&2
    time run_once
done
