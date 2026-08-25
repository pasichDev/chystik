# Platform support

Chystik has one shared deterministic engine. The target-selected
`chystik_core::platform` adapter owns host paths, storage volumes, protected
roots, allocated-byte accounting and cleanup capability. No frontend reads
`/proc`, XDG variables or Win32 APIs directly.

| Platform | Scan | Cleanup | Artifact / evidence |
|---|---|---|---|
| Linux x86_64 | Supported | Supported through native desktop trash | PR CI builds and smoke-tests AppImage, `.deb`, and `.rpm`; Ubuntu, Fedora, and Arch build/runtime dependencies are checked separately. |
| macOS | Preview | Supported through native Trash | Native macOS CI compiles and tests the Trash flow. No signed/notarized app artifact yet. |
| Windows 10/11 x64 | Preview | Supported through native Recycle Bin | Windows x64 CI runs guard/junction and Recycle Bin item/byte evidence; tags build a portable ZIP. Hosted CI is Windows Server 2022, so a real Windows 10 desktop acceptance run remains required. |
| Windows 11 ARM64 | Preview | Supported through native Recycle Bin | Native Windows 11 Arm runner builds, tests, and publishes a portable ARM64 ZIP. No signed installer yet. |
| Other | Unsupported | Disabled (scan-only) | Builds retain a conservative fallback adapter. |

## Why cleanup is intentionally asymmetric

The user-visible scan may be useful before cleanup is safe. A platform only
advertises `CleanupSupport::NativeTrash` after it proves both a native recovery
mechanism and its symlink/reparse-point contract with real integration tests.
There is no direct-delete fallback. On a scan-only platform `clean` reports
each requested item as unavailable and never calls a remover; the GUI disables
the same action.

Linux and macOS use native recovery mechanisms. Windows uses the Shell file
operation with `FOFX_RECYCLEONDELETE`, which fails the operation when a volume
cannot recycle an item instead of falling back to permanent deletion. Native CI
then verifies that a disposable fixture increases the documented Recycle Bin
item and byte totals. The common guard refuses links; the Windows adapter additionally refuses
every reparse point (including junctions and mount points) during scan, privacy
probing, validation and the identity re-check. Its identity is the native
volume-serial/file-index pair, opened without following a reparse point.

The resulting Windows support is deliberately still marked Preview: hosted
GitHub has a Windows Server x64 image and a Windows 11 Arm image, but no
Windows 10 desktop image. The x64 artifact targets 64-bit Windows 10 and 11;
release acceptance should include a real Windows 10 machine before changing
that row to Supported. ZIP archives are portable and unsigned until a signing
certificate and installer policy are introduced.

## Extension boundary

```text
platform adapter  →  scanner / rules  →  findings  →  GUI or future CLI
                         │                  │
                         └── guard → cleaner ┘
                               (safety authority)
                                      ↑
                         future classifier may rank only
```

- The rule engine, severity model, guard and cleaner are deterministic shared
  core. GUI and the future CLI must call them, not reimplement their own scan
  or delete behavior.
- A future local classifier receives `Finding` data to rank or explain. It
  cannot make an unsafe item actionable, modify guard decisions, or call a
  remover.
- Host-specific code stays private below `chystik_core::platform`. New
  platform capability requires an adapter and target-native evidence; it does
  not add `cfg` branches to the GUI, CLI or rules.

## Windows privacy and disks contract

Privacy traces are fixed paths, never profile globs. `%APPDATA%` (roaming) and
`%LOCALAPPDATA%` (local) are resolved by the Windows adapter, and the exact
same base becomes the guard root when the user chooses **Move to Trash**. A
redirected profile therefore remains usable without widening cleanup to a
guessed parent directory.

| Profile base | Explicit traces |
|---|---|
| `%APPDATA%` | PSReadLine ConsoleHost history; Windows Recent Items |
| `%LOCALAPPDATA%` | Default-profile Chrome, Chromium, and Edge history plus cookie databases |

Firefox profiles, thumbnail globs, unknown browser profiles, and arbitrary
application data are intentionally absent. Their layouts need a separately
audited selection rule before they can be offered for cleanup.

The Disks view enumerates only fixed and removable logical volumes through
Win32, reports their real capacity/free space, labels roots such as `C:\\`
correctly, and excludes network drives. System Volume Information and each
volume's `$Recycle.Bin` are protected roots, so selecting `C:\\` never turns
the recovery store into a scan target.

## Linux distribution contract

Linux releases target x86_64 only for now. The project builds four distribution
paths from one staging tree:

| Audience | Distribution path | Evidence |
|---|---|---|
| Generic Linux desktop | `Chystik-<version>-x86_64.AppImage` | Bundled with pinned linuxdeploy + GTK plugin; launched in an empty fixture home under Xvfb. |
| Debian / Ubuntu | `chystik_<version>_amd64.deb` | `dpkg-deb` metadata, extraction, and fixture-home launch smoke. |
| Fedora / RHEL compatible | `chystik-<version>-1.x86_64.rpm` | `rpmbuild` metadata, extraction, and fixture-home launch smoke. |
| Arch | `packaging/arch/PKGBUILD` source recipe | Arch container compiles the GUI with GTK dependencies; AUR publication requires a maintainer and a release-tarball checksum. |

Ubuntu, Fedora, and Arch CI checks compile the GUI against their GTK/runtime
dependencies. This is deliberately narrower than “every Linux distribution”:
unlisted distributions, old glibc releases, non-x86_64 machines, exotic
desktop sessions, disconnected mounts, and nonstandard trash backends are not
certified merely because an artifact starts there.

The existing `packaging/install.sh` remains the source-tree desktop installer.
Release packages include the same desktop entry, hicolor icons, binary, and
MIT license without requiring root at runtime.

## Release and rollback policy

A stable tag matching the exact Cargo version — for example `v0.1.0` for
`version = "0.1.0"` — starts the release workflow. The tagged commit
must already be on `main`, and `CHANGELOG.md` must contain the matching dated
section `## [0.1.0] - YYYY-MM-DD`; that section becomes the GitHub Release
body. The workflow validates every artifact before creating a release with the
AppImage, its SHA-256 sidecar, the `.deb`, the `.rpm`, Windows x64 and ARM64
portable ZIPs, and a `SHA256SUMS` manifest for all distributable files. Manual
workflow runs upload review artifacts only; they do not publish a release.

If an artifact is wrong, remove that release asset and its checksum reference,
mark the release as a bad build, fix the source, then publish a new tag. The
workflow never overwrites an existing asset silently. Never advise users to
delete files outside their desktop trash as part of recovery. Arch maintainers
must replace the PKGBUILD `SKIP` checksum with the published tag archive
checksum before AUR publication.
