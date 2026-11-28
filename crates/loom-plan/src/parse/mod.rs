//! Reading front matter and TOML files into typed values; errors become [`Finding`]s.

pub(crate) mod body;
pub(crate) mod directives;
pub(crate) mod front_matter;
pub(crate) mod items;
pub(crate) mod toml_file;

use crate::finding::{Finding, Rule, Severity};

/// Builds the `unknown-key` finding: names the file, the key and the keys the model knows.
pub(crate) fn unknown_key(path: &str, key: &str, expected: &[&str], line: Option<u32>) -> Finding {
    let mut message = format!("unknown key `{key}` in {path}");
    if !expected.is_empty() {
        let list: Vec<String> = expected.iter().map(|k| format!("`{k}`")).collect();
        message.push_str("; expected one of ");
        message.push_str(&list.join(", "));
    }
    Finding {
        severity: Severity::Error,
        rule: Rule::UnknownKey,
        path: path.to_owned(),
        line,
        message,
    }
}

/// Builds the `parse` finding.
pub(crate) fn parse_error(path: &str, detail: &str, line: Option<u32>) -> Finding {
    Finding {
        severity: Severity::Error,
        rule: Rule::Parse,
        path: path.to_owned(),
        line,
        message: format!("cannot read {path}: {detail}"),
    }
}

/// 1-based line of a byte offset in `text`.
pub(crate) fn line_at(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    let count = text.get(..end).map_or(0, |head| head.matches('\n').count());
    u32::try_from(count + 1).unwrap_or(u32::MAX)
}
