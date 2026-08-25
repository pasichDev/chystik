# Platform support

Chystik has one shared deterministic engine. The target-selected
`chystik_core::platform` adapter owns host paths, storage volumes, protected
roots, allocated-byte accounting and cleanup capability. No frontend reads
`/proc`, XDG variables or Win32 APIs directly.

| Platform | Scan | Cleanup | Artifact / evidence |
|---|---|---|---|
| Linux | Supported | Supported through native desktop trash | Linux CI runs formatting, clippy, unit tests, deletion integration tests and release build. |
| macOS | Preview | Supported through native Trash | Native `cargo check --all-targets` on macOS CI; cleanup uses macOS's Trash API. No signed/notarized app artifact yet. |
| Windows | Preview | Disabled (scan-only) | Native `cargo check --all-targets` on Windows CI. No signed installer yet. |
| Other | Unsupported | Disabled (scan-only) | Builds retain a conservative fallback adapter. |

## Why cleanup is intentionally asymmetric

The user-visible scan may be useful before cleanup is safe. A platform only
advertises `CleanupSupport::NativeTrash` after it proves both a native recovery
mechanism and its symlink/reparse-point contract with real integration tests.
There is no direct-delete fallback. On a scan-only platform `clean` reports
each requested item as unavailable and never calls a remover; the GUI disables
the same action.

Linux and macOS use native recovery mechanisms. Their existing guard and
identity re-check remain shared across platforms; macOS cleanup delegates to
the system Trash API. Windows still needs a Recycle Bin adapter plus tests
that cover links, reparse points, junctions, permission failures and recovery
visibility before its row can change to supported.

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

## Releases

The current desktop installer and `.desktop` integration are Linux-only.
Portable Linux, macOS and Windows distributable artifacts are a release-work
slice after native compile coverage: they require signing/notarization policy,
per-platform smoke hardware and an update/rollback design. Source builds may
be used for scan preview where the native compile CI is green.
