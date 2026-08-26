# AUR handoff

`PKGBUILD` and `.SRCINFO` are a complete, source-built Arch package for the
current tagged release. The package compiles Chystik locally; it does not
download or repackage a prebuilt binary.

Before publishing, verify the exact release source and generated metadata on an
Arch machine:

```bash
cd packaging/arch
makepkg --verifysource --nobuild
makepkg --printsrcinfo | diff -u .SRCINFO -
```

Publishing to AUR requires the maintainer's AUR account and SSH key. The AUR
repository must contain only its source-package metadata, not this whole
upstream checkout:

```bash
git clone ssh://aur@aur.archlinux.org/chystik.git
cd chystik
cp /path/to/chystik/packaging/arch/PKGBUILD .
cp /path/to/chystik/packaging/arch/.SRCINFO .
git diff --check
git add PKGBUILD .SRCINFO
git commit -m 'Initial import: chystik 0.2.1'
git push
```

For every future release, update `pkgver`, replace the tarball SHA-256 with the
published tag archive checksum, regenerate `.SRCINFO` with `makepkg
--printsrcinfo`, run the two verification commands above, then commit the two
metadata files to the separate AUR repository.
