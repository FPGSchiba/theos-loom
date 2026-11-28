//! The front matter of `epic-<slug>.md`.

use schemars::JsonSchema;
use serde::Deserialize;

use super::ids::{AfterRef, Key, LoomMark};
use super::ticket_common::{ContainerStatus, Risk, Scalar, TrackerStatus};
use crate::finding::Finding;
use crate::parse::front_matter::front_matter;

/// The only `type` an epic file carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EpicType {
    /// `type: epic`.
    Epic,
}

/// Front matter of an epic file.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpicFront {
    /// Tracker id, written at publish.
    pub tracker_id: Option<String>,
    /// Always `epic`.
    #[serde(rename = "type")]
    pub kind: EpicType,
    /// The outcome.
    pub title: String,
    /// Folder name of the initiative.
    pub parent: Option<String>,
    /// Parent requirement ids it owns.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Inputs the requirements were derived from.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files read beside this ticket.
    #[serde(default)]
    pub companions: Vec<String>,
    /// Epics it waits on.
    #[serde(default)]
    pub after: Vec<AfterRef>,
    /// `in-progress`, `done` or `dropped`.
    pub status: Option<ContainerStatus>,
    /// False while the epic is thin.
    pub refined: Option<bool>,
    /// Best guess, then its children's highest.
    pub risk: Option<Risk>,
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
}

impl EpicFront {
    /// Reads the front matter of the epic file at `path`; `None` when it has none.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Option<EpicFront>, Finding> {
        front_matter::<EpicFront>(path, text).map(|(front, _)| front)
    }
}
