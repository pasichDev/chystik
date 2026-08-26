# Contributing to Chystik

## The short version

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must be clean. CI runs exactly these.

## Linux release packaging

Linux packages share one staging root, so change it before changing an
individual package format. The local contract checks are:

```bash
bash packaging/linux/tests/stage-layout.sh
bash packaging/linux/tests/native-packages.sh
bash packaging/linux/tests/appimage-builder.sh
bash packaging/linux/tests/arch-metadata.sh
bash packaging/linux/tests/version-contract.sh
bash packaging/linux/tests/release-notes.sh
```

`native-packages.sh` builds the Debian package locally. CI runs it with
`PACKAGING_TEST_RPM=1`, because the RPM tools are intentionally supplied by
the release environment. The CI packaging job builds all artifacts, extracts
them, and launches each one with an empty temporary home under Xvfb; it does
not scan or clean a contributor's home directory.

Do not change the linuxdeploy version or SHA-256 values in
`packaging/linux/build-appimage.sh` without updating both together and testing
the real AppImage build. The Arch recipe is source metadata; before an AUR
release, replace its `SKIP` checksum with the checksum of the immutable tag
archive.

The workspace version in `Cargo.toml`, `packaging/arch/PKGBUILD`'s `pkgver`,
the matching `CHANGELOG.md` section, and a release tag must be identical:
`0.1.0` becomes `v0.1.0` and `## [0.1.0] - YYYY-MM-DD`. The release workflow
refuses a mismatch, a missing changelog section, a tag outside `main`, or an
existing GitHub Release. Stable `MAJOR.MINOR.PATCH` tags are the only release
inputs; a tag creates the Release from that changelog section only after the
Linux and both portable Windows artifacts succeed, then uploads them with one
`SHA256SUMS` manifest.

### Cutting a release

After the version/changelog PR is merged, tag the exact `main` commit with an
annotated tag and push only that tag:

```bash
git switch main
git pull --ff-only origin main
git tag -a v0.1.0 -m "Chystik 0.1.0"
git push origin v0.1.0
```

The tag starts the release workflow. Do not move or reuse a release tag. If a
published artifact is wrong, make a patch release with a new version and a new
changelog section instead.

## Adding a cleanup rule

This is the most useful contribution and usually the smallest.

Rules live in `crates/chystik-core/src/rules/`, one module per domain. Most are
a single entry in that module's `HOME_RULES` table:

```rust
HomeRule {
    rel: ".cache/your-tool",
    category: Category::PackageCaches,
    severity: SAFE,
    note: "Your Tool cache — refetched on the next run",
},
```

Four things a reviewer will check:

1. **The path is specific.** `.cache/your-tool`, never `.cache`. A rule that
   can match a directory the user cares about is a bug, not a feature.
2. **The severity is honest.** *Safe* means the tool rebuilds it with no user
   action. If a build fails until something is re-downloaded, that is
   *Moderate*. If it cannot come back automatically, *Risky*.
3. **The note says what it is and how it returns.** The note is the whole
   product — it is what a user reads before deciding. "cache" is not a note.
4. **There is a test.** Add one to the module's `mod tests`. A rule with no
   test is a rule nobody can safely change later.

Rules that need a project marker (a lockfile, a manifest) go in `core.rs`
instead — see `marker_rule` there, and note the `VENDOR_TREES` exclusion:
`node_modules` shipped inside an application is *not* restorable by
`npm install`.

Cross-platform developer and driver rules live in `rules/catalog.rs`. They
must have a stable `rule_id`, exact platform locator, `FindingPolicy`, concrete
recovery cost, primary vendor/upstream `source_url`, and a sibling-negative
test. `DirectSafe` is the only policy allowed into `clean --safe`; use
`DirectReview` for an explicit manual decision and `AdvisoryOnly` or
`VendorCommandOnly` when the owning tool must clean it. Never add a broad
application, profile, driver-store, project, virtual-environment, or system
directory as a cleanup candidate. Treat third-party cleaner databases as
research leads only: write original locators and explanations from primary
vendor/upstream documentation; do not copy their rule text or metadata.

### Order matters

`rules::classify` tries modules in a fixed order and the first match wins. If
your rule never fires, check whether an earlier module already claims the path;
the ordering rationale is documented in `rules/mod.rs`.

## Adding a language

No Rust changes needed:

1. Copy `crates/chystik-gui/locales/en.json` to your language code.
2. Translate the values. Keep every `{placeholder}` — a test fails if one is
   dropped, because a button that loses `{size}` silently stops showing the
   number.
3. Add the variant to `Lang` in `crates/chystik-gui/src/i18n.rs` (three lines:
   the enum, `code()` and `source()`).

`cargo test -p chystik-gui` verifies that the file parses, that every category
and severity is present, and that nothing was left in English.

## Changing the safety guard

`crates/chystik-core/src/guard.rs` is the last line of defence before a
deletion. Changes there need a test showing both what is now allowed and what
is still refused. Widening the `.config` allowlist in particular requires
naming, per entry, why that path holds nothing a user would miss.

The crate-level test `every_home_rule_targets_a_path_the_guard_allows` asserts
that rules and the guard cannot silently disagree. Do not delete it.

## Code style

- `cargo fmt` decides formatting; do not hand-align.
- Comments explain *why*, not *what*. The code already says what.
- Prefer a table entry over a new code path.
- GUI code is split by responsibility (`panels`, `modals`, `widgets`, `state`,
  `theme`, `format`). A new dialog belongs in `modals.rs`, not in `app.rs`.

## Commits and pull requests

- One logical change per commit; present-tense summary line.
- Say what you tested. "Scanned my own `$HOME`, 3 new findings, all correct"
  is worth more than a paragraph of description.
- Note it explicitly if a change affects what can be deleted.

## Regenerating the icon

`assets/` is generated. Edit the constants in `packaging/render-icon.py` and
re-run it; CI fails if the committed assets do not match the script.
