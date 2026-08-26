//! Contributor-facing catalog schema plus validation shared by build-time and
//! runtime loading. This module has no filesystem or cleanup side effects.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use serde::Deserialize;

pub const ENVIRONMENT_OVERRIDES: &[&str] = &[
    "PIP_CACHE_DIR",
    "CCACHE_DIR",
    "SCCACHE_DIR",
    "VCPKG_DEFAULT_BINARY_CACHE",
    "OPTIX_CACHE_PATH",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogFile {
    #[serde(default)]
    pub rule: Vec<RawRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRule {
    pub id: String,
    pub category: String,
    pub recovery: String,
    /// Human-readable recovery consequence retained in the public provenance
    /// contract. The controlled `recovery` value above is the policy class.
    pub recovery_note: String,
    pub cleanup_policy: String,
    pub note: String,
    pub source_url: String,
    pub reviewed_at: String,
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub advice: Option<String>,
    #[serde(default)]
    pub locator: Vec<RawLocator>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLocator {
    pub platform: String,
    pub root: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default = "exact_matcher")]
    pub matcher: String,
    #[serde(default)]
    pub required_files: Vec<String>,
    #[serde(default)]
    pub required_dirs: Vec<String>,
}

fn exact_matcher() -> String {
    "exact".into()
}

pub fn parse_catalog(source_name: &str, source: &str) -> Result<Vec<RawRule>, String> {
    let parsed: CatalogFile =
        toml::from_str(source).map_err(|error| format!("{source_name}: invalid TOML: {error}"))?;
    if parsed.rule.is_empty() {
        return Err(format!("{source_name}: catalog file declares no rules"));
    }
    Ok(parsed.rule)
}

pub fn validate_catalog(rules: &[RawRule]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut locators = BTreeSet::new();
    for rule in rules {
        validate_rule(rule)?;
        if !ids.insert(&rule.id) {
            return Err(format!("duplicate catalog rule id: {}", rule.id));
        }
        for locator in &rule.locator {
            let key = format!(
                "{}|{}|{}|{}|{}|{}",
                rule.id,
                locator.platform,
                locator.root,
                locator.path.as_deref().unwrap_or_default(),
                locator.environment.as_deref().unwrap_or_default(),
                locator.matcher,
            );
            if !locators.insert(key) {
                return Err(format!("{}: duplicate locator", rule.id));
            }
        }
    }
    Ok(())
}

fn validate_rule(rule: &RawRule) -> Result<(), String> {
    if rule.id.trim().is_empty()
        || !rule
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(format!("invalid catalog rule id: {:?}", rule.id));
    }
    if !matches!(
        rule.category.as_str(),
        "build-artifacts"
            | "package-caches"
            | "ide-toolchains"
            | "ai-models"
            | "browser-system"
            | "android-dev"
            | "ai-agents"
            | "containers"
            | "installers"
            | "game-launchers"
            | "media-apps"
            | "messengers"
            | "cloud-sync"
            | "office-docs"
            | "system-junk"
    ) {
        return Err(format!("{}: unknown category {:?}", rule.id, rule.category));
    }
    if !matches!(
        rule.recovery.as_str(),
        "automatic" | "rebuild-redownload" | "manual-irreplaceable"
    ) {
        return Err(format!("{}: unknown recovery {:?}", rule.id, rule.recovery));
    }
    if !matches!(
        rule.cleanup_policy.as_str(),
        "auto-cleanable" | "review-required" | "tool-managed" | "advisory-only"
    ) {
        return Err(format!(
            "{}: unknown cleanup_policy {:?}",
            rule.id, rule.cleanup_policy
        ));
    }
    if rule.cleanup_policy == "auto-cleanable" && rule.recovery != "automatic" {
        return Err(format!(
            "{}: auto-cleanable rules must have automatic recovery",
            rule.id
        ));
    }
    if rule.note.trim().is_empty() {
        return Err(format!("{}: note must not be empty", rule.id));
    }
    if rule.recovery_note.trim().is_empty() {
        return Err(format!("{}: recovery_note must not be empty", rule.id));
    }
    if rule
        .advice
        .as_deref()
        .is_some_and(|advice| advice.trim().is_empty())
    {
        return Err(format!(
            "{}: advice must not be empty when present",
            rule.id
        ));
    }
    if !rule.source_url.starts_with("https://") {
        return Err(format!("{}: source_url must use HTTPS", rule.id));
    }
    NaiveDate::parse_from_str(&rule.reviewed_at, "%Y-%m-%d")
        .map_err(|_| format!("{}: invalid reviewed_at date", rule.id))?;
    if rule.preconditions.is_empty()
        || rule
            .preconditions
            .iter()
            .any(|condition| condition.trim().is_empty())
    {
        return Err(format!("{}: preconditions must be non-empty", rule.id));
    }
    if rule.locator.is_empty() {
        return Err(format!("{}: rule needs at least one locator", rule.id));
    }
    for locator in &rule.locator {
        validate_locator(rule, locator)?;
    }
    Ok(())
}

fn validate_locator(rule: &RawRule, locator: &RawLocator) -> Result<(), String> {
    if !matches!(
        locator.platform.as_str(),
        "linux" | "macos" | "windows" | "all"
    ) {
        return Err(format!(
            "{}: unknown platform {:?}",
            rule.id, locator.platform
        ));
    }
    if !matches!(
        locator.root.as_str(),
        "home"
            | "cache"
            | "local-app-data"
            | "library-caches"
            | "developer"
            | "volume-root"
            | "environment"
    ) {
        return Err(format!("{}: unsupported root {:?}", rule.id, locator.root));
    }
    let path = locator.path.as_deref().unwrap_or_default();
    if locator.root == "environment" {
        let environment = locator.environment.as_deref().unwrap_or_default();
        if !ENVIRONMENT_OVERRIDES.contains(&environment) {
            return Err(format!("{}: unsupported environment override", rule.id));
        }
        if !path.is_empty() {
            return Err(format!(
                "{}: environment locators must name the exact override root",
                rule.id
            ));
        }
    } else if locator.environment.is_some() {
        return Err(format!(
            "{}: environment is valid only with root = environment",
            rule.id
        ));
    }
    if locator.root != "environment" && path.is_empty() {
        return Err(format!("{}: broad root-only locator is forbidden", rule.id));
    }
    if !path.is_empty() {
        validate_relative_path(rule, path)?;
    }
    if !matches!(
        locator.matcher.as_str(),
        "exact" | "descendant-with-markers" | "direct-child-with-markers"
    ) {
        return Err(format!(
            "{}: unsupported matcher {:?}",
            rule.id, locator.matcher
        ));
    }
    let has_markers = !locator.required_files.is_empty() || !locator.required_dirs.is_empty();
    if locator.matcher == "exact" && has_markers {
        return Err(format!("{}: exact locator cannot declare markers", rule.id));
    }
    if locator.matcher != "exact" && !has_markers {
        return Err(format!("{}: marker locator needs a marker", rule.id));
    }
    for marker in locator.required_files.iter().chain(&locator.required_dirs) {
        if marker.is_empty() || marker.contains(['/', '\\']) || marker == "." || marker == ".." {
            return Err(format!("{}: invalid marker {:?}", rule.id, marker));
        }
    }
    Ok(())
}

fn validate_relative_path(rule: &RawRule, path: &str) -> Result<(), String> {
    if path.starts_with('/') || path.contains(':') {
        return Err(format!("{}: locator path must be relative", rule.id));
    }
    if path.contains('\\') {
        return Err(format!(
            "{}: locator path must use forward slashes",
            rule.id
        ));
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("{}: locator path has unsafe traversal", rule.id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> RawRule {
        RawRule {
            id: "example.cache".into(),
            category: "package-caches".into(),
            recovery: "automatic".into(),
            recovery_note: "the next run restores the cache".into(),
            cleanup_policy: "auto-cleanable".into(),
            note: "example cache — restored by the next run".into(),
            source_url: "https://example.com/docs".into(),
            reviewed_at: "2026-08-26".into(),
            preconditions: vec!["exact documented cache root".into()],
            advice: None,
            locator: vec![RawLocator {
                platform: "linux".into(),
                root: "cache".into(),
                path: Some("example".into()),
                environment: None,
                matcher: "exact".into(),
                required_files: vec![],
                required_dirs: vec![],
            }],
        }
    }

    #[test]
    fn strict_validation_rejects_broad_or_unsafe_rules() {
        let mut invalid = rule();
        invalid.locator[0].path = Some("../important".into());
        assert!(validate_catalog(&[invalid]).is_err());

        let mut invalid = rule();
        invalid.recovery = "manual-irreplaceable".into();
        assert!(validate_catalog(&[invalid]).is_err());

        let mut invalid = rule();
        invalid.source_url = "http://example.com".into();
        assert!(validate_catalog(&[invalid]).is_err());

        let mut invalid = rule();
        invalid.locator[0].path = Some("pip\\..\\important".into());
        assert!(validate_catalog(&[invalid]).is_err());

        let mut invalid = rule();
        invalid.locator[0].root = "environment".into();
        invalid.locator[0].environment = Some("PIP_CACHE_DIR".into());
        invalid.locator[0].path = Some("child".into());
        assert!(validate_catalog(&[invalid]).is_err());
    }
}
