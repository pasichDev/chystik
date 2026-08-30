//! UI localisation.
//!
//! Translations live in `locales/<lang>.json`, NOT in this file. Adding a
//! language means adding a JSON file and one enum variant; changing wording
//! never touches Rust. Translators work in a plain data file and cannot
//! break the build.
//!
//! The files are embedded with `include_str!` and parsed once on first use,
//! so there is nothing to install or ship alongside the binary. `serde`'s
//! `deny_unknown_fields` plus a required-field struct means a locale that
//! is missing a key, or carries a stale one, fails loudly — and the tests
//! below parse every locale, so that failure surfaces in CI rather than on
//! a user's screen.
//!
//! Scope boundary: this covers the interface. Per-finding `note` text is
//! authored by the rule modules in `chystik-core` and is still English —
//! translating it belongs with the rules, not here.

use std::collections::HashMap;
use std::sync::OnceLock;

use chystik_core::model::{Category, FindingPolicy, Severity};
use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Uk,
    De,
    Es,
    Fr,
    It,
    Pl,
    PtBr,
    Ro,
    Tr,
}

impl Lang {
    pub const ALL: [Lang; 10] = [
        Lang::En,
        Lang::Uk,
        Lang::De,
        Lang::Es,
        Lang::Fr,
        Lang::It,
        Lang::Pl,
        Lang::PtBr,
        Lang::Ro,
        Lang::Tr,
    ];

    /// Short label for the language switcher.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Uk => "UA",
            Lang::De => "DE",
            Lang::Es => "ES",
            Lang::Fr => "FR",
            Lang::It => "IT",
            Lang::Pl => "PL",
            Lang::PtBr => "PT-BR",
            Lang::Ro => "RO",
            Lang::Tr => "TR",
        }
    }

    /// Endonym — a language is always listed in its own language.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Uk => "Українська",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::Pl => "Polski",
            Lang::PtBr => "Português (Brasil)",
            Lang::Ro => "Română",
            Lang::Tr => "Türkçe",
        }
    }

    /// Raw contents of this language's locale file.
    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../locales/en.json"),
            Lang::Uk => include_str!("../locales/uk.json"),
            Lang::De => include_str!("../locales/de.json"),
            Lang::Es => include_str!("../locales/es.json"),
            Lang::Fr => include_str!("../locales/fr.json"),
            Lang::It => include_str!("../locales/it.json"),
            Lang::Pl => include_str!("../locales/pl.json"),
            Lang::PtBr => include_str!("../locales/pt_br.json"),
            Lang::Ro => include_str!("../locales/ro.json"),
            Lang::Tr => include_str!("../locales/tr.json"),
        }
    }
}

/// Pick a language from the POSIX locale environment.
///
/// Checked in the order `LC_ALL` → `LC_MESSAGES` → `LANG`, which is what
/// gettext does; anything with no matching locale file falls back to English.
pub fn detect() -> Lang {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let Some(value) = std::env::var_os(key) else {
            continue;
        };
        let value = value.to_string_lossy().to_lowercase();
        if value.is_empty() || value == "c" || value == "posix" {
            continue;
        }
        return match value.split(['_', '.', '-']).next().unwrap_or_default() {
            "uk" => Lang::Uk,
            "de" => Lang::De,
            "es" => Lang::Es,
            "fr" => Lang::Fr,
            "it" => Lang::It,
            "pl" => Lang::Pl,
            "pt" => Lang::PtBr,
            "ro" => Lang::Ro,
            "tr" => Lang::Tr,
            _ => Lang::En,
        };
    }
    Lang::En
}

/// One parsed locale file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locale {
    pub ui: Strings,
    categories: HashMap<Category, CategoryText>,
    severities: HashMap<Severity, SeverityText>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryText {
    label: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeverityText {
    label: String,
    cost: String,
}

/// Every piece of interface chrome. Field names are the JSON keys; a locale
/// file missing one fails to parse rather than rendering an empty label.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strings {
    pub app_name: String,
    // command bar
    pub scan: String,
    pub scan_hint: String,
    pub cancel: String,
    pub cancel_hint: String,
    pub export: String,
    pub export_hint: String,
    pub refresh_disks: String,
    pub refresh_disks_hint: String,
    pub targets_hint: String,
    pub add_folder: String,
    pub add_folder_hint: String,
    pub filter_hint: String,
    pub filter_tooltip: String,
    pub no_disks: String,
    // scan status
    pub scanning_dirs: String,
    pub found_so_far: String,
    pub press_scan: String,
    pub cancelling: String,
    pub scanning_target: String,
    pub reclaimable_summary: String,
    pub cancelled_summary: String,
    pub scanner_died: String,
    pub scan_failed: String,
    // sidebar
    pub reclaimable: String,
    pub items_in_categories: String,
    pub all_categories: String,
    pub all_categories_hint: String,
    pub nothing_found: String,
    pub filter_all: String,
    pub filter_all_hint: String,
    pub filter_safe_hint: String,
    pub filter_review: String,
    pub filter_review_hint: String,
    pub filter_risky_hint: String,
    pub items_word: String,
    pub item_word: String,
    // detail
    pub everything_found: String,
    pub everything_found_sub: String,
    pub select_auto_cleanable: String,
    pub select_auto_cleanable_hint: String,
    pub clear_selection: String,
    pub clear_selection_hint: String,
    pub col_path: String,
    pub col_size: String,
    pub col_recovery: String,
    pub col_age: String,
    pub sort_hint: String,
    pub risky_locked_hint: String,
    pub empty_title: String,
    pub empty_body: String,
    pub empty_filtered_title: String,
    pub empty_filtered_body: String,
    pub scanning_title: String,
    pub scanning_body: String,
    // footer
    pub totals_found: String,
    pub totals_auto_cleanable: String,
    pub totals_review_required: String,
    pub totals_manual_valuable: String,
    pub selected_word: String,
    pub select_all: String,
    pub select_all_hint: String,
    pub clear: String,
    pub clear_hint: String,
    pub move_to_trash: String,
    pub move_to_trash_idle: String,
    pub move_to_trash_hint: String,
    pub cleanup_unavailable: String,
    // confirm modal
    pub confirm_title: String,
    pub confirm_sub: String,
    pub guard_will_skip: String,
    pub esc_to_cancel: String,
    pub ok: String,
    // age
    pub age_today: String,
    pub age_days: String,
    pub age_months: String,
    pub age_years: String,
    pub age_unknown: String,
    // first-run risk acknowledgement
    pub consent_title: String,
    pub consent_lead: String,
    pub consent_p1_title: String,
    pub consent_p1_body: String,
    pub consent_p2_title: String,
    pub consent_p2_body: String,
    pub consent_p3_title: String,
    pub consent_p3_body: String,
    pub consent_p4_title: String,
    pub consent_p4_body: String,
    pub consent_p5_title: String,
    pub consent_p5_body: String,
    pub consent_checkbox: String,
    pub consent_continue: String,
    pub consent_quit: String,
    // settings dialog
    pub settings: String,
    pub settings_hint: String,
    pub settings_language: String,
    pub settings_version: String,
    pub settings_developer: String,
    pub settings_source: String,
    pub settings_open_link: String,
    pub close: String,
    // exclusions and advisories
    pub exclusions_title: String,
    pub exclusions_hint: String,
    pub exclusions_add: String,
    pub exclusions_remove: String,
    pub exclusions_empty: String,
    pub exclusions_unreadable: String,
    pub advice_run: String,
    pub advice_copy: String,
    pub advice_copied: String,
    pub policy_direct_safe: String,
    pub policy_direct_review: String,
    pub policy_advisory_only: String,
    pub policy_vendor_command_only: String,
    pub policy_never_clean: String,
    pub recovery_heading: String,
    pub cleanup_heading: String,
    pub evidence_rule: String,
    pub evidence_recovery: String,
    pub evidence_source: String,
    pub evidence_reviewed: String,
    pub evidence_preconditions: String,
    // sections, disks and privacy
    pub section_cleanup: String,
    pub section_disks: String,
    pub section_privacy: String,
    pub section_switch_hint: String,
    pub palette_title: String,
    pub disks_attached: String,
    pub disks_in_use: String,
    pub disks_idle_banner: String,
    pub disks_idle_explain: String,
    pub disks_not_mounted: String,
    pub disks_swap: String,
    pub disks_used: String,
    pub disks_none: String,
    pub privacy_title: String,
    pub privacy_lead: String,
    pub privacy_reveals: String,
    pub privacy_cost: String,
    pub privacy_clear: String,
    pub privacy_clear_idle: String,
    pub privacy_nothing_preselected: String,
    pub privacy_none: String,
    pub privacy_selected: String,
    pub privacy_confirm_title: String,
    pub privacy_confirm_lead: String,
    pub privacy_confirm_trash: String,
    pub privacy_confirm_erase: String,
    pub privacy_confirm_risky: String,
    // results
    pub trash_done_title: String,
    pub trash_done_recovery: String,
    pub trash_moved: String,
    pub trash_skipped: String,
    pub export_done: String,
    pub export_failed: String,
}

/// Parse and cache a locale. The files are compiled in, so a parse that
/// succeeds once succeeds always — `load_all` in the tests proves both
/// files parse before anything ships.
fn locale(lang: Lang) -> &'static Locale {
    static CACHE: OnceLock<HashMap<&'static str, Locale>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        Lang::ALL
            .into_iter()
            .map(|l| {
                let parsed = serde_json::from_str(l.source()).unwrap_or_else(|e| {
                    panic!("locales/{}.json is malformed: {e}", l.code().to_lowercase())
                });
                (l.code(), parsed)
            })
            .collect()
    });
    &cache[lang.code()]
}

pub fn strings(lang: Lang) -> &'static Strings {
    &locale(lang).ui
}

/// Category name in the active language.
pub fn category_label(lang: Lang, c: Category) -> &'static str {
    locale(lang)
        .categories
        .get(&c)
        .map(|t| t.label.as_str())
        .unwrap_or_else(|| c.label())
}

/// One sentence on what a category holds and what losing it costs. Shown as
/// a tooltip on the sidebar row and as the subtitle of the detail pane.
pub fn category_description(lang: Lang, c: Category) -> &'static str {
    locale(lang)
        .categories
        .get(&c)
        .map(|t| t.description.as_str())
        .unwrap_or("")
}

pub fn severity_label(lang: Lang, s: Severity) -> &'static str {
    locale(lang)
        .severities
        .get(&s)
        .map(|t| t.label.as_str())
        .unwrap_or_else(|| s.label())
}

/// What deleting this costs, in words. Surfaces `Severity::regeneration_cost`,
/// which the interface never called.
pub fn severity_cost(lang: Lang, s: Severity) -> &'static str {
    locale(lang)
        .severities
        .get(&s)
        .map(|t| t.cost.as_str())
        .unwrap_or("")
}

/// What Chystik is authorised to do with this precise finding.
pub fn policy_label(lang: Lang, policy: FindingPolicy) -> &'static str {
    let strings = strings(lang);
    match policy {
        FindingPolicy::DirectSafe => strings.policy_direct_safe.as_str(),
        FindingPolicy::DirectReview => strings.policy_direct_review.as_str(),
        FindingPolicy::AdvisoryOnly => strings.policy_advisory_only.as_str(),
        FindingPolicy::VendorCommandOnly => strings.policy_vendor_command_only.as_str(),
        FindingPolicy::NeverClean => strings.policy_never_clean.as_str(),
    }
}

/// `{n}` / `{size}` / `{c}` / `{items}` substitution. Deliberately tiny: a
/// full ICU formatter would be more machinery than eighty strings deserve.
pub fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in pairs {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_substitutes_every_placeholder() {
        assert_eq!(
            fill("{n} items · {size}", &[("n", "3"), ("size", "1.2 GB")]),
            "3 items · 1.2 GB"
        );
        // An unknown placeholder is left alone rather than silently blanked.
        assert_eq!(fill("{nope}", &[("n", "1")]), "{nope}");
    }

    /// The load-bearing test: every shipped locale file parses. Because the
    /// files are embedded, passing here means the runtime parse cannot fail.
    #[test]
    fn every_locale_file_parses_completely() {
        for lang in Lang::ALL {
            let parsed: Result<Locale, _> = serde_json::from_str(lang.source());
            let loc = parsed
                .unwrap_or_else(|e| panic!("locales/{}.json failed to parse: {e}", lang.code()));
            for c in Category::all() {
                assert!(loc.categories.contains_key(&c), "{lang:?} missing {c:?}");
            }
            for s in [Severity::Safe, Severity::Moderate, Severity::Risky] {
                assert!(loc.severities.contains_key(&s), "{lang:?} missing {s:?}");
            }
        }
    }

    #[test]
    fn no_string_is_blank_and_descriptions_say_something() {
        for lang in Lang::ALL {
            assert!(!strings(lang).app_name.trim().is_empty());
            assert!(!strings(lang).move_to_trash.trim().is_empty());
            for c in Category::all() {
                assert!(!category_label(lang, c).trim().is_empty(), "{c:?} label");
                let d = category_description(lang, c);
                assert!(
                    d.len() > 40,
                    "{lang:?} {c:?} description is too thin: {d:?}"
                );
            }
        }
    }

    #[test]
    fn every_policy_has_a_localized_explanation() {
        for lang in Lang::ALL {
            for policy in [
                FindingPolicy::DirectSafe,
                FindingPolicy::DirectReview,
                FindingPolicy::AdvisoryOnly,
                FindingPolicy::VendorCommandOnly,
                FindingPolicy::NeverClean,
            ] {
                assert!(
                    !policy_label(lang, policy).trim().is_empty(),
                    "{lang:?} missing {policy:?} policy label"
                );
            }
        }
    }

    #[test]
    fn placeholders_survive_translation() {
        // A translator dropping `{size}` would silently produce a button
        // that never shows the number.
        for lang in Lang::ALL {
            let s = strings(lang);
            assert!(s.move_to_trash.contains("{size}"), "{lang:?} move_to_trash");
            assert!(
                s.select_auto_cleanable.contains("{n}")
                    && s.select_auto_cleanable.contains("{size}")
            );
            assert!(s.items_in_categories.contains("{n}"));
            assert!(s.items_in_categories.contains("{c}"));
            assert!(s.confirm_sub.contains("{size}"));
            for age in [&s.age_days, &s.age_months, &s.age_years] {
                assert!(age.contains("{n}"), "{lang:?} age format lost {{n}}");
            }
        }
    }

    #[test]
    fn recovery_and_cleanup_labels_are_unambiguous_in_english_and_ukrainian() {
        assert_eq!(severity_label(Lang::En, Severity::Safe), "Automatic");
        assert_eq!(
            severity_label(Lang::En, Severity::Moderate),
            "Rebuild / redownload"
        );
        assert_eq!(
            severity_label(Lang::Uk, Severity::Risky),
            "Ручне / невідновлюване"
        );
        assert_eq!(
            policy_label(Lang::En, FindingPolicy::DirectSafe),
            "Auto-cleanable"
        );
        assert_eq!(
            policy_label(Lang::Uk, FindingPolicy::DirectReview),
            "Потрібна перевірка"
        );
    }

    #[test]
    fn ukrainian_is_actually_translated() {
        // Guards against a copy-paste that leaves a language in English.
        for c in Category::all() {
            assert_ne!(
                category_label(Lang::En, c),
                category_label(Lang::Uk, c),
                "{c:?} was never translated"
            );
        }
        assert_ne!(strings(Lang::En).scan, strings(Lang::Uk).scan);
    }

    #[test]
    fn every_non_english_locale_translates_the_safety_critical_chrome() {
        let english = strings(Lang::En);
        for lang in Lang::ALL.into_iter().filter(|lang| *lang != Lang::En) {
            let translated = strings(lang);
            for (name, actual, source) in [
                ("scan", &translated.scan, &english.scan),
                (
                    "move_to_trash",
                    &translated.move_to_trash,
                    &english.move_to_trash,
                ),
                (
                    "recovery_heading",
                    &translated.recovery_heading,
                    &english.recovery_heading,
                ),
                (
                    "cleanup_heading",
                    &translated.cleanup_heading,
                    &english.cleanup_heading,
                ),
                (
                    "policy_direct_safe",
                    &translated.policy_direct_safe,
                    &english.policy_direct_safe,
                ),
                (
                    "policy_direct_review",
                    &translated.policy_direct_review,
                    &english.policy_direct_review,
                ),
            ] {
                assert_ne!(actual, source, "{lang:?} left {name} in English");
            }
        }
    }
}
