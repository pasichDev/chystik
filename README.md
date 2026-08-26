<div align="center">

<img src="assets/icons/chystik-128.png" width="96" height="96" alt="">

# Chystik

**A safety-first disk-cleanup tool that knows what a directory *is*.**

[![CI](https://github.com/pasichDev/chystik/actions/workflows/ci.yml/badge.svg)](https://github.com/pasichDev/chystik/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/pasichDev/chystik?display_name=tag&sort=semver)](https://github.com/pasichDev/chystik/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platforms](https://img.shields.io/badge/platform-Linux%20supported%20%C2%B7%20macOS%20preview%20%C2%B7%20Windows%20preview-5369a5)](docs/SUPPORT.md)

[**Download latest release**](https://github.com/pasichDev/chystik/releases/latest) · [Website](https://pasichdev.github.io/chystik/) · [Support matrix](docs/SUPPORT.md)

</div>

---

Disk analysers like QDirStat and Filelight tell you a directory is 8 GB. They
do not tell you that it is `~/.cache/go-build`, that Go rebuilds it on the next
compile, and that deleting it costs you forty seconds. Chystik does.

It classifies what it finds into fifteen categories, states both what recovery
costs and whether Chystik may clean the exact item automatically, and — where
native-trash safety is verified — moves what you choose to the desktop trash,
never straight to `unlink`.

## What it does

- **Understands, not just measures.** ~200 rules recognise package caches,
  build outputs, downloaded toolchains, AI model weights, agent transcripts,
  container layers and desktop junk — each with a sentence explaining what it
  is and how it comes back.
- **Explains recovery.** *Automatic* regenerates on its own. *Rebuild /
  redownload* costs time. *Manual / irreplaceable* cannot return on its own
  and is never bulk-selectable.
- **Shows its cleanup authority.** Catalog-backed findings state the exact
  rule, recovery class and upstream source. *Auto-cleanable* may join an
  automatic cleanup; *review required*, *tool-managed* and *advisory only*
  remain deliberate user decisions.
- **Refuses to do damage.** Every path passes through a guard before deletion:
  system directories, `.git`, `.ssh`, `.gnupg` and your settings are rejected
  outright, symlinks and Windows reparse points are never followed, and the
  scan root itself is never deletable.
- **Fail-closed cleanup.** Linux, macOS and Windows deletion go through their
  native desktop recovery mechanisms. On a platform without a verified
  native-trash adapter, cleanup is disabled rather than falling back to direct
  deletion.
- **Pruned parallel scan.** Chystik classifies and prunes subtrees during its
  parallel `jwalk` walk. See the reproducible [benchmark methodology and
  reference measurements](docs/BENCHMARKS.md); timing depends on the machine,
  filesystem, target and cache state.
- **English and Ukrainian**, detected from your locale.

See [platform support](docs/SUPPORT.md) for the current Linux/macOS/Windows
capabilities and release status.

## Screenshot

The category rail on the left answers *"what is worth doing?"*; the detail pane
answers *"what recovery costs, and may Chystik clean this?"*.

![Chystik after a full scan of a Linux system: the category rail on the left,
findings with path, size, recovery class and last-used date on the
right.](docs/img/screenshot-gui.png)

Every row carries the sentence explaining what it is and how it comes back —
`Go build cache — rebuilt automatically by 'go build'`. Findings that need a
command you run yourself (old snap revisions, unused Flatpak runtimes) show
that command instead of a checkbox, because Chystik will not run it for you.

The interface follows your locale; the same view in Ukrainian is at
[docs/img/screenshot-gui-uk.png](docs/img/screenshot-gui-uk.png).

## Install

### From source

Requires a Rust toolchain (1.80 or newer) and the usual desktop development
libraries.

```bash
git clone https://github.com/pasichDev/chystik
cd chystik
cargo build --release
```

The binaries land at `target/release/chystik-gui` and `target/release/chystik`.

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
for your distribution; all three contain the GUI, `chystik` CLI, generated
manual/completions where the format supports them, and the same trash-only
cleanup contract.

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

### Windows release artifacts

Tagged releases also publish portable ZIP archives. Extract one archive and
run `Chystik-GUI.exe` for the desktop app or `chystik.exe` for the terminal;
no installer, administrator rights, or direct-delete mode is involved.

```powershell
Expand-Archive .\Chystik-<version>-windows-x86_64.zip -DestinationPath .\Chystik
.\Chystik\Chystik-GUI.exe
.\Chystik\chystik.exe scan --safe
```

`windows-x86_64` is the release target for 64-bit Windows 10 and Windows 11.
`windows-aarch64` is the native ARM64 archive for Windows 11 on Arm. Archives
are checksummed in `SHA256SUMS`; they are not code-signed yet, so Windows may
show a SmartScreen warning. See the support matrix for the distinction between
native CI evidence and a Windows 10 desktop acceptance run.

## Usage

1. Pick what to scan. Chystik offers your real mounted volumes; add any folder
   with **Targets → Add folder**.
2. Press **Scan**. Results stream in as they are found.
3. Work through the categories in the left rail, largest first.
4. **Select auto-cleanable** ticks only exact findings eligible for automatic
   cleanup in the current category. Review-required items require a deliberate
   row selection; manual / valuable data is never bulk-selected.
5. **Move to Trash** shows a full manifest — every path, its recovery class and cleanup policy,
   and a tick showing whether the safety guard will accept it. Read it.

Keyboard: `/` focuses the filter, `Esc` closes a dialog, `Enter` confirms one.

### Command line

```
chystik-gui                       # normal launch
chystik scan --safe               # read-only terminal scan
chystik clean . --safe --dry-run  # inspect an auto-cleanable cleanup manifest
CHYSTIK_AUTOSCAN=1 …              # start GUI scan immediately (headless smoke testing)
LANG=uk_UA.UTF-8 …                # force GUI language
```

See [CLI reference](docs/CLI.md) for JSON/JSONL contracts, confirmation
policy, exit codes, completions, and `chystik(1)`.

## Safety model

This is a tool that deletes things, so the safety model is the product.

| Layer | What it does |
|---|---|
| Rules | Only match paths a rule explicitly recognises. There is no "delete anything over N GB". |
| Recovery | Every match carries the cost of losing it. Manual / irreplaceable data is excluded from bulk selection. |
| Cleanup policy & evidence | Catalog rules record whether Chystik may clean the exact path, its recovery class, and a vendor/upstream source. `clean --safe` remains compatible and means **auto-cleanable**: only `DirectSafe` automatic findings are eligible. |
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
crates/chystik-core     scanner, rules, recovery, safety guard, reporting
  src/platform/         target-selected host policy and capability seam
  src/rules/            one module per domain; declarative catalog TOML + validator
  src/guard.rs          the last line of defence before any deletion
  src/app.rs            shared roots, filters, manifests and cleanup plans
  src/config.rs         versioned consent and never-touch policy
crates/chystik-cli      scriptable frontend, JSON/JSONL, completions and man
crates/chystik-gui      the desktop application (egui/eframe)
  src/panels.rs         window regions
  src/modals.rs         dialogs, including the first-run risk acknowledgement
  locales/*.json        translations — no Rust changes needed to add a language
packaging/              desktop entry, icon renderer, installer
site/                   the GitHub Pages site, published from main by pages.yml
docs/img/               screenshots shared by this file and the site
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a cleanup rule is usually a
small declarative TOML entry plus evidence; that is the most useful
contribution.

Security issues: see [SECURITY.md](SECURITY.md).

## Licence

MIT — see [LICENSE](LICENSE).

Bundles IBM Plex Sans and IBM Plex Mono, © 2017 IBM Corp., under the SIL Open
Font License 1.1. See [NOTICE](NOTICE).
