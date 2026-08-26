# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

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
