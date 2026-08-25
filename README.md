<div align="center">

<img src="assets/icons/chystik-128.png" width="96" height="96" alt="">

# Chystik

**A safety-first disk-cleanup tool that knows what a directory *is*.**

[![CI](https://github.com/pasichDev/chystik/actions/workflows/ci.yml/badge.svg)](https://github.com/pasichDev/chystik/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)

</div>

---

Disk analysers like QDirStat and Filelight tell you a directory is 8 GB. They
do not tell you that it is `~/.cache/go-build`, that Go rebuilds it on the next
compile, and that deleting it costs you forty seconds. Chystik does.

It classifies what it finds into fifteen categories, rates every item by what
losing it actually costs, and — where native-trash safety is verified — moves
what you choose to the desktop trash, never straight to `unlink`.

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
- **Fail-closed cleanup.** Linux and macOS deletion go through their native
  desktop Trash. On a platform without a verified native-trash adapter,
  cleanup is disabled rather than falling back to direct deletion.
- **Fast.** A full Linux scan of `/` on a developer machine takes about a second:
  a parallel `jwalk` walk that prunes a subtree as soon as it is classified,
  and skips pseudo, read-only and network mounts entirely.
- **English and Ukrainian**, detected from your locale.

See [platform support](docs/SUPPORT.md) for the current Linux/macOS/Windows
capabilities and release status.

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

### Linux desktop dependencies

On Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libgtk-3-dev
```

On Fedora:

```bash
sudo dnf install gcc pkgconf-pkg-config gtk3-devel
```

On Arch Linux:

```bash
sudo pacman -S --needed base-devel cargo pkgconf gtk3 libxkbcommon-x11
```

### Linux release artifacts

Tagged releases publish x86_64 artifacts on GitHub Releases. Choose the format
for your distribution; all three contain the same GUI and retain the same
trash-only cleanup contract.

Generic Linux desktops with a GTK-compatible X11 or Wayland session can use
the AppImage:

```bash
chmod +x Chystik-<version>-x86_64.AppImage
./Chystik-<version>-x86_64.AppImage
```

Debian and Ubuntu derivatives can install the Debian package:

```bash
sudo apt install ./chystik_<version>_amd64.deb
```

Fedora and RHEL-compatible distributions can install the RPM package:

```bash
sudo dnf install ./chystik-<version>-1.x86_64.rpm
```

Arch users can build the versioned source recipe in `packaging/arch`:

```bash
cd packaging/arch
makepkg -si
```

The AppImage is a portable x86_64 release target, not a claim that every
distribution, desktop environment, glibc version, or graphics stack is
certified. See the support matrix for the tested environments and limitations.

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
| Guard | `chystik_core::guard::check` runs before every deletion and refuses platform-protected roots, protected names, symlinks and anything outside the scan root. |
| Manifest | Nothing is deleted until you have seen the full list with per-item guard verdicts. |
| Capability | `chystik_core::platform` enables native-trash cleanup only where its safety contract is verified; every other target is scan-only. |

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
  src/platform/         target-selected host policy and capability seam
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
