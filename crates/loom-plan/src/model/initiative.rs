//! The front matter of `initiative-<slug>.md`.

use schemars::JsonSchema;
use serde::Deserialize;

use super::ticket_common::{ContainerStatus, Risk, Scalar, TrackerStatus};
use crate::finding::Finding;
use crate::parse::front_matter::front_matter;

/// The only `type` an initiative file carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InitiativeType {
    /// `type: initiative`.
    Initiative,
}

/// Front matter of an initiative file.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitiativeFront {
    /// Tracker id, written at publish.
    pub tracker_id: Option<String>,
    /// Always `initiative`.
    #[serde(rename = "type")]
    pub kind: InitiativeType,
    /// The outcome.
    pub title: String,
    /// Folder name of the parent initiative.
    pub parent: Option<String>,
    /// Parent ids it owns.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Inputs the requirements were derived from.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files read beside this ticket.
    #[serde(default)]
    pub companions: Vec<String>,
    /// Initiatives it waits on, by folder name.
    #[serde(default)]
    pub after: Vec<String>,
    /// `in-progress`, `done` or `dropped`.
    pub status: Option<ContainerStatus>,
    /// Best guess.
    pub risk: Option<Risk>,
    /// Tracker url.
    pub remote: Option<String>,
    /// Board state from a tracker sync.
    pub tracker_status: Option<TrackerStatus>,
}

impl InitiativeFront {
    /// Reads the front matter of the initiative file at `path`; `None` when it has none.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Option<InitiativeFront>, Finding> {
        front_matter::<InitiativeFront>(path, text).map(|(front, _)| front)
    }
}
