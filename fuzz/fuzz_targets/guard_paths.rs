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

// Pure guard helpers only: no files are opened, created, moved or deleted.
fuzz_target!(|data: &[u8]| {
    let path = path_from_input(data);
    let _ = chystik_core::guard::lexical_path_contains_protected_name(&path);
    let _ = chystik_core::guard::normalize_lexically(&path);
});
