//! Why a run ended.

use serde::{Deserialize, Serialize};

use super::usage::TokenKind;

/// The cap that stopped a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapKind {
    /// The cap of one token kind.
    Tokens(TokenKind),
    /// The spend cap.
    Spend,
    /// The wall-time cap.
    WallTime,
}

/// The cause a run ended with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopCause {
    /// The CLI finished successfully.
    Completed,
    /// The CLI failed.
    Failed {
        /// The exit code, when the process had one.
        exit_code: Option<i32>,
    },
    /// The run was cancelled.
    Cancelled,
    /// A cap stopped the run.
    Cap(CapKind),
    /// A spend cap was set but the model has no price.
    NoPrice {
        /// The model without a price.
        model: String,
    },
    /// The plan's rate limit was hit.
    RateLimited {
        /// When the limit resets, as a Unix timestamp, when known.
        resets_at: Option<i64>,
    },
    /// The CLI's sign-in failed.
    AuthFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_cause_names_its_cap() {
        let json =
            serde_json::to_string(&StopCause::Cap(CapKind::Tokens(TokenKind::CacheRead))).unwrap();
        assert!(json.contains("\"CacheRead\""), "{json}");
    }
}
