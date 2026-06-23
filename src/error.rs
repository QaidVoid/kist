#![allow(dead_code)] // consumed by engine/main in later task groups

//! Error handling helpers.

/// Convenience alias used throughout the application.
pub type Result<T> = anyhow::Result<T>;

/// Render an error as a short, human-readable status line.
///
/// Uses the error's top-level message (without the full cause chain) so it
/// fits on a single terminal line.
pub fn to_status_line(err: &anyhow::Error) -> String {
    let msg = err.to_string();
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        format!("{err:#}")
    } else {
        trimmed.to_string()
    }
}
