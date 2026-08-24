//! AI-agent tooling rule set (v0.2) — working data of CLI coding agents:
//! Claude Code, Codex CLI, Gemini CLI, Cursor agent, and aider.
//!
//! All rules are `$HOME`-relative (matched via `strip_prefix(home_root())`,
//! same idiom as `core.rs::home_rule`). Nothing here targets `~/.config/**`
//! because `crate::guard::PROTECTED_NAMES` refuses any candidate containing
//! a `.config` component at deletion time; every path below lives in an
//! app-owned dot-dir or cache location the guard accepts.
//!
//! Severity rationale: agent session transcripts/history are irreplaceable
//! user state (Moderate), while shell-environment snapshots, chat temp
//! files, and tool caches regenerate during normal use (Safe).

use std::path::Path;

use crate::model::{Category, Severity};

use super::{home_root, Match};

/// Evaluate the AI-agent rule set against `dir`.
pub(crate) fn classify(dir: &Path) -> Option<Match> {
    let home = home_root()?;
    let rel = dir
        .strip_prefix(&home)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");

    const MOD: Severity = Severity::Moderate;
    const SAFE: Severity = Severity::Safe;
    const AGENT: Category = Category::AiAgents;
    let (sev, note) = match rel.as_str() {
        // Claude Code (Anthropic): per-project JSONL transcripts plus
        // shell-environment snapshots taken at session start.
        ".claude/projects" => (
            MOD,
            "Claude Code session transcripts — deleting permanently loses conversation history"
                .into(),
        ),
        ".claude/shell-snapshots" => (
            SAFE,
            "Claude Code shell snapshots — recreated when the next session starts".into(),
        ),
        // Codex CLI (OpenAI): rollout session logs and shell snapshots.
        ".codex/sessions" => (
            MOD,
            "Codex CLI session rollouts — deleting permanently loses conversation history".into(),
        ),
        ".codex/shell_snapshots" => (
            SAFE,
            "Codex shell snapshots — recreated on the next session".into(),
        ),
        // Gemini CLI: scratch space for an in-flight chat turn.
        ".gemini/tmp" => (
            SAFE,
            "Gemini CLI temporary chat files — recreated during normal use".into(),
        ),
        // Cursor agent keeps per-project state keyed by slugified project
        // path under ~/.cursor/projects.
        ".cursor/projects" => (
            MOD,
            "Cursor agent per-project history — deleting loses past agent session logs".into(),
        ),
        // aider: repo-map/ctags cache, rebuilt from the repository on the
        // next run.
        ".aider/caches" => (
            SAFE,
            "aider repo-map cache — rebuilt automatically on the next aider run".into(),
        ),
        // Downloaded agent runtimes are handled by GROUP rules in
        // `rules::GROUP_RULES`, which spare the newest build and report the
        // superseded ones individually. Claiming the parent here would
        // prune the walk before the group rule ever sees the children.
        ".opencode/bin" => (
            MOD,
            "downloaded opencode runtime binaries — re-downloaded on the next launch".into(),
        ),
        // Agent scratch space, named as such by the tools themselves.
        ".codex/.tmp" => (
            SAFE,
            "Codex CLI scratch space — plugin and marketplace checkouts, refetched".into(),
        ),
        ".gemini/antigravity-backup" => (
            MOD,
            "superseded Gemini agent state backup — kept only for rollback".into(),
        ),
        ".cache/opencode" => (
            SAFE,
            "opencode package cache — refetched on the next launch".into(),
        ),
        _ => return None,
    };
    Some(Match {
        category: AGENT,
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

    /// Marker telling a re-exec'd test binary that it runs in child mode.
    const CHILD_FLAG: &str = "CHYSTIK_AI_AGENTS_CHILD";

    /// Positive and negative expectations for every `$HOME`-relative rule.
    /// Runs either in a child process with an isolated CHYSTIK_TEST_HOME,
    /// or directly when that marker is already set.
    fn run_rule_expectations(home: &std::path::Path) {
        let projects = mk(home, ".claude/projects");
        let snapshots = mk(home, ".claude/shell-snapshots");
        let codex_sessions = mk(home, ".codex/sessions");
        let codex_snapshots = mk(home, ".codex/shell_snapshots");
        let gemini_tmp = mk(home, ".gemini/tmp");
        let cursor_projects = mk(home, ".cursor/projects");
        let aider_caches = mk(home, ".aider/caches");

        let m = classify(&projects).expect(".claude/projects should match");
        assert_eq!(m.category, Category::AiAgents);
        assert_eq!(m.severity, Severity::Moderate);
        assert_eq!(
            classify(&snapshots)
                .expect("shell-snapshots should match")
                .severity,
            Severity::Safe
        );
        assert_eq!(
            classify(&codex_sessions)
                .expect(".codex/sessions should match")
                .severity,
            Severity::Moderate
        );
        assert_eq!(
            classify(&codex_snapshots)
                .expect(".codex/shell_snapshots should match")
                .severity,
            Severity::Safe
        );
        assert_eq!(
            classify(&gemini_tmp)
                .expect(".gemini/tmp should match")
                .severity,
            Severity::Safe
        );
        assert_eq!(
            classify(&cursor_projects)
                .expect(".cursor/projects should match")
                .severity,
            Severity::Moderate
        );
        let m = classify(&aider_caches).expect(".aider/caches should match");
        assert_eq!(m.category, Category::AiAgents);
        assert_eq!(m.severity, Severity::Safe);

        // Negative cases: wrong names inside the app roots, an unrelated
        // relative path, and a correctly-named tree outside HOME.
        for bogus in [
            ".claude/projects-backup",
            ".claude/unknown",
            ".codex/logs",
            ".gemini/state",
            ".cursor/extensions",
            ".aider/statsig",
            "src/main",
        ] {
            assert!(
                classify(&mk(home, bogus)).is_none(),
                "{bogus} must not match"
            );
        }
    }

    #[test]
    fn ai_agent_home_rules_match_and_reject() {
        // CHYSTIK_TEST_HOME is process-global and sibling rule-set modules'
        // tests read/write it under their own locks, so mutating it here
        // would be racy. Instead re-exec this test binary as a child whose
        // environment carries an isolated override; this process's env is
        // never touched.
        if std::env::var_os(CHILD_FLAG).is_some() {
            let home = std::env::var_os("CHYSTIK_TEST_HOME")
                .expect("child mode requires CHYSTIK_TEST_HOME");
            run_rule_expectations(std::path::Path::new(&home));
            return;
        }
        let fake = tempdir().unwrap();
        let exe = std::env::current_exe().expect("test binary path");
        let out = std::process::Command::new(exe)
            .args(["ai_agent_home_rules_child", "--exact"])
            .env(CHILD_FLAG, "1")
            .env("CHYSTIK_TEST_HOME", fake.path())
            .output()
            .expect("spawn child test binary");
        assert!(
            out.status.success(),
            "child run failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    /// Entry point executed only in the re-exec'd child (see the parent
    /// test above); inert in a normal `cargo test` run.
    #[test]
    fn ai_agent_home_rules_child() {
        if std::env::var_os(CHILD_FLAG).is_none() {
            return;
        }
        let home =
            std::env::var_os("CHYSTIK_TEST_HOME").expect("child mode requires CHYSTIK_TEST_HOME");
        run_rule_expectations(std::path::Path::new(&home));
    }

    /// Without any override a tempdir is not under the real HOME, so even a
    /// correctly-named tree must return None. This variant needs no env
    /// manipulation and runs in-process.
    #[test]
    fn correctly_named_tree_outside_home_does_not_match() {
        let root = tempdir().unwrap();
        assert!(classify(&mk(root.path(), ".codex/sessions")).is_none());
    }
}
