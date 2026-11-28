//! The front matter of a story, spike or bug file.

use schemars::JsonSchema;
use serde::Deserialize;

use super::ids::{AfterRef, Key, LoomMark, TicketId, yaml_after_list};
use super::ticket_common::{Origin, PlanStatus, Risk, Scalar, Severity, TrackerStatus};
use crate::finding::Finding;
use crate::parse::front_matter::front_matter;

/// The kind of a leaf: `type` of a leaf file or of a `tickets.toml` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LeafKind {
    /// A story.
    Story,
    /// A spike.
    Spike,
    /// A bug.
    Bug,
}

/// Front matter of a leaf file.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeafFront {
    /// Tracker id, written at publish.
    pub tracker_id: Option<String>,
    /// The entry id of `tickets.toml`, or the folder's `next_id`.
    pub id: Option<TicketId>,
    /// Story, spike or bug.
    #[serde(rename = "type")]
    pub kind: LeafKind,
    /// What works when done.
    pub title: String,
    /// Folder name of the epic or initiative; none for a backlog leaf.
    pub parent: Option<String>,
    /// Epic ids it serves.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Inputs the criteria were derived from.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files read beside this ticket.
    #[serde(default)]
    pub companions: Vec<String>,
    /// Prerequisites.
    #[serde(default, deserialize_with = "yaml_after_list")]
    pub after: Vec<AfterRef>,
    /// A tracker's assignee.
    pub assignee: Option<String>,
    /// True once the criteria are approved.
    pub refined: Option<bool>,
    /// True when a person does part of the work.
    pub hitl: Option<bool>,
    /// Risk.
    pub risk: Option<Risk>,
    /// Estimate, as the user gives it.
    pub estimate: Option<Scalar>,
    /// Bugs only.
    pub severity: Option<Severity>,
    /// Older files only; read when there is no plan.
    pub status: Option<PlanStatus>,
    /// Tracker url.
    pub remote: Option<String>,
    /// Board state from a tracker sync.
    pub tracker_status: Option<TrackerStatus>,
    /// (Loom) project-unique key.
    pub key: Option<Key>,
    /// (Loom) whether Loom may work on it.
    pub loom: Option<LoomMark>,
    /// (Loom) cross-component prerequisites.
    #[serde(default)]
    pub needs: Vec<Key>,
    /// (Loom) set when slicing wrote the leaf.
    pub origin: Option<Origin>,
}

impl LeafFront {
    /// Reads the front matter of the leaf file at `path`; `None` when it has none.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Option<LeafFront>, Finding> {
        front_matter::<LeafFront>(path, text).map(|(front, _)| front)
    }
}
