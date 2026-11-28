//! Value types shared by the ticket-tree models: risk, severity, tracker states, scalars.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// `risk`: a best guess for a container, a call for a leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// Low risk.
    Low,
    /// Medium risk.
    Medium,
    /// High risk.
    High,
}

/// `severity` of a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Severity {
    /// Outage, data loss, security exposure.
    P0,
    /// Core function broken, no workaround.
    P1,
    /// Impaired, a workaround exists.
    P2,
    /// Cosmetic.
    P3,
}

/// `tracker_status`: the board state a tracker sync writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TrackerStatus {
    /// Not started.
    Backlog,
    /// Being worked on.
    InProgress,
    /// Built, in review.
    Review,
    /// Done.
    Done,
    /// Dropped.
    Dropped,
}

/// `status` of a container (initiative or epic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerStatus {
    /// Its first ticket started.
    InProgress,
    /// Finished.
    Done,
    /// Given up.
    Dropped,
}

/// The PO's eight statuses of a leaf, as the plan file carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    /// Being drafted.
    Draft,
    /// Ready to build.
    ReadyForDev,
    /// Being built.
    InProgress,
    /// In review.
    InReview,
    /// Built, nobody called it done.
    Built,
    /// Done.
    Done,
    /// Halted with a reason.
    Blocked,
    /// Given up.
    Dropped,
}

/// Where a leaf came from (`origin` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// Written by Loom's slicing.
    Sliced,
}

/// A YAML or TOML value the PO writes as a number or as text (`covers`, `estimate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    /// A whole number.
    Int(i64),
    /// A decimal number.
    Float(f64),
    /// Text.
    Text(String),
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

impl JsonSchema for Scalar {
    fn schema_name() -> Cow<'static, str> {
        "Scalar".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "number"] })
    }
}
