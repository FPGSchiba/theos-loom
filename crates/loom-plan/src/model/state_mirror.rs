//! `_state/<component>/<epic key>.toml` and `backlog.toml`: the mirror of epic and story state.

use std::collections::BTreeMap;

use loom_core::state::{EpicState, StoryState};
use schemars::JsonSchema;
use serde::Deserialize;

use super::format::Format;
use super::ids::Key;
use crate::finding::Finding;
use crate::parse::toml_file::toml_file;

/// The state of one epic and its stories, as P02 writes it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateMirror {
    /// Settings format; must be 1.
    pub format: Format,
    /// The epic's state; absent in `backlog.toml`.
    pub epic: Option<EpicState>,
    /// Story key to story state.
    #[serde(default)]
    pub stories: BTreeMap<Key, StoryState>,
}

impl StateMirror {
    /// Parses the text of the state file at `path`.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<StateMirror, Finding> {
        toml_file(path, text)
    }
}
