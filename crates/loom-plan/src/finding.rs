//! Findings: what `check` and the reader report about a planning repo.

use std::fmt;

use serde::{Serialize, Serializer};

/// How bad a finding is. Errors order before notes, so a sorted list shows errors first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Makes `check` fail.
    Error,
    /// Worth saying, never fails `check`.
    Note,
}

/// The rule that produced a finding. Serialized and displayed in kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// A typed file that does not parse.
    Parse,
    /// A key the model does not know.
    UnknownKey,
    /// A path or file shape the tree does not allow.
    Layout,
    /// A story file and its `tickets.toml` entry disagree.
    Drift,
    /// A reference naming something that does not exist.
    BrokenReference,
    /// A cycle in the dependency graph.
    Cycle,
    /// Two things with one id.
    DuplicateId,
    /// A component directive that contradicts a project directive.
    DirectiveContradiction,
    /// A directive without a bold name.
    UnnamedDirective,
    /// An epic without goal, scope or requirement line.
    EpicIncomplete,
    /// A story, spike or bug without numbered criteria.
    StoryWithoutAcs,
    /// An epic or story without a `key`.
    MissingKey,
}

impl Rule {
    /// Every rule, in declaration order.
    pub const ALL: [Rule; 12] = [
        Rule::Parse,
        Rule::UnknownKey,
        Rule::Layout,
        Rule::Drift,
        Rule::BrokenReference,
        Rule::Cycle,
        Rule::DuplicateId,
        Rule::DirectiveContradiction,
        Rule::UnnamedDirective,
        Rule::EpicIncomplete,
        Rule::StoryWithoutAcs,
        Rule::MissingKey,
    ];

    /// The kebab-case name; the one place the names are written.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Rule::Parse => "parse",
            Rule::UnknownKey => "unknown-key",
            Rule::Layout => "layout",
            Rule::Drift => "drift",
            Rule::BrokenReference => "broken-reference",
            Rule::Cycle => "cycle",
            Rule::DuplicateId => "duplicate-id",
            Rule::DirectiveContradiction => "directive-contradiction",
            Rule::UnnamedDirective => "unnamed-directive",
            Rule::EpicIncomplete => "epic-incomplete",
            Rule::StoryWithoutAcs => "story-without-acs",
            Rule::MissingKey => "missing-key",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Serialize for Rule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// One thing `check` or the reader found in a planning repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// How bad it is.
    pub severity: Severity,
    /// The rule that found it.
    pub rule: Rule,
    /// Path relative to the repo root, with `/` separators.
    pub path: String,
    /// The line, when known.
    pub line: Option<u32>,
    /// What is wrong and what to change, in plain words.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn rule_names_are_kebab_case() -> TestResult {
        assert_eq!(Rule::UnknownKey.to_string(), "unknown-key");
        assert_eq!(Rule::StoryWithoutAcs.to_string(), "story-without-acs");
        for rule in Rule::ALL {
            assert_eq!(serde_json::to_string(&rule)?, format!("\"{rule}\""));
        }
        Ok(())
    }

    #[test]
    fn errors_sort_before_notes() {
        assert!(Severity::Error < Severity::Note);
    }
}
