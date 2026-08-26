# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [0.2.3] - 2026-08-26

### Added

- Compact colour recovery markers in the cleanup table, with the full
  recovery meaning available on hover.

### Fixed

- Keep cleanup-table columns fixed while long paths and descriptions scroll
  into view, so item details cannot push adjacent fields sideways.
- Separate command-copy feedback from finding detail tooltips; copied commands
  now show stable inline confirmation instead of a flashing one-frame modal.
- Refine the cleanup and privacy footer action area with a single divider and
  intentional space above its controls.

## [0.2.2] - 2026-08-26

### Added

- Declarative, build-validated catalog rules with reviewed TOML sources,
  property tests for path guards, seed corpus and bounded cargo-fuzz workflow.
- Reproducible scan-only benchmark fixture and methodology, GitHub Issue Forms,
  safety-oriented PR checklist, release badges and generated website version.

### Changed

- Recovery cost and cleanup eligibility are now presented as separate axes in
  the GUI, CLI, docs and catalog schema. Legacy `--safe` remains compatible
  and now explicitly means auto-cleanable under Chystik policy.

## [0.2.1] - 2026-08-26

### Added

- Catalog-backed findings now include their review date and explicit path or
  ownership conditions in JSON/JSONL and verbose CLI/GUI evidence views.

### Changed

- Resolve catalog roots, environment overrides, and exact targets once per
  scan; the scanner now reuses that immutable rule engine for every candidate.
- The GUI consumes the shared streaming scan event contract directly and no
  longer maintains a duplicate final findings buffer.
- Catalog tests now enforce unique rule IDs, HTTPS sources, review dates,
  declared conditions, exact platform fixtures, and precedence over legacy
  rules. Removed obsolete implementation-era ownership markers.

## [0.2.0] - 2026-08-26

### Added

- Evidence-backed cross-platform catalog for pip, CocoaPods, ccache, sccache,
  vcpkg archives, NVIDIA OptiX, Windows driver installer staging, and Xcode
  caches. Findings now carry policy, recovery cost, and upstream source in
  JSON/JSONL and verbose CLI/GUI detail views; vendor-managed caches stay
  advisory-only.
- `chystik` CLI with read-only scan/report/explain commands, versioned JSON and
  streaming JSONL, shell completions, generated manual, and documented exit
  codes.
- Interactive terminal scan table with live animation, counters, keyboard
  navigation, `--no-tui` fallback, and clean terminal restoration.
- Safe native-Trash CLI cleanup manifests with dry-run, terminal confirmation,
  interactive selection, persisted consent, exclusions, and guard revalidation.
- Machine output now carries version, platform, and generation-time metadata;
  machine argument/runtime errors have a versioned stderr document.
- Project website in `site/`, published to GitHub Pages from `main` by
  `pages.yml`, which refuses to deploy a page whose local assets are missing or
  whose screenshot has drifted from the one in `README.md`.
- Application screenshots in `docs/img/`, in English and Ukrainian.

### Changed

- Moved root normalization, filtering, streaming scan, cleanup planning, and
  policy persistence into shared `chystik-core` application services used by
  both the GUI and CLI.
- Linux packages stage the CLI, manual, and completions; Windows release ZIPs
  carry separate `Chystik-GUI.exe` and `chystik.exe` binaries.
- Native Trash CI proves the consented `chystik clean --safe --yes` route moves
  only an eligible disposable fixture.

## [0.1.0] - 2026-08-25

### Added

- Safety-first cleanup GUI with scan-first review, path guards, and recoverable cleanup.
- Native desktop Trash cleanup on Linux, macOS, and Windows Recycle Bin, with junction/reparse-point refusal.
- Disk capacity and privacy-trace views with platform-aware safety policy, including Windows roaming and local profiles.
- Versioned x86_64 Linux AppImage, Debian, RPM, Arch source-package, and portable Windows x64/ARM64 release paths.
