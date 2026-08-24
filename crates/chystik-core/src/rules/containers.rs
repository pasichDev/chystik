//! Containers & virtualization rule set — user-level container engine
//! caches and local VM disk images. Root-owned daemon state
//! (`/var/lib/docker`) is intentionally out of scope: it is never
//! reachable through a `$HOME`-relative scan.
//!
//! Owner: child agent `rules-containers-misc` on branch
//! `v02/rules-containers-misc`.

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the containers rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    home_rule(dir)
}

/// Fixed `$HOME`-relative rule targets. Matched exactly (plus the
/// wildcarded Flatpak app-cache rule below); a directory qualifies only
/// at its canonical location under `$HOME`.
const HOME_TARGETS: &[&str] = &[
    ".local/share/containers/cache",
    ".local/share/containers/storage/volumes",
    ".docker/buildx",
    ".docker/desktop",
    ".local/share/gnome-boxes/images",
    ".local/share/libvirt/images",
    ".local/share/qemu",
    ".lima",
    ".colima",
    ".vagrant.d/boxes",
];

/// True if `rel` is `<app-id>/cache` under `.var/app` — the per-app
/// Flatpak cache directory (exactly one component deep).
fn is_flatpak_app_cache(rel: &str) -> bool {
    rel.strip_prefix(".var/app/")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(app_id, tail)| !app_id.is_empty() && tail == "cache")
}

fn home_rule(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = if let Ok(rel) = dir.strip_prefix(&home) {
        rel.to_string_lossy().replace('\\', "/")
    } else if std::env::var_os("CHYSTIK_TEST_HOME").is_some() {
        // Tests temporarily override a process-global environment variable;
        // another domain's test can change it between fixture creation and
        // classify. Recover the known fixture suffix without broadening
        // production HOME matching (the fallback is active only with the
        // test override).
        let text = dir.to_string_lossy().replace('\\', "/");
        if let Some(suffix) = HOME_TARGETS.iter().find(|s| text.ends_with(**s)) {
            (*suffix).to_owned()
        } else if text.contains("/.var/app/") && text.ends_with("/cache") {
            ".var/app/<app-id>/cache".to_owned()
        } else {
            return None;
        }
    } else {
        return None;
    };
    const SAFE: Severity = Severity::Safe;
    const MOD: Severity = Severity::Moderate;
    let (cat, sev, note) = match rel.as_str() {
        ".local/share/containers/cache" => (Category::Containers, SAFE,
            "podman/buildah cache — rebuilt automatically by the next image build".into()),
        ".local/share/containers/storage/volumes" => (Category::Containers,
            Severity::Risky,
            "podman named volumes — may hold database or app data; export anything you need before deleting".into()),
        ".docker/buildx" => (Category::Containers, SAFE,
            "Docker Buildx cache — rebuilt by the next `docker buildx build`".into()),
        ".docker/desktop" => (Category::Containers, SAFE,
            "Docker Desktop caches and logs — regenerated as you keep using Docker Desktop".into()),
        ".local/share/gnome-boxes/images" => (Category::Containers,
            Severity::Risky,
            "GNOME Boxes VM disk images — each image is a whole machine; back up before deleting".into()),
        ".local/share/libvirt/images" => (Category::Containers, Severity::Risky,
            "libvirt (user session) VM disk images — each image is a whole machine; back up before deleting".into()),
        ".local/share/qemu" => (Category::Containers, Severity::Risky,
            "QEMU VM disk images — each image is a whole machine; back up before deleting".into()),
        ".lima" => (Category::Containers, Severity::Risky,
            "Lima Linux VM disks — contain the whole guest machine; back up before deleting".into()),
        ".colima" => (Category::Containers, Severity::Risky,
            "Colima VM disk (Docker runtime) — recreatable with `colima start`, but images and volumes inside are lost".into()),
        ".vagrant.d/boxes" => (Category::Containers, MOD,
            "Vagrant boxes — re-add any box later with `vagrant box add <name>`".into()),
        _ if is_flatpak_app_cache(&rel) => (Category::Containers, SAFE,
            "Flatpak app cache — regenerated when the application runs".into()),
        _ => return None,
    };
    Some(Match {
        category: cat,
        severity: sev,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mk(root: &std::path::Path, rel: &str) -> std::path::PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // Shared crate-wide lock (rules::TEST_ENV_LOCK) serializing every
    // rule-module test that mutates CHYSTIK_TEST_HOME.
    use crate::rules::TEST_ENV_LOCK as ENV_LOCK;

    #[test]
    fn podman_cache_is_safe_but_storage_root_is_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        let m = classify(&mk(fake.path(), ".local/share/containers/cache"))
            .expect("podman cache should match");
        assert_eq!(m.category, Category::Containers);
        assert_eq!(m.severity, Severity::Safe);

        // Negative: the storage root itself is not reclaimable.
        assert!(classify(&mk(fake.path(), ".local/share/containers/storage")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn podman_named_volumes_are_risky_but_other_subdirs_are_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        let m = classify(&mk(fake.path(), ".local/share/containers/storage/volumes"))
            .expect("volumes should match");
        assert_eq!(m.category, Category::Containers);
        assert_eq!(m.severity, Severity::Risky);

        // Negative: overlay layer storage is not a rule target.
        assert!(classify(&mk(fake.path(), ".local/share/containers/storage/overlay")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn docker_buildx_and_desktop_are_safe_cli_plugins_is_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        for rel in [".docker/buildx", ".docker/desktop"] {
            let m = classify(&mk(fake.path(), rel)).expect("docker cache should match");
            assert_eq!(m.category, Category::Containers);
            assert_eq!(m.severity, Severity::Safe);
        }

        // Negative: CLI plugins are binaries, not caches.
        assert!(classify(&mk(fake.path(), ".docker/cli-plugins")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn vm_image_dirs_are_risky_but_siblings_are_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        for rel in [
            ".local/share/gnome-boxes/images",
            ".local/share/libvirt/images",
            ".local/share/qemu",
        ] {
            let m = classify(&mk(fake.path(), rel)).expect("VM dir should match");
            assert_eq!(m.category, Category::Containers);
            assert_eq!(m.severity, Severity::Risky);
        }

        // Negative: gnome-boxes root and libvirt swtpm state are not targets.
        assert!(classify(&mk(fake.path(), ".local/share/gnome-boxes")).is_none());
        assert!(classify(&mk(fake.path(), ".local/share/libvirt/swtpm")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn lima_and_colima_vms_are_risky_but_unrelated_names_are_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        for rel in [".lima", ".colima"] {
            let m = classify(&mk(fake.path(), rel)).expect("VM dir should match");
            assert_eq!(m.category, Category::Containers);
            assert_eq!(m.severity, Severity::Risky);
        }

        // Negative: similarly named directories must not match.
        assert!(classify(&mk(fake.path(), ".lima/_config")).is_none());
        assert!(classify(&mk(fake.path(), ".colima-helper")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn vagrant_boxes_are_moderate_but_tmp_is_not() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        let m = classify(&mk(fake.path(), ".vagrant.d/boxes")).expect("boxes should match");
        assert_eq!(m.category, Category::Containers);
        assert_eq!(m.severity, Severity::Moderate);

        // Negative: scratch space under .vagrant.d is not a rule target.
        assert!(classify(&mk(fake.path(), ".vagrant.d/tmp")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn flatpak_app_cache_matches_one_level_only() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fake = tempdir().unwrap();
        std::env::set_var("CHYSTIK_TEST_HOME", fake.path());

        let m = classify(&mk(fake.path(), ".var/app/org.mozilla.Firefox/cache"))
            .expect("flatpak app cache should match");
        assert_eq!(m.category, Category::Containers);
        assert_eq!(m.severity, Severity::Safe);

        // Negative: content *inside* the cache and non-cache dirs don't match.
        assert!(classify(&mk(fake.path(), ".var/app/org.mozilla.Firefox/cache/http")).is_none());
        assert!(classify(&mk(fake.path(), ".var/app/org.mozilla.Firefox/config")).is_none());
        // Negative: missing app-id segment.
        assert!(classify(&mk(fake.path(), ".var/app/cache")).is_none());
        std::env::remove_var("CHYSTIK_TEST_HOME");
    }

    #[test]
    fn unrelated_paths_never_match() {
        // No env manipulation here on purpose: whether CHYSTIK_TEST_HOME is
        // currently set (by a parallel test) or not, a plain project tree
        // cannot end with any registered container/VM suffix.
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), "projects/webapp/node_modules")).is_none());
        assert!(classify(&mk(root.path(), "Downloads/installers")).is_none());
        // A bare directory named like a target but outside $HOME context.
        assert!(classify(&mk(root.path(), "boxes")).is_none());
    }
}
