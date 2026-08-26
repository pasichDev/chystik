use std::process::Command;

fn chystik() -> Command {
    Command::new(env!("CARGO_BIN_EXE_chystik"))
}

fn isolated(command: &mut Command, home: &std::path::Path) {
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("APPDATA", home.join("appdata"))
        .env("LOCALAPPDATA", home.join("localappdata"))
        .env("CHYSTIK_TEST_HOME", home);
}

/// Resolve the native configuration location through the CLI itself instead
/// of copying Linux paths into a test that also runs on macOS and Windows.
fn isolated_config_path(home: &std::path::Path) -> std::path::PathBuf {
    let mut command = chystik();
    command.args(["config", "path"]);
    isolated(&mut command, home);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "config path failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

#[test]
fn scan_json_writes_a_versioned_document_and_no_diagnostics_on_success() {
    let fixture = tempfile::tempdir().unwrap();
    let output = chystik()
        .args([
            "scan",
            fixture.path().to_str().unwrap(),
            "--format",
            "json",
            "--min-size",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["kind"], "scan");
    assert_eq!(document["chystik_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(document["platform"], std::env::consts::OS);
    assert!(
        document["generated_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "generated_at must be an RFC 3339 UTC timestamp: {document}"
    );
    assert_eq!(document["findings"], serde_json::json!([]));
    assert!(document["roots"][0].as_str().unwrap().starts_with('/'));
}

#[test]
fn json_runtime_errors_are_versioned_documents_on_stderr() {
    let fixture = tempfile::tempdir().unwrap();
    let missing = fixture.path().join("missing-root");
    let output = chystik()
        .args(["scan", missing.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "failed JSON must not write stdout"
    );
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["kind"], "error");
    assert_eq!(error["exit_code"], 2);
    assert_eq!(error["chystik_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(error["platform"], std::env::consts::OS);
    assert!(error["message"].as_str().unwrap().contains("missing-root"));
}

#[test]
fn json_argument_errors_are_versioned_documents_on_stderr() {
    let output = chystik()
        .args(["scan", "--format", "json", "--definitely-not-an-option"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "failed JSON must not write stdout"
    );
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["kind"], "error");
    assert_eq!(error["exit_code"], 2);
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("definitely-not-an-option"));
}

#[test]
fn jsonl_runtime_errors_stay_off_the_stream_and_are_versioned() {
    let fixture = tempfile::tempdir().unwrap();
    let missing = fixture.path().join("missing-root");
    let output = chystik()
        .args(["report", missing.to_str().unwrap(), "--format", "jsonl"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["exit_code"], 2);
    assert_eq!(error["schema_version"], 1);
}

#[test]
fn scan_safe_is_read_only_and_never_starts_a_gui_or_optional_classifier() {
    let fixture = tempfile::tempdir().unwrap();
    let output = chystik()
        .args([
            "scan",
            fixture.path().to_str().unwrap(),
            "--safe",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(document["findings"].as_array().unwrap().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn legacy_safe_and_auto_cleanable_alias_select_the_same_catalog_finding() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let cache = home.join("cache/pip");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("fixture.whl"), "x".repeat(2048)).unwrap();

    let scan = |flag: &str| {
        let mut command = chystik();
        command.args([
            "scan",
            home.to_str().unwrap(),
            flag,
            "--format",
            "json",
            "--min-size",
            "0",
        ]);
        isolated(&mut command, &home);
        command.env("XDG_CACHE_HOME", home.join("cache"));
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{flag} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        document["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| finding["provenance"]["rule_id"].clone())
            .collect::<Vec<_>>()
    };

    let legacy = scan("--safe");
    let alias = scan("--auto-cleanable");
    assert_eq!(legacy, alias);
    assert_eq!(legacy, vec![serde_json::json!("python.pip.cache")]);
}

#[cfg(target_os = "linux")]
#[test]
fn catalog_finding_exposes_policy_and_evidence_in_json_and_verbose_output() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let cache = home.join("cache/pip");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("fixture.whl"), "x".repeat(2048)).unwrap();

    let mut json = chystik();
    json.args([
        "scan",
        home.to_str().unwrap(),
        "--format",
        "json",
        "--min-size",
        "0",
    ]);
    isolated(&mut json, &home);
    json.env("XDG_CACHE_HOME", home.join("cache"));
    let output = json.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let pip = document["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["provenance"]["rule_id"] == "python.pip.cache")
        .expect("pip cache must carry catalog provenance");
    assert_eq!(pip["provenance"]["policy"], "direct_safe");
    assert!(pip["provenance"]["source_url"]
        .as_str()
        .unwrap()
        .contains("pip.pypa.io"));
    assert!(pip["provenance"]["recovery_cost"].as_str().is_some());
    assert_eq!(pip["provenance"]["reviewed_at"], "2026-08-26");
    assert!(pip["provenance"]["preconditions"]
        .as_array()
        .is_some_and(|preconditions| !preconditions.is_empty()));

    let mut human = chystik();
    human.args([
        "scan",
        home.to_str().unwrap(),
        "--safe",
        "--no-tui",
        "--verbose",
        "--min-size",
        "0",
    ]);
    isolated(&mut human, &home);
    human.env("XDG_CACHE_HOME", home.join("cache"));
    let output = human.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("policy: direct_safe"));
    assert!(stdout.contains("rule: python.pip.cache"));
    assert!(stdout.contains("recovery:"));
    assert!(stdout.contains("source: https://pip.pypa.io/"));
    assert!(stdout.contains("reviewed: 2026-08-26"));
    assert!(stdout.contains("requires:"));

    let mut preview = chystik();
    preview.args([
        "clean",
        home.to_str().unwrap(),
        "--safe",
        "--dry-run",
        "--format",
        "json",
        "--min-size",
        "0",
    ]);
    isolated(&mut preview, &home);
    preview.env("XDG_CACHE_HOME", home.join("cache"));
    let output = preview.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(document["plan"]["eligible"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| {
            item["finding"]["provenance"]["rule_id"] == "python.pip.cache"
                && item["finding"]["provenance"]["policy"] == "direct_safe"
        }));
}

#[test]
fn human_scan_announces_that_work_started_before_printing_the_final_report() {
    let fixture = tempfile::tempdir().unwrap();
    let output = chystik()
        .args(["scan", fixture.path().to_str().unwrap(), "--safe"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Scanning"),
        "human scans must give immediate progress feedback instead of appearing idle"
    );
}

#[test]
fn help_exposes_examples_and_the_machine_output_contract_at_each_command_level() {
    let root = chystik().arg("--help").output().unwrap();
    let scan = chystik().args(["scan", "--help"]).output().unwrap();
    let report = chystik().args(["report", "--help"]).output().unwrap();
    let clean = chystik().args(["clean", "--help"]).output().unwrap();
    let explain = chystik().args(["explain", "--help"]).output().unwrap();
    let config = chystik().args(["config", "--help"]).output().unwrap();
    let completion = chystik().args(["completion", "--help"]).output().unwrap();

    for output in [
        &root,
        &scan,
        &report,
        &clean,
        &explain,
        &config,
        &completion,
    ] {
        assert!(output.status.success());
    }

    let root_help = String::from_utf8(root.stdout).unwrap();
    let scan_help = String::from_utf8(scan.stdout).unwrap();
    let report_help = String::from_utf8(report.stdout).unwrap();
    let clean_help = String::from_utf8(clean.stdout).unwrap();
    let explain_help = String::from_utf8(explain.stdout).unwrap();
    let config_help = String::from_utf8(config.stdout).unwrap();
    let completion_help = String::from_utf8(completion.stdout).unwrap();
    assert!(root_help.contains("EXAMPLES:"));
    assert!(root_help.contains("chystik scan --safe"));
    assert!(scan_help.contains("Interactive terminals open a live table"));
    assert!(scan_help.contains("--no-tui"));
    assert!(report_help.contains("Use --format jsonl for streaming progress"));
    assert!(clean_help.contains("Every cleanup moves items only to native Trash"));
    assert!(explain_help.contains("chystik explain ~/.cache/go-build"));
    assert!(config_help.contains("chystik config reset"));
    assert!(completion_help.contains("chystik completion fish"));
}

#[test]
fn scan_jsonl_streams_individually_valid_records_and_a_terminal_summary() {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path().join("project");
    let modules = project.join("node_modules/left-pad");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();
    std::fs::write(modules.join("index.js"), "x".repeat(2048)).unwrap();

    let output = chystik()
        .args([
            "scan",
            fixture.path().to_str().unwrap(),
            "--format",
            "jsonl",
            "--min-size",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(records.iter().all(|record| {
        record["schema_version"] == 1
            && record["chystik_version"] == env!("CARGO_PKG_VERSION")
            && record["platform"] == std::env::consts::OS
            && record["generated_at"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
    }));
    assert!(records.iter().any(|record| record["kind"] == "finding"));
    let summary = records.last().unwrap();
    assert_eq!(summary["kind"], "summary");
    assert_eq!(summary["schema_version"], 1);
    assert!(summary["summary"]["findings"].as_u64().unwrap() >= 1);
}

#[test]
fn report_json_has_its_own_stable_kind() {
    let fixture = tempfile::tempdir().unwrap();
    let output = chystik()
        .args([
            "report",
            fixture.path().to_str().unwrap(),
            "--format",
            "json",
            "--min-size",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["kind"], "report");
    assert_eq!(document["schema_version"], 1);
}

#[test]
fn config_show_is_a_versioned_machine_document() {
    let fixture = tempfile::tempdir().unwrap();
    let mut command = chystik();
    command.args(["config", "show"]);
    isolated(&mut command, fixture.path());
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["kind"], "config");
    assert_eq!(document["chystik_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(document["platform"], std::env::consts::OS);
    assert_eq!(document["config"]["schema_version"], 1);
}

#[test]
fn explain_prints_recovery_and_cleanup_policy_for_a_known_directory() {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path().join("project");
    let modules = project.join("node_modules");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(project.join("package.json"), "{}").unwrap();
    std::fs::write(project.join("package-lock.json"), "{}").unwrap();

    let output = chystik()
        .args(["explain", modules.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("build_artifacts"));
    assert!(text.contains("Recovery: Rebuild / redownload"));
    assert!(text.contains("Cleanup: Review required"));
    assert!(text.contains("moderate"));
    assert!(text.contains("cleanup_policy: direct_review"));
}

#[test]
fn clean_safe_dry_run_prints_a_manifest_and_never_modifies_the_fixture() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join(".cache/go-build");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("cache.bin"), "x".repeat(2048)).unwrap();
    let mut command = chystik();
    command.args([
        "clean",
        fixture.path().to_str().unwrap(),
        "--safe",
        "--dry-run",
        "--format",
        "json",
        "--min-size",
        "0",
    ]);
    isolated(&mut command, fixture.path());
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(manifest["kind"], "cleanup_preview");
    assert_eq!(manifest["plan"]["eligible"].as_array().unwrap().len(), 1);
    assert!(cache.exists(), "dry-run must never invoke native Trash");
}

#[test]
fn human_safe_cleanup_shows_scan_progress_before_its_preview() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join(".cache/go-build");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("cache.bin"), "x".repeat(2048)).unwrap();
    let output = chystik()
        .args([
            "clean",
            fixture.path().to_str().unwrap(),
            "--safe",
            "--dry-run",
            "--min-size",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Scanning"));
    assert!(cache.exists(), "preview must not move a fixture to Trash");
}

#[test]
fn clean_safe_yes_without_persisted_consent_is_policy_refused() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join(".cache/go-build");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("cache.bin"), "x".repeat(2048)).unwrap();
    let mut command = chystik();
    command.args([
        "clean",
        fixture.path().to_str().unwrap(),
        "--safe",
        "--yes",
        "--min-size",
        "0",
    ]);
    isolated(&mut command, fixture.path());
    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(cache.exists(), "--yes must not bypass first-run consent");
}

#[test]
fn clean_without_confirmation_on_a_pipe_is_cancelled_not_executed() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join(".cache/go-build");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("cache.bin"), "x".repeat(2048)).unwrap();
    let mut command = chystik();
    command.args([
        "clean",
        fixture.path().to_str().unwrap(),
        "--safe",
        "--min-size",
        "0",
    ]);
    isolated(&mut command, fixture.path());
    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(
        cache.exists(),
        "a non-interactive invocation must not clean"
    );
}

#[test]
fn invalid_clean_arguments_use_exit_code_two() {
    let fixture = tempfile::tempdir().unwrap();
    let output = chystik()
        .args(["clean", fixture.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[test]
#[ignore = "moves a disposable fixture through the host native Trash"]
fn clean_safe_yes_moves_only_eligible_paths_to_native_trash() {
    #[cfg(target_os = "windows")]
    let fixture_parent = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .expect("Windows must provide an absolute USERPROFILE");
    #[cfg(not(target_os = "windows"))]
    let fixture_parent = std::env::current_dir().expect("test must have a working directory");
    let fixture = tempfile::Builder::new()
        .prefix("chystik-cli-native-trash-")
        .tempdir_in(fixture_parent)
        .expect("create a disposable CLI cleanup fixture");
    let safe = fixture.path().join(".cache/go-build");
    let excluded = fixture.path().join("excluded/.cache/go-build");
    let risky = fixture.path().join(".ollama/models");
    for path in [&safe, &excluded, &risky] {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("fixture.bin"), "x".repeat(2048)).unwrap();
    }
    let reported_safe = std::fs::canonicalize(&safe).unwrap();
    #[cfg(unix)]
    let symlink_target = {
        let target = fixture.path().join("outside/.cache/go-build");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("fixture.bin"), "x".repeat(2048)).unwrap();
        std::os::unix::fs::symlink(fixture.path().join("outside"), fixture.path().join("link"))
            .unwrap();
        target
    };

    let config_path = isolated_config_path(fixture.path());
    let config_dir = config_path.parent().unwrap();
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(
        config_path,
        serde_json::json!({
            "schema_version": 1,
            "exclusions": [excluded],
            "acknowledged_version": env!("CARGO_PKG_VERSION"),
        })
        .to_string(),
    )
    .unwrap();

    let mut command = chystik();
    command.args([
        "clean",
        fixture.path().to_str().unwrap(),
        "--safe",
        "--yes",
        "--format",
        "json",
        "--min-size",
        "0",
        "--exclude",
        excluded.to_str().unwrap(),
    ]);
    isolated(&mut command, fixture.path());
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "native cleanup failed with {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["kind"], "cleanup");
    assert!(
        document["outcome"]["removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == reported_safe.to_string_lossy().as_ref()),
        "safe fixture was not reported as moved: {document}"
    );
    assert!(
        !safe.exists(),
        "eligible safe data must leave the source tree"
    );
    assert!(excluded.exists(), "an excluded path must survive --yes");
    assert!(
        risky.exists(),
        "risky data must survive a safe bulk cleanup"
    );
    #[cfg(unix)]
    assert!(
        symlink_target.exists(),
        "a symlinked parent must not send its target to Trash"
    );
}
