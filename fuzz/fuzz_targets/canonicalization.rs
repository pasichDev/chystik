#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

#[cfg(unix)]
fn path_from_input(data: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(data.to_vec()))
}

#[cfg(not(unix))]
fn path_from_input(data: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(data).into_owned())
}

// Exercise the same lexical normalization used by the cleanup guard. This
// target is deliberately filesystem-free: it cannot reach Trash, unlink, a
// Recycle Bin API, or a host path no matter what bytes libFuzzer supplies.
fuzz_target!(|data: &[u8]| {
    let raw = path_from_input(data);
    let root = PathBuf::from("/chystik-fuzz-root");
    let candidate = root.join("inside").join(&raw);
    let _ = chystik_core::guard::normalize_lexically(&root);
    let _ = chystik_core::guard::normalize_lexically(&candidate);
    let _ = chystik_core::guard::lexical_path_contains_protected_name(&candidate);
});
