//! Human-readable formatting for byte counts, speeds, percentages, ratios,
//! durations, and display-width-aware text truncation.
//!
//! These helpers are the single source of truth for number and text formatting
//! across the header, list rows, and detail pane.

use std::time::Duration;

use unicode_width::UnicodeWidthChar;

/// The character appended when text is cut: `…` (one column wide).
pub const ELLIPSIS: char = '\u{2026}';

/// Terminal display width of a string (wide characters count as two columns).
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Truncate `s` to at most `max` display columns, appending `…` if cut.
///
/// Returns the string unchanged when it already fits. `max == 0` yields an
/// empty string.
pub fn truncate_end(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > max - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push(ELLIPSIS);
    out
}

/// Truncate `s` in the middle to at most `max` display columns, keeping the
/// start and (favoring) the end, joined by `…`. Useful for file paths where
/// the filename is the discriminating part.
pub fn truncate_middle(s: &str, max: usize) -> String {
    if display_width(s) <= max {
        return s.to_string();
    }
    if max <= 1 {
        return if max == 0 {
            String::new()
        } else {
            ELLIPSIS.to_string()
        };
    }
    let budget = max - 1;
    let head_budget = budget / 2;
    let tail_budget = budget - head_budget;

    let mut head = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > head_budget {
            break;
        }
        head.push(c);
        used += w;
    }

    let mut tail_chars = Vec::new();
    let mut tail_used = 0;
    for c in s.chars().rev() {
        let w = c.width().unwrap_or(0);
        if tail_used + w > tail_budget {
            break;
        }
        tail_chars.push(c);
        tail_used += w;
    }
    let tail: String = tail_chars.into_iter().rev().collect();
    format!("{head}{ELLIPSIS}{tail}")
}

/// Format a duration compactly, e.g. `47s`, `4m12s`, `1h02m`, `2d03h`.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d{:02}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

/// Format a byte count with binary units, e.g. `1.4 GiB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Format a bytes-per-second rate, e.g. `1.4 MiB/s`.
pub fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_size(bytes_per_sec))
}

/// Format a fraction (clamped to `0.0..=1.0`) as a percentage, e.g. `42.1%`.
pub fn format_percent(frac: f64) -> String {
    format!("{:.1}%", frac.clamp(0.0, 1.0) * 100.0)
}

/// Format a share ratio with two decimals, e.g. `1.23`.
///
/// Returns `0.00` when nothing has been downloaded.
pub fn format_ratio(ratio: f64) -> String {
    format!("{:.2}", ratio.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1_500_000), "1.4 MiB");
        assert_eq!(format_size(1024u64 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.0 TiB");
    }

    #[test]
    fn speeds_suffix_per_second() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(1024 * 1024), "1.0 MiB/s");
    }

    #[test]
    fn percent_clamps_to_range() {
        assert_eq!(format_percent(0.0), "0.0%");
        assert_eq!(format_percent(0.421), "42.1%");
        assert_eq!(format_percent(1.0), "100.0%");
        assert_eq!(format_percent(1.5), "100.0%");
        assert_eq!(format_percent(-0.1), "0.0%");
    }

    #[test]
    fn ratio_never_negative() {
        assert_eq!(format_ratio(0.0), "0.00");
        assert_eq!(format_ratio(1.234), "1.23");
        assert_eq!(format_ratio(2.0), "2.00");
        assert_eq!(format_ratio(-1.0), "0.00");
    }

    #[test]
    fn durations_are_compact() {
        assert_eq!(format_duration(Duration::from_secs(47)), "47s");
        assert_eq!(format_duration(Duration::from_secs(252)), "4m12s");
        assert_eq!(format_duration(Duration::from_secs(3720)), "1h02m");
        assert_eq!(format_duration(Duration::from_secs(2 * 86_400 + 3 * 3600)), "2d03h");
    }

    #[test]
    fn truncate_end_respects_display_width() {
        assert_eq!(truncate_end("short", 10), "short");
        assert_eq!(truncate_end("abcdefgh", 5), "abcd…");
        assert_eq!(truncate_end("abc", 0), "");
        // Wide CJK chars occupy two columns each.
        assert_eq!(display_width("日本語"), 6);
        let cut = truncate_end("日本語テスト", 5);
        assert!(display_width(&cut) <= 5, "got {cut:?}");
        assert!(cut.ends_with(ELLIPSIS));
    }

    #[test]
    fn truncate_middle_keeps_tail() {
        assert_eq!(truncate_middle("short", 10), "short");
        let cut = truncate_middle("dir/subdir/filename.bin", 15);
        assert!(display_width(&cut) <= 15, "got {cut:?}");
        assert!(cut.ends_with(".bin"), "got {cut:?}");
        assert!(cut.contains(ELLIPSIS));
        // Wide chars never split into over-budget output.
        let wide = truncate_middle("日本語のとても長いファイル名.mkv", 12);
        assert!(display_width(&wide) <= 12, "got {wide:?}");
    }
}
