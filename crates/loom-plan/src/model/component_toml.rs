//! `<component>/component.toml`: the settings of one component.

use schemars::JsonSchema;
use serde::Deserialize;

use super::format::Format;
use crate::finding::{Finding, Rule, Severity};
use crate::parse::toml_file::toml_file;

/// The contents of a `component.toml`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentToml {
    /// Settings format; must be 1.
    pub format: Format,
    /// The component.
    pub component: ComponentSettings,
}

/// The `[component]` table.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentSettings {
    /// Component id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The component's repository.
    pub repo: String,
    /// Branch phases start from.
    pub default_branch: String,
    /// Version of the kit the component uses.
    pub kit_version: Option<String>,
    /// Globs of paths agents may not change.
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
    /// How epics are sliced.
    #[serde(default)]
    pub slicing: Slicing,
    /// Where specs live, when not the default.
    pub spec_path: Option<String>,
    /// Where plans live, when not the default.
    pub plan_path: Option<String>,
}

/// How an epic is sliced into stories.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Slicing {
    /// Slice without asking.
    #[default]
    Auto,
    /// A human reviews the slices.
    Review,
}

impl ComponentToml {
    /// Parses the text of the `component.toml` at `path`.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<ComponentToml, Finding> {
        toml_file(path, text)
    }

    /// Checks that every `forbidden_paths` entry is a valid glob.
    #[must_use]
    pub fn validate(&self, path: &str) -> Vec<Finding> {
        self.component
            .forbidden_paths
            .iter()
            .filter_map(|glob| {
                globset::Glob::new(glob).err().map(|err| Finding {
                    severity: Severity::Error,
                    rule: Rule::Parse,
                    path: path.to_owned(),
                    line: None,
                    message: format!("forbidden_paths: `{glob}` is not a valid glob: {err}"),
                })
            })
            .collect()
    }
}
