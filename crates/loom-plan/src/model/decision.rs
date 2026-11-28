//! Planning-repo decision records: `decisions/NNNN-<slug>.md` with `kind` and `date` front matter.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::finding::{Finding, Rule, Severity};
use crate::parse::front_matter::front_matter;

/// What a decision record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionKind {
    /// A decision of the owner.
    Decision,
    /// An answer recorded for a question.
    Answer,
}

/// The front matter of a decision record.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionFront {
    /// Decision or answer.
    pub kind: DecisionKind,
    /// `YYYY-MM-DD`.
    #[serde(deserialize_with = "iso_date")]
    #[schemars(regex(pattern = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"))]
    pub date: String,
}

/// A decision record: its numbered file name and its front matter.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    /// The `NNNN` of the file name.
    pub number: u32,
    /// The `<slug>` of the file name.
    pub slug: String,
    /// Repo-relative path.
    pub path: String,
    /// The front matter.
    pub front: DecisionFront,
}

fn iso_date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let s = String::deserialize(deserializer)?;
    if is_iso_date(&s) {
        Ok(s)
    } else {
        Err(serde::de::Error::custom(format!(
            "date `{s}` is not a YYYY-MM-DD date"
        )))
    }
}

/// True for `YYYY-MM-DD` with month 01-12 and day 01-31.
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let month = (b[5] - b'0') * 10 + (b[6] - b'0');
    let day = (b[8] - b'0') * 10 + (b[9] - b'0');
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// Splits a file name `NNNN-<slug>.md` into number and slug.
///
/// The number is four digits; the slug is lowercase letters, digits and single hyphens.
fn split_file_name(name: &str) -> Option<(u32, &str)> {
    let stem = name.strip_suffix(".md")?;
    let (number, slug) = stem.split_once('-')?;
    let slug_ok = !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if number.len() == 4 && number.bytes().all(|b| b.is_ascii_digit()) && slug_ok {
        Some((number.parse().ok()?, slug))
    } else {
        None
    }
}

fn error(path: &str, message: String) -> Finding {
    Finding {
        severity: Severity::Error,
        rule: Rule::Parse,
        path: path.to_owned(),
        line: None,
        message,
    }
}

impl DecisionRecord {
    /// Parses the decision record at `path` from its Markdown `text`.
    ///
    /// # Errors
    /// A `parse` finding for a bad file name or missing front matter, or the front matter's own
    /// `parse` / `unknown-key` finding.
    pub fn parse(path: &str, text: &str) -> Result<DecisionRecord, Finding> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let (number, slug) = split_file_name(name).ok_or_else(|| {
            error(
                path,
                format!("decision file name `{name}` must be NNNN-<slug>.md"),
            )
        })?;
        let (front, _) = front_matter::<DecisionFront>(path, text)?;
        let front = front.ok_or_else(|| {
            error(
                path,
                format!("{path} has no front matter with `kind` and `date`"),
            )
        })?;
        Ok(DecisionRecord {
            number,
            slug: slug.to_owned(),
            path: path.to_owned(),
            front,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_split_or_refused() {
        assert_eq!(split_file_name("0007-use-rust.md"), Some((7, "use-rust")));
        for bad in [
            "7-use-rust.md",
            "0007-Use.md",
            "0007-.md",
            "0007-a--b.md",
            "0007-a.txt",
            "0007.md",
        ] {
            assert_eq!(split_file_name(bad), None, "{bad}");
        }
    }
}
