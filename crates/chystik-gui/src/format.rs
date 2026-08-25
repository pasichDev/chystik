//! Human-readable formatting. Pure functions, unit-tested below.

use chrono::{DateTime, Utc};

use chystik_core::platform::StorageVolume;

use crate::i18n::{self, Strings};

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Age label from `last_used`: `"today"`, `"12 d"`, `"3 mo"`, `"2 y"`, `"unknown"`.
pub(crate) fn age_label(
    last_used: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    s: &Strings,
) -> String {
    let Some(t) = last_used else {
        return s.age_unknown.to_string();
    };
    let days = (now - t).num_days();
    if days <= 0 {
        s.age_today.to_string()
    } else if days < 30 {
        i18n::fill(s.age_days.as_str(), &[("n", &days.to_string())])
    } else if days < 365 {
        i18n::fill(s.age_months.as_str(), &[("n", &(days / 30).to_string())])
    } else {
        i18n::fill(s.age_years.as_str(), &[("n", &(days / 365).to_string())])
    }
}

/// Severity colours chosen so the three stay distinguishable without
/// colour vision.
///
/// The previous green `#43B581` / yellow `#CCA700` pair sat at the same
/// relative luminance (1.11:1 against each other) AND both on the red-green
/// axis, so deuteranopes and greyscale displays saw one colour. Safe is now
/// blue-shifted teal, which keeps a blue channel the amber does not have,
/// and Risky drops a clear luminance step below both.
///
/// Colour is never the only cue: see `severity_glyph` (shape) and
/// `severity_pill` (fill weight).
pub(crate) fn capacity_summary(disks: &[StorageVolume]) -> String {
    let total: u64 = disks.iter().map(|d| d.total_bytes).sum();
    let free: u64 = disks.iter().map(|d| d.free_bytes).sum();
    format!(
        "\u{3a3} {} \u{b7} {} free",
        format_size(total),
        format_size(free)
    )
}

/// `used / total` usage pair for one volume chip.
pub(crate) fn disk_usage_label(d: &StorageVolume) -> String {
    format!(
        "{} / {}",
        format_size(d.total_bytes.saturating_sub(d.free_bytes)),
        format_size(d.total_bytes)
    )
}

/// Middle-truncate a string with an ellipsis, keeping head and tail visible.
pub(crate) fn truncate_middle(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    let half = max_chars.saturating_sub(1) / 2;
    let head: String = chars[..half].iter().collect();
    let tail: String = chars[chars.len() - half..].iter().collect();
    format!("{head}\u{2026}{tail}")
}

/// Split an absolute path into a dimmable directory prefix and the final
/// component. The last component is what identifies a finding, so it gets
/// full contrast while the prefix recedes — the one trick that makes a
/// column of long paths scannable.
///
/// `$HOME` collapses to `~` first: on a real machine most paths share it,
/// and spelling it out on every row is noise.
pub(crate) fn split_path_tail(full: &str) -> (String, String) {
    let home = chystik_core::platform::current().app_paths().home_dir;
    let shortened = full
        .strip_prefix(&home.to_string_lossy().into_owned())
        .map(|tail| format!("~{tail}"))
        .unwrap_or_else(|| full.to_string());
    match shortened.rfind('/') {
        Some(i) => (shortened[..=i].to_string(), shortened[i + 1..].to_string()),
        None => (String::new(), shortened),
    }
}

/// Last `depth` path components joined with `/` — compact chip labels.
pub(crate) fn path_tail(path: &std::path::Path, depth: usize) -> String {
    let mut parts: Vec<String> = path
        .components()
        .rev()
        .take(depth)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.reverse();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use std::path::PathBuf;

    const GB: u64 = 1024 * 1024 * 1024;

    /// Minimal volume for the capacity helpers.
    fn disk(mount: &str, total: u64, free: u64) -> StorageVolume {
        StorageVolume {
            source: String::new(),
            mount_point: PathBuf::from(mount),
            fs_type: "ext4".to_string(),
            total_bytes: total,
            free_bytes: free,
        }
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(format_size(2_u64.pow(40)), "1.0 TB");
    }

    #[test]
    fn age_label_cases() {
        let s = i18n::strings(Lang::En);
        let now = Utc::now();
        assert_eq!(age_label(None, now, s), "unknown");
        assert_eq!(age_label(Some(now), now, s), "today");
        let twelve_days = now - chrono::Duration::days(12);
        assert_eq!(age_label(Some(twelve_days), now, s), "12 d");
        let three_months = now - chrono::Duration::days(95);
        assert_eq!(age_label(Some(three_months), now, s), "3 mo");
        let two_years = now - chrono::Duration::days(760);
        assert_eq!(age_label(Some(two_years), now, s), "2 y");
    }

    #[test]
    fn age_label_follows_the_active_language() {
        let now = Utc::now();
        let uk = i18n::strings(Lang::Uk);
        assert_eq!(age_label(None, now, uk), "невідомо");
        assert_eq!(age_label(Some(now), now, uk), "сьогодні");
        assert_eq!(
            age_label(Some(now - chrono::Duration::days(12)), now, uk),
            "12 дн"
        );
    }

    #[test]
    fn truncate_middle_short_and_long() {
        assert_eq!(truncate_middle("/a/b", 10), "/a/b");
        let t = truncate_middle("/home/user/very/long/path/node_modules", 20);
        assert!(t.chars().count() <= 20);
        assert!(t.contains('\u{2026}'));
    }

    #[test]
    fn path_tail_joins_last_components() {
        assert_eq!(
            path_tail(std::path::Path::new("/home/u/.ollama/models"), 2),
            ".ollama/models"
        );
        assert_eq!(path_tail(std::path::Path::new("models"), 2), "models");
    }

    #[test]
    fn capacity_summary_totals_every_shown_disk() {
        let disks = vec![disk("/", 500, 100), disk("/media/ext", 300, 300)];
        assert_eq!(capacity_summary(&disks), "\u{3a3} 800 B \u{b7} 400 B free");
        assert_eq!(capacity_summary(&[]), "\u{3a3} 0 B \u{b7} 0 B free");
    }

    #[test]
    fn disk_usage_label_shows_used_over_total() {
        let d = disk("/mnt/data", 3 * GB, GB);
        assert_eq!(disk_usage_label(&d), "2.0 GB / 3.0 GB");
        let tiny = disk("/scratch", 512, 512);
        assert_eq!(disk_usage_label(&tiny), "0 B / 512 B");
    }
}
