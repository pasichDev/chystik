<div align="center">

<img src="assets/icons/chystik-128.png" width="96" height="96" alt="">

# Chystik

**A disk-cleanup tool for Linux developers that knows what a directory *is*.**

[![CI](https://github.com/pasichDev/chystik/actions/workflows/ci.yml/badge.svg)](https://github.com/pasichDev/chystik/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)

</div>

---

Disk analysers like QDirStat and Filelight tell you a directory is 8 GB. They
do not tell you that it is `~/.cache/go-build`, that Go rebuilds it on the next
compile, and that deleting it costs you forty seconds. Chystik does.

It classifies what it finds into fifteen categories, rates every item by what
losing it actually costs, and moves what you choose to the desktop trash —
never straight to `unlink`.

## What it does

- **Understands, not just measures.** ~200 rules recognise package caches,
  build outputs, downloaded toolchains, AI model weights, agent transcripts,
  container layers and desktop junk — each with a sentence explaining what it
  is and how it comes back.
- **Rates by cost of loss.** *Safe* regenerates on its own. *Moderate* costs a
  re-download or a rebuild. *Risky* cannot be recreated automatically and is
  never included in a bulk selection.
- **Refuses to do damage.** Every path passes through a guard before deletion:
  system directories, `.git`, `.ssh`, `.gnupg` and your settings are rejected
  outright, symlinks are never followed, and the scan root itself is never
  deletable.
- **Trash-only.** Deletion goes through the XDG trash. Nothing is erased in
  place, so every action is reversible from your file manager.
- **Fast.** A full scan of `/` on a developer machine takes about a second:
  a parallel `jwalk` walk that prunes a subtree as soon as it is classified,
  and skips pseudo, read-only and network mounts entirely.
- **English and Ukrainian**, detected from your locale.

## Screenshot

The category rail on the left answers *"what is worth doing?"*; the detail pane
answers *"is this one safe?"*.

<!-- Add a screenshot at docs/screenshot.png and it renders here. -->

## Install

### From source

Requires a Rust toolchain (1.80 or newer) and the usual desktop development
libraries.

```bash
git clone https://github.com/pasichDev/chystik
cd chystik
cargo build --release
```

The binary lands at `target/release/chystik-gui`.

To register the desktop entry and icons for your user:

```bash
./packaging/install.sh
```

### Dependencies

On Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libgtk-3-dev
```

On Fedora:

```bash
sudo dnf install gcc pkgconf-pkg-config gtk3-devel
```

## Usage

1. Pick what to scan. Chystik offers your real mounted volumes; add any folder
   with **Targets → Add folder**.
2. Press **Scan**. Results stream in as they are found.
3. Work through the categories in the left rail, largest first.
4. **Select all safe** ticks everything non-risky in the current category.
5. **Move to Trash** shows a full manifest — every path, its size, its rating,
   and a tick showing whether the safety guard will accept it. Read it.

Keyboard: `/` focuses the filter, `Esc` closes a dialog, `Enter` confirms one.

### Command line

```
chystik-gui              # normal launch
CHYSTIK_AUTOSCAN=1 …     # start a scan immediately (headless smoke testing)
LANG=uk_UA.UTF-8 …       # force a language
```

## Safety model

This is a tool that deletes things, so the safety model is the product.

| Layer | What it does |
|---|---|
| Rules | Only match paths a rule explicitly recognises. There is no "delete anything over N GB". |
| Severity | Every match carries the cost of losing it; Risky is excluded from bulk selection. |
| Size floor | Findings under 1 MiB are dropped, so the signal is not buried in noise. |
| Guard | `chystik_core::guard::check` runs before every deletion and refuses protected prefixes, protected names, symlinks and anything outside the scan root. |
| Manifest | Nothing is deleted until you have seen the full list with per-item guard verdicts. |
| Trash | Deletion is `trash::delete`, never `remove_dir_all`. |

A crate-level test asserts that no rule can ever propose a path the guard
refuses — the two cannot silently disagree.

**It is still your disk.** Rules can be wrong about your machine. Read the
manifest.

## Categories

Build artifacts · Package caches · IDE & toolchains · AI models ·
Browser & system · Android dev · AI agents · Containers · Installers ·
Games · Media · Messengers · Cloud sync · Office · System junk

## Project layout

```
crates/chystik-core     scanner, rules, severity, safety guard, reporting
  src/rules/            one module per domain; see rules/mod.rs
  src/guard.rs          the last line of defence before any deletion
crates/chystik-gui      the desktop application (egui/eframe)
  src/panels.rs         window regions
  src/modals.rs         dialogs, including the first-run risk acknowledgement
  locales/*.json        translations — no Rust changes needed to add a language
packaging/              desktop entry, icon renderer, installer
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a cleanup rule is usually a
one-line table entry plus a test; that is the most useful contribution.

Security issues: see [SECURITY.md](SECURITY.md).

## Licence

MIT — see [LICENSE](LICENSE).

Bundles IBM Plex Sans and IBM Plex Mono, © 2017 IBM Corp., under the SIL Open
Font License 1.1. See [NOTICE](NOTICE).
