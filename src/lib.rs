//! Safe, explicit game-controller input capture primitives.
//!
//! The public API is intentionally not established yet.

/// Returns the crate's current development status.
pub const fn status() -> &'static str {
    "pre-1.0"
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_pre_release_status() {
        assert_eq!(super::status(), "pre-1.0");
    }
}
