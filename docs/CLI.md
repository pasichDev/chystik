# Chystik CLI

`chystik` is the terminal frontend over the same `chystik-core` scan,
exclusion, guard, and native-Trash policy used by `chystik-gui`. It does not
start a GUI, daemon, Python process, classifier, or network service.

## Commands

```text
chystik scan [ROOT...] [OPTIONS]
chystik explain PATH
chystik clean [ROOT...] --safe [OPTIONS]
chystik report [ROOT...] --format json|jsonl [OPTIONS]
chystik config show|path|reset
chystik completion bash|zsh|fish|powershell
chystik version
```

`ROOT` may be repeated and `--root ROOT` is an equivalent explicit form. If
no root is supplied, the current directory is used. Every root is made
absolute, verified as an existing directory, and de-duplicated so a child is
never scanned twice.

Shared scan options are `--safe`, `--severity safe|moderate|risky`,
`--category CATEGORY`, `--min-size SIZE`, `--include-advisories`,
`--exclude PATH`, and `--sort size|age|severity|path`. Sizes accept bytes or
`KiB`, `MiB`, and `GiB` suffixes. The category values are stable snake_case
identifiers such as `package_caches` and `build_artifacts`.

`scan` defaults to compact human output. `--format json` produces one JSON
document and `--format jsonl` emits progress, each finding, then a final
summary line. JSONL is deliberately discovery-ordered: sorting it would
require retaining the full scan, which defeats streaming. `report` has the
same JSON/JSONL schemas and is intended for automation. Successful machine
payloads go only to stdout; diagnostics and versioned machine errors go only
to stderr. No ANSI sequences are emitted in either machine format.

In an interactive terminal, human `scan` opens a proper alternate-screen TUI:
an animated loader, live counters, a colour-coded `SIZE / SEVERITY / CATEGORY / PATH`
table, and keyboard navigation. `q`, Escape, or Ctrl-C cancels an
active scan; after it finishes, use `↑/↓` (or `j`/`k`) to browse and Enter,
Escape, or `q` to close. Human `clean` uses the same TUI while it scans, then
returns to the normal confirmation manifest.

`--no-tui`, `--quiet`, redirected output, and non-interactive SSH sessions use
the readable line-progress fallback instead. JSON remains one clean stdout
document and JSONL remains the live automation protocol; neither receives TUI
escapes or status text. For machine consumers that need progress, use `scan`
or `report --format jsonl`. `--no-color` keeps the TUI but renders it without
semantic colours.

Every level supports `--help`; the root and long-running commands include
copy-pasteable examples, format rules, and the safety contract. For example:

```bash
chystik --help
chystik scan --help
chystik clean --help
```

Examples:

```bash
chystik scan --safe --min-size 100MiB
chystik scan ~/work --category build_artifacts --format json
chystik report ~/work --format jsonl > chystik-report.jsonl
chystik explain ~/.cache/go-build
```

## Cleanup contract

`clean` is intentionally narrower than scan:

```bash
chystik clean ~/work --safe --dry-run
chystik clean ~/work --safe
chystik clean ~/work --safe --interactive
chystik clean ~/work --safe --yes
```

- `--safe` is required. Only `safe` findings can enter the plan; moderate and
  risky findings are listed as skipped and can never join a bulk cleanup.
- The default is a manifest followed by a terminal confirmation. A pipe or
  redirected stdin is cancelled rather than guessed.
- `--dry-run` renders a manifest and never calls the remover.
- `--interactive` asks about each eligible manifest item, then asks for final
  confirmation.
- `--yes` is valid only with `clean --safe --yes`. It still runs persisted
  exclusions, advisory rejection, root ownership, the guard, identity
  re-check, and native-Trash capability checks. It is refused until the user
  has acknowledged the current safety policy during an interactive cleanup.
- Cleanup uses the platform's native Trash/Recycle Bin only. The CLI never
  invokes `unlink`, `remove_file`, `remove_dir_all`, a shell, `sudo`, or a
  permanent-delete fallback.

The guard runs once when the manifest is built and again immediately before
each item reaches the native Trash adapter. A changed path, protected path,
symlink/reparse point, advisory, exclusion, or missing owning root is skipped.
Partial cleanup is reported rather than hidden.

## Machine schemas and exit codes

Every JSON document and every JSONL record contains this stable metadata:

```json
{
  "schema_version": 1,
  "chystik_version": "0.1.0",
  "platform": "linux",
  "generated_at": "2026-08-25T18:42:15.123Z"
}
```

`generated_at` is an RFC 3339 UTC timestamp. `scan` and `report` additionally
contain the normalized absolute `roots` array; a JSONL `summary` contains the
same roots after the streamed findings. Stable `kind` values are `scan`,
`report`, `progress`, `finding`, `summary`, `cleanup_preview`, `cleanup`, and
`config`. Paths are absolute UTF-8 path strings where the platform can
represent them; consumers must treat them as host paths, not portable
identifiers. Severity, category, plan skip reason, and cleanup skip reason are
stable snake_case enums.

For `--format json` and `--format jsonl`, argument and runtime failures write
one versioned document to stderr and never add an error payload to stdout. A
JSONL command can already have streamed valid records before a later failure:

```json
{
  "schema_version": 1,
  "chystik_version": "0.1.0",
  "platform": "linux",
  "generated_at": "2026-08-25T18:42:15.123Z",
  "kind": "error",
  "exit_code": 2,
  "message": "invalid input: …"
}
```

| Code | Meaning |
| ---: | --- |
| 0 | Successful command; an empty scan is successful. |
| 1 | Operational failure or partial cleanup. |
| 2 | Invalid arguments or unusable configuration. |
| 3 | User cancelled confirmation/selection. |
| 4 | Policy refused every requested cleanup item. |
| 5 | `Ctrl-C`, `SIGTERM`, or `SIGHUP` interrupted a scan. |

The CLI installs one cross-platform interruption handler per process. It asks
the scanner to stop and does not start cleanup after an observed interrupt.

## Configuration, completions, and manual

`chystik config show` prints a versioned `kind: "config"` document whose
`config` field is the effective persisted policy; `config path` prints its
platform-native location, and `config reset` writes an empty policy without
deleting arbitrary files. Existing GUI `consent.json` and
`exclusions.json` records are read once when the new `config.json` does not
yet exist, so never-touch paths remain protected while upgrading.

Completion scripts are generated by the installed binary and should be
regenerated after upgrades:

```bash
source <(chystik completion bash)
source <(chystik completion zsh)
chystik completion fish > ~/.config/fish/completions/chystik.fish
chystik completion powershell > chystik.ps1
```

Linux packages install `chystik`, `chystik-gui`, Bash/Zsh/Fish/PowerShell
completion files, and the generated `chystik(1)` manual. Run `man chystik`
after installation. The manual is generated from the parser during the build,
so its command reference cannot drift from `--help`.
