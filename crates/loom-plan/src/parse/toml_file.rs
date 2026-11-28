//! Deserializing a TOML file into a typed value.

use serde::de::DeserializeOwned;

use super::{line_at, parse_error, unknown_key};
use crate::finding::Finding;

/// Parses `text` (the file at `path`) as TOML into `T`.
///
/// `toml::de::Error` has no structured access to an unknown field, so serde's standard
/// ``unknown field `x`, expected one of `a`, `b` `` message is matched; every other error is a
/// `parse` finding. The line comes from the error's span.
pub(crate) fn toml_file<T: DeserializeOwned>(path: &str, text: &str) -> Result<T, Finding> {
    toml::from_str(text).map_err(|err| {
        let line = err.span().map(|s| line_at(text, s.start));
        let message = err.message();
        match parse_unknown_field(message) {
            Some((key, expected)) => unknown_key(path, key, &expected, line),
            None => parse_error(path, message, line),
        }
    })
}

/// Splits serde's ``unknown field `x`, expected one of `a`, `b` `` (or ``expected `a` ``).
fn parse_unknown_field(message: &str) -> Option<(&str, Vec<&str>)> {
    let rest = message.strip_prefix("unknown field `")?;
    let (key, rest) = rest.split_once('`')?;
    let expected = rest.split('`').skip(1).step_by(2).collect();
    Some((key, expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Rule;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct T {
        a: u32,
    }

    fn err(text: &str) -> Result<Finding, Box<dyn std::error::Error>> {
        toml_file::<T>("x/y.toml", text)
            .err()
            .ok_or_else(|| "expected an error".into())
    }

    // AC: TU-01.1
    #[test]
    fn unknown_toml_key_names_file_and_key() -> TestResult {
        let f = err("a = 1\nzz = 2")?;
        assert_eq!(f.rule, Rule::UnknownKey);
        assert!(
            f.message.contains("zz") && f.message.contains("x/y.toml"),
            "{}",
            f.message
        );
        assert!(f.message.contains("`a`"), "{}", f.message);
        assert_eq!(f.line, Some(2));
        Ok(())
    }

    #[test]
    fn valid_toml_parses_and_bad_toml_is_a_parse_finding() -> TestResult {
        let t: T = toml_file("x.toml", "a = 1").map_err(|f| f.message)?;
        assert_eq!(t.a, 1);
        assert_eq!(err("a = [")?.rule, Rule::Parse);
        assert_eq!(err("")?.rule, Rule::Parse);
        Ok(())
    }
}
