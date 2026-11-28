//! The front matter of a plan file `<type>-<slug>-plan.md`.

use schemars::JsonSchema;
use serde::Deserialize;

use super::ticket_common::{PlanStatus, Scalar};
use crate::finding::Finding;
use crate::parse::front_matter::front_matter;

/// The build's type of work (never a ticket type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PlanType {
    /// New behaviour.
    Feature,
    /// A defect fix.
    Bugfix,
    /// Restructuring.
    Refactor,
    /// Upkeep.
    Chore,
}

/// Front matter of a plan file.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanFront {
    /// The entry id of the leaf, or the file stem of a backlog leaf.
    pub ticket: Scalar,
    /// The leaf's title, as `tickets.py mark` writes it.
    pub title: Option<String>,
    /// The build's type.
    #[serde(rename = "type")]
    pub kind: Option<PlanType>,
    /// The leaf's status.
    pub status: Option<PlanStatus>,
    /// Who works on it.
    pub assignee: Option<String>,
    /// Date it was blocked.
    pub blocked_at: Option<String>,
    /// Why it was blocked.
    pub blocked_reason: Option<String>,
    /// The commit the work started from.
    pub baseline_revision: Option<String>,
}

impl PlanFront {
    /// Reads the front matter of the plan file at `path`; `None` when it has none.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Option<PlanFront>, Finding> {
        front_matter::<PlanFront>(path, text).map(|(front, _)| front)
    }
}
