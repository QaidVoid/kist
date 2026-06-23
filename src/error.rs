//! Error handling helpers.

/// Convenience alias used throughout the application.
pub type Result<T> = anyhow::Result<T>;

/// Render an error as a short, human-readable status line.
///
/// Uses anyhow's alternate display so the full cause chain is shown (each
/// context separated by ": "), which is essential for diagnosing failures
/// instead of only seeing a generic wrapper like "failed to add torrent".
pub fn to_status_line(err: &anyhow::Error) -> String {
    format!("{err:#}")
}
