//! The words the export bar uses, and the callback that feeds it.
//!
//! Altium and KiCad both go through these, so the two targets report a run the
//! same way: what is starting, what is being fetched, and how each component
//! ended. Keeping them here rather than at the call sites is what stops the two
//! drifting into different vocabularies for the same steps.

/// Called after each component with (done, total, message).
pub type ExportProgressCallback = std::sync::Arc<dyn Fn(usize, usize, &str) + Send + Sync>;

/// A run that has been handed its components but has not fetched one yet.
pub fn starting(count: usize) -> String {
    format!("Starting {count} components")
}

/// The component being fetched right now.
pub fn fetching(id: &str) -> String {
    format!("Fetching {id}")
}

/// A component that was written. The name is left off when there is not one,
/// which is what a failure looks like too.
pub fn exported(id: &str, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => shorten(format!("Exported {id} {name}")),
        None => format!("Exported {id}"),
    }
}

/// Keeps a line short enough for the bar, where a long part name would push the
/// numbers out of sight.
fn shorten(text: String) -> String {
    if text.chars().count() > 52 {
        format!("{}...", text.chars().take(49).collect::<String>())
    } else {
        text
    }
}

/// A component that could not be written.
pub fn failed(id: &str) -> String {
    format!("Failed {id}")
}

/// A component the run did not have to fetch, because an earlier run had.
pub fn skipped(id: &str) -> String {
    format!("Skipped {id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_reads_the_same_way() {
        assert_eq!(starting(11), "Starting 11 components");
        assert_eq!(fetching("C2040"), "Fetching C2040");
        assert_eq!(exported("C2040", Some("RP2040")), "Exported C2040 RP2040");
        assert_eq!(exported("C2040", None), "Exported C2040");
        assert_eq!(exported("C2040", Some("  ")), "Exported C2040");
        assert_eq!(failed("C2040"), "Failed C2040");
        assert_eq!(skipped("C2040"), "Skipped C2040");
    }
}
