//! Human-readable formatting for byte counts and speeds.
//!
//! These helpers are the single source of truth for number formatting across
//! the header, list rows, and detail pane. Percentage and ratio formatters
//! live alongside once the detail pane consumes them.

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
}
