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
`X.Y.Z` becomes `vX.Y.Z` and `## [X.Y.Z] - YYYY-MM-DD`. The release workflow
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
git tag -a vX.Y.Z -m "Chystik X.Y.Z"
git push origin vX.Y.Z
```

The tag starts the release workflow. Do not move or reuse a release tag. If a
published artifact is wrong, make a patch release with a new version and a new
changelog section instead.

## Contributing a cleanup rule

Most evidence-backed cross-platform rules need no Rust change:

1. Add or edit one TOML file in `crates/chystik-core/rules/catalog/`.
2. Add an authoritative upstream HTTPS source and exact path evidence.
3. Add or update a positive and sibling-negative fixture when the locator is
   new or unusual.
4. Run `cargo test -p chystik-core catalog`.
5. Open a PR using the safety checklist.

Minimal copy-pasteable rule:

```toml
[[rule]]
id = "example.tool-cache"
category = "package-caches"
recovery = "automatic"
recovery_note = "the next install downloads the cache again"
cleanup_policy = "auto-cleanable"
note = "Example Tool download cache — fetched again on the next install"
source_url = "https://vendor.example/docs/cache"
reviewed_at = "2026-08-26"
preconditions = ["the path is the exact documented cache root"]

[[rule.locator]]
platform = "linux"
root = "cache"
path = "example-tool"
```

The contributor-facing terms are deliberately two axes:

- **Recovery:** `automatic`, `rebuild-redownload`, or
  `manual-irreplaceable` — what losing the data costs.
- **Cleanup policy:** `auto-cleanable`, `review-required`, `tool-managed`, or
  `advisory-only` — what Chystik is allowed to do.

Only `automatic` + `auto-cleanable` is eligible for `clean --safe` (also
`--auto-cleanable`). The build validator rejects any other combination before
a binary can be built. The guard still revalidates every selected path and can
refuse a catalog rule.

Each locator is an explicit platform-owned root plus a relative exact path.
The schema rejects duplicate IDs/locators, unknown values, non-HTTPS sources,
missing or invalid review dates, empty notes/preconditions, absolute paths,
`..`, broad root-only candidates, unreviewed environment variables and marker
rules without exact marker evidence. Binaries embed reviewed TOML at build time;
there is no rule download, local plugin or remote auto-update path.

We reject entire application-data directories, user profiles, broad cache
parents, project roots, virtualenv roots without precise semantics, `.git`,
credentials, unknown generated data, and rules justified only by another
cleaner's database. Third-party cleaner databases are research leads only:
write the locator and explanation from primary vendor/upstream documentation.

Specialized procedural rules with semantics the schema cannot express safely
(for example project markers) remain Rust in `crates/chystik-core/src/rules/`.
Do not migrate one by weakening its marker or sibling-negative checks.

### Order matters

`RuleEngine::classify_with_metadata` tries modules in a fixed order and the first match wins. If
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
