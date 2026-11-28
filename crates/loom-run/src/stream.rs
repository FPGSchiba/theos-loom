//! Parsing the CLI's `stream-json` output: one stdout line becomes one [`RunEvent`].

use loom_core::run::{RunEvent, Usage};
use serde_json::Value;

/// Phrases of a result's text that mean the CLI is not signed in (lower case).
const AUTH_PHRASES: [&str; 4] = [
    "invalid api key",
    "/login",
    "authentication",
    "authenticate",
];

/// Parse one line of the CLI's stdout into a typed event. Never fails: a line that is not a
/// known JSON event is kept as [`RunEvent::Unknown`] (spec R-e).
///
/// A `rate_limit_event` whose status starts with `allowed` (`allowed`, `allowed_warning`) is
/// only [`RunEvent::RateLimitStatus`] and never stops a run; any other status is
/// [`RunEvent::RateLimited`].
#[must_use]
pub fn parse_line(line: &[u8]) -> RunEvent {
    let parsed = serde_json::from_slice::<Value>(line).ok().and_then(typed);
    parsed.unwrap_or_else(|| {
        let raw = String::from_utf8_lossy(line).into_owned();
        tracing::debug!(%raw, "stream line is no known event");
        RunEvent::Unknown { raw }
    })
}

/// The typed event of a parsed line, or `None` when its type is not known.
fn typed(raw: Value) -> Option<RunEvent> {
    let kind = raw.get("type")?.as_str()?;
    match kind {
        "system" => Some(system(raw)),
        "assistant" => Some(assistant(&raw)),
        "user" => Some(RunEvent::User { raw }),
        "rate_limit_event" => Some(rate_limit(&raw)),
        "result" => Some(result(raw)),
        _ => None,
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn system(raw: Value) -> RunEvent {
    let subtype = text(&raw, "subtype");
    if subtype == "init" {
        RunEvent::Init {
            session_id: text(&raw, "session_id"),
            model: text(&raw, "model"),
            cli_version: raw
                .get("claude_code_version")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    } else {
        RunEvent::System { subtype, raw }
    }
}

fn assistant(raw: &Value) -> RunEvent {
    let message = raw.get("message").unwrap_or(&Value::Null);
    RunEvent::Assistant {
        message_id: text(message, "id"),
        model: text(message, "model"),
        usage: message.get("usage").map(usage),
        raw: raw.clone(),
    }
}

fn count(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

/// Token counts of a `usage` object. The cache-write split is used when the CLI reports it,
/// else all cache writes count as 5-minute ones.
fn usage(value: &Value) -> Usage {
    let split = value.get("cache_creation").filter(|c| c.is_object());
    let (short_lived, long_lived) = match split {
        Some(c) => (
            count(c, "ephemeral_5m_input_tokens").unwrap_or(0),
            count(c, "ephemeral_1h_input_tokens").unwrap_or(0),
        ),
        None => (count(value, "cache_creation_input_tokens").unwrap_or(0), 0),
    };
    Usage {
        input: count(value, "input_tokens").unwrap_or(0),
        output: count(value, "output_tokens").unwrap_or(0),
        cache_write_5m: short_lived,
        cache_write_1h: long_lived,
        cache_read: count(value, "cache_read_input_tokens").unwrap_or(0),
    }
}

fn rate_limit(raw: &Value) -> RunEvent {
    let info = raw.get("rate_limit_info").unwrap_or(&Value::Null);
    let status = text(info, "status");
    let resets_at = info.get("resetsAt").and_then(Value::as_i64);
    if status.starts_with("allowed") {
        RunEvent::RateLimitStatus {
            kind: status,
            resets_at,
        }
    } else {
        RunEvent::RateLimited {
            kind: info
                .get("rateLimitType")
                .and_then(Value::as_str)
                .map(str::to_owned),
            resets_at,
        }
    }
}

fn result(raw: Value) -> RunEvent {
    let is_error = raw
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = text(&raw, "result");
    match raw.get("api_error_status").and_then(Value::as_i64) {
        Some(429) => {
            return RunEvent::RateLimited {
                kind: None,
                resets_at: None,
            };
        }
        Some(401 | 403) => return RunEvent::AuthFailed { message },
        _ => {}
    }
    let lower = message.to_lowercase();
    if is_error && AUTH_PHRASES.iter().any(|p| lower.contains(p)) {
        return RunEvent::AuthFailed { message };
    }
    RunEvent::Result {
        subtype: text(&raw, "subtype"),
        is_error,
        total_cost_usd: raw.get("total_cost_usd").and_then(Value::as_f64),
        raw,
    }
}

#[cfg(test)]
mod tests {
    use loom_core::run::Usage;

    use super::*;

    const NORMAL: &[u8] = include_bytes!("../tests/fixtures/streams/normal_run.jsonl");
    const AUTH: &[u8] = include_bytes!("../tests/fixtures/streams/auth_failure.jsonl");
    const REJECTED: &[u8] = include_bytes!("../tests/fixtures/streams/rate_limit_rejected.jsonl");
    const R429: &[u8] = include_bytes!("../tests/fixtures/streams/result_429.jsonl");

    fn lines(data: &[u8]) -> Vec<&[u8]> {
        data.split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .collect()
    }

    // AC: TU-03.2
    #[test]
    fn every_line_of_a_real_run_becomes_a_typed_event() {
        let events: Vec<RunEvent> = lines(NORMAL).into_iter().map(parse_line).collect();
        assert_eq!(events.len(), 4);
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, RunEvent::Unknown { .. }))
        );
        assert!(matches!(
            &events[0],
            RunEvent::Init { session_id, model, cli_version }
                if session_id == "00000000-0000-4000-8000-000000000001"
                    && model == "claude-haiku-5-5"
                    && cli_version.as_deref() == Some("2.1.322")
        ));
        let RunEvent::Assistant {
            message_id,
            model,
            usage,
            ..
        } = &events[1]
        else {
            panic!("second line is not an assistant event: {:?}", events[1]);
        };
        assert_eq!(message_id, "msg_FAKE000000000000000001");
        assert_eq!(model, "claude-haiku-5-5");
        assert_eq!(
            *usage,
            Some(Usage {
                input: 2,
                output: 1,
                cache_write_5m: 0,
                cache_write_1h: 21522,
                cache_read: 17316,
            })
        );
        assert!(matches!(
            &events[2],
            RunEvent::RateLimitStatus { kind, resets_at: Some(1_795_891_800) } if kind == "allowed"
        ));
        assert!(matches!(
            &events[3],
            RunEvent::Result { subtype, is_error: false, total_cost_usd: Some(_), .. }
                if subtype == "success"
        ));
    }

    // AC: TU-03.2
    #[test]
    fn a_line_that_is_not_json_is_kept_as_unknown() {
        assert_eq!(
            parse_line(b"Warning: x"),
            RunEvent::Unknown {
                raw: "Warning: x".into()
            }
        );
    }

    #[test]
    fn an_unknown_event_type_is_kept_as_unknown() {
        let line = br#"{"type":"brand_new"}"#;
        assert_eq!(
            parse_line(line),
            RunEvent::Unknown {
                raw: r#"{"type":"brand_new"}"#.into()
            }
        );
    }

    #[test]
    fn other_system_and_user_events_are_typed() {
        let sys = parse_line(br#"{"type":"system","subtype":"api_retry","attempt":1}"#);
        assert!(matches!(&sys, RunEvent::System { subtype, .. } if subtype == "api_retry"));
        let user = parse_line(br#"{"type":"user","message":{"role":"user"}}"#);
        assert!(matches!(user, RunEvent::User { .. }));
    }

    // AC: TU-03.6
    #[test]
    fn a_rejected_rate_limit_is_rate_limited() {
        let event = parse_line(lines(REJECTED)[0]);
        assert_eq!(
            event,
            RunEvent::RateLimited {
                kind: Some("five_hour".into()),
                resets_at: Some(1_795_891_800),
            }
        );
    }

    // AC: TU-03.6
    #[test]
    fn an_allowed_rate_limit_is_only_status() {
        let event = parse_line(lines(NORMAL)[2]);
        assert!(matches!(event, RunEvent::RateLimitStatus { .. }));
    }

    // AC: TU-03.6
    #[test]
    fn an_allowed_warning_is_only_status() {
        let line = br#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":5,"rateLimitType":"five_hour"}}"#;
        assert_eq!(
            parse_line(line),
            RunEvent::RateLimitStatus {
                kind: "allowed_warning".into(),
                resets_at: Some(5)
            }
        );
    }

    // AC: TU-03.6
    #[test]
    fn a_429_result_is_rate_limited() {
        let event = parse_line(lines(R429)[0]);
        assert!(matches!(event, RunEvent::RateLimited { .. }), "{event:?}");
    }

    // AC: TU-03.6
    #[test]
    fn an_auth_failure_is_auth_failed_not_rate_limited() {
        let events: Vec<RunEvent> = lines(AUTH).into_iter().map(parse_line).collect();
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, RunEvent::RateLimited { .. }))
        );
        let failed: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RunEvent::AuthFailed { .. }))
            .collect();
        assert_eq!(failed.len(), 1, "{events:?}");
        assert!(matches!(
            failed[0],
            RunEvent::AuthFailed { message } if message.contains("401")
        ));
    }

    // AC: TU-03.6
    #[test]
    fn a_not_signed_in_result_is_auth_failed() {
        for text in [
            "Invalid API key · Please run /login",
            "Authentication required",
        ] {
            let line = format!(
                r#"{{"type":"result","subtype":"success","is_error":true,"result":"{text}"}}"#
            );
            assert!(
                matches!(parse_line(line.as_bytes()), RunEvent::AuthFailed { .. }),
                "{text}"
            );
        }
        let ok = br#"{"type":"result","subtype":"success","is_error":true,"result":"boom"}"#;
        assert!(matches!(
            parse_line(ok),
            RunEvent::Result { is_error: true, .. }
        ));
    }

    #[test]
    fn cache_writes_split_into_5m_and_1h() {
        let split = br#"{"type":"assistant","message":{"id":"m","model":"x","usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":30,"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":20}}}}"#;
        let total = br#"{"type":"assistant","message":{"id":"m","model":"x","usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":30}}}"#;
        let usage = |line: &[u8]| match parse_line(line) {
            RunEvent::Assistant { usage, .. } => usage,
            other => panic!("not an assistant event: {other:?}"),
        };
        assert_eq!(
            usage(split),
            Some(Usage {
                input: 1,
                output: 2,
                cache_write_5m: 10,
                cache_write_1h: 20,
                cache_read: 3
            })
        );
        assert_eq!(
            usage(total),
            Some(Usage {
                input: 1,
                output: 2,
                cache_write_5m: 30,
                cache_write_1h: 0,
                cache_read: 3
            })
        );
    }
}
