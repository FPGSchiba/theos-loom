//! The typed events of a run.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::stop::StopCause;
use super::usage::Usage;

/// The id of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub Uuid);

impl RunId {
    /// A new random (v4) id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One of the two output streams of the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// One event of a run: a parsed stream line, or something the runtime itself observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum RunEvent {
    /// The image the container was started from.
    Image {
        /// The image reference or id.
        id: String,
    },
    /// The workflow kit in the worktree.
    Kit {
        /// The kit's repository.
        repository: String,
        /// The pinned version.
        version: String,
    },
    /// The CLI version the run used (T14).
    CliVersion {
        /// The version string.
        version: String,
    },
    /// Updating the CLI failed; the run continues with the installed one.
    CliUpdateFailed {
        /// What went wrong.
        message: String,
    },
    /// The CLI's `system`/`init` event.
    Init {
        /// The session id.
        session_id: String,
        /// The model.
        model: String,
        /// The CLI version, when the event carries it.
        cli_version: Option<String>,
    },
    /// An assistant message.
    Assistant {
        /// The message id.
        message_id: String,
        /// The model that produced it.
        model: String,
        /// Its token usage, when reported.
        usage: Option<Usage>,
        /// The event as the CLI wrote it.
        raw: Value,
    },
    /// A user message (tool results and the like).
    User {
        /// The event as the CLI wrote it.
        raw: Value,
    },
    /// Any other `system` event.
    System {
        /// The subtype.
        subtype: String,
        /// The event as the CLI wrote it.
        raw: Value,
    },
    /// Rate-limit information that is not a rejection.
    RateLimitStatus {
        /// The status, as the CLI names it.
        kind: String,
        /// When the window resets, as a Unix timestamp, when known.
        resets_at: Option<i64>,
    },
    /// The plan's rate limit was hit.
    RateLimited {
        /// Which limit, when known.
        kind: Option<String>,
        /// When it resets, as a Unix timestamp, when known.
        resets_at: Option<i64>,
    },
    /// The CLI's sign-in failed.
    AuthFailed {
        /// The CLI's message.
        message: String,
    },
    /// The final `result` event.
    Result {
        /// The subtype.
        subtype: String,
        /// Whether the CLI marked the result an error.
        is_error: bool,
        /// The cost the CLI reports, in US dollars.
        total_cost_usd: Option<f64>,
        /// The event as the CLI wrote it.
        raw: Value,
    },
    /// A destination the proxy saw.
    Egress {
        /// The host.
        host: String,
        /// The port, when known.
        port: Option<u16>,
        /// Whether the proxy allowed it.
        allowed: bool,
    },
    /// The computed spend differs from the one the CLI reports.
    SpendMismatch {
        /// Spend computed from usage and prices, in US dollars.
        computed_usd: f64,
        /// Spend the CLI reports, in US dollars.
        reported_usd: f64,
    },
    /// A line that is no known event, kept as text (R-e). A non-JSON line lands here too.
    Unknown {
        /// The line, as lossy UTF-8.
        raw: String,
    },
    /// The run ended.
    RunEnded {
        /// Why.
        cause: StopCause,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::stop::CapKind;
    use super::super::usage::TokenKind;
    use super::*;

    #[test]
    fn every_event_round_trips_through_json() {
        let usage = Usage {
            input: 1,
            output: 2,
            cache_write_5m: 3,
            cache_write_1h: 4,
            cache_read: 5,
        };
        let events = vec![
            RunEvent::Image { id: "img".into() },
            RunEvent::Kit {
                repository: "r".into(),
                version: "v1".into(),
            },
            RunEvent::CliVersion {
                version: "1.2.3".into(),
            },
            RunEvent::CliUpdateFailed {
                message: "m".into(),
            },
            RunEvent::Init {
                session_id: "s".into(),
                model: "m".into(),
                cli_version: Some("1".into()),
            },
            RunEvent::Assistant {
                message_id: "id".into(),
                model: "m".into(),
                usage: Some(usage),
                raw: json!({"a": 1}),
            },
            RunEvent::User {
                raw: json!({"b": [1, 2]}),
            },
            RunEvent::System {
                subtype: "x".into(),
                raw: json!(null),
            },
            RunEvent::RateLimitStatus {
                kind: "allowed".into(),
                resets_at: Some(10),
            },
            RunEvent::RateLimited {
                kind: Some("five_hour".into()),
                resets_at: None,
            },
            RunEvent::AuthFailed {
                message: "m".into(),
            },
            RunEvent::Result {
                subtype: "success".into(),
                is_error: false,
                total_cost_usd: Some(0.5),
                raw: json!({}),
            },
            RunEvent::Egress {
                host: "example.org".into(),
                port: Some(443),
                allowed: true,
            },
            RunEvent::SpendMismatch {
                computed_usd: 1.5,
                reported_usd: 2.5,
            },
            RunEvent::Unknown {
                raw: "not json".into(),
            },
            RunEvent::RunEnded {
                cause: StopCause::Cap(CapKind::Tokens(TokenKind::Output)),
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let back: RunEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event, "{json}");
        }
    }

    #[test]
    fn run_id_displays_as_a_uuid() {
        let id = RunId::new();
        assert_eq!(id.to_string(), id.0.to_string());
    }
}
