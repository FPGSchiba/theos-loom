//! `loom.toml`: the project settings of a planning repo.

use schemars::JsonSchema;
use serde::Deserialize;

use super::format::Format;
use crate::finding::Finding;
use crate::parse::toml_file::toml_file;

/// The contents of `loom.toml`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoomToml {
    /// Settings format; must be 1.
    pub format: Format,
    /// The project.
    pub project: ProjectSettings,
    /// Optional budget weight.
    pub budget: Option<Budget>,
    /// Optional notification target.
    pub notify: Option<Notify>,
}

/// The `[project]` table.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSettings {
    /// Project id.
    pub id: String,
    /// Display name.
    pub name: String,
}

/// The `[budget]` table.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Share of the shared budget this project gets.
    pub weight: u32,
}

/// The `[notify]` table.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    /// Where notifications go.
    pub target: String,
}

impl LoomToml {
    /// Parses the text of the `loom.toml` at `path`.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<LoomToml, Finding> {
        toml_file(path, text)
    }
}
