//! Severity helpers.

use crate::model::Severity;
use chrono::{DateTime, Utc};

/// Combined recommendation for UI sorting/labels.
#[derive(Debug, Clone, PartialEq)]
pub enum Recommendation {
    /// Green: remove now.
    RemoveNow,
    /// Yellow: safe but will need reinstall time.
    RemoveWithReinstall,
    /// Red: review manually first.
    ReviewFirst,
}

/// Derive recommendation from severity + how long since last use.
pub fn recommend(
    severity: Severity,
    _last_used: Option<DateTime<Utc>>,
    _now: DateTime<Utc>,
) -> Recommendation {
    // Age informs the UI (idle_label) but never overrides severity.
    match severity {
        Severity::Safe => Recommendation::RemoveNow,
        Severity::Moderate => Recommendation::RemoveWithReinstall,
        Severity::Risky => Recommendation::ReviewFirst,
    }
}

/// Human string like "unused for 3 months" or "used today".
pub fn idle_label(last_used: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(t) = last_used else {
        return "unknown".to_owned();
    };
    let secs = now.signed_duration_since(t).num_seconds().max(0);
    if secs < 24 * 3600 {
        return "today".to_owned();
    }
    let days = secs / 86_400;
    match days {
        0..=6 => unit(days, "day", "days"),
        7..=29 => unit(days / 7, "week", "weeks"),
        30..=364 => unit(days / 30, "month", "months"),
        _ => unit(days / 365, "year", "years"),
    }
}

fn unit(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Severity;
    use chrono::{Duration, TimeZone, Utc};

    fn ago(days: i64) -> Option<DateTime<Utc>> {
        Some(Utc::now() - Duration::days(days))
    }

    #[test]
    fn recommendation_follows_severity_only() {
        let now = Utc.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        assert_eq!(
            recommend(Severity::Safe, ago(400), now),
            Recommendation::RemoveNow
        );
        assert_eq!(
            recommend(Severity::Moderate, None, now),
            Recommendation::RemoveWithReinstall
        );
        assert_eq!(
            recommend(Severity::Risky, ago(1), now),
            Recommendation::ReviewFirst
        );
    }

    #[test]
    fn idle_labels() {
        let now = Utc.with_ymd_and_hms(2026, 8, 24, 12, 0, 0).unwrap();
        assert_eq!(idle_label(None, now), "unknown");
        assert_eq!(
            idle_label(now.checked_sub_signed(Duration::hours(3)), now),
            "today"
        );
        assert_eq!(
            idle_label(now.checked_sub_signed(Duration::days(5)), now),
            "5 days"
        );
        assert_eq!(
            idle_label(now.checked_sub_signed(Duration::days(14)), now),
            "2 weeks"
        );
        assert_eq!(
            idle_label(now.checked_sub_signed(Duration::days(90)), now),
            "3 months"
        );
        assert_eq!(
            idle_label(now.checked_sub_signed(Duration::days(800)), now),
            "2 years"
        );
    }
}
