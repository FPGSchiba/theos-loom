//! JSON schemas of every front-matter and settings type, generated with `schemars`.

use std::path::{Path, PathBuf};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::Value;

use crate::model::{
    Breakdown, ComponentToml, DecisionFront, DocFront, EpicFront, InitiativeFront, LeafFront,
    LoomToml, PlanFront, StateMirror,
};
use crate::parse::front_matter::split_front_matter;

fn schema_of<T: JsonSchema>() -> Schema {
    SchemaGenerator::default().into_root_schema_for::<T>()
}

/// Every schema as `(name, schema)`; the name is the file stem of `<name>.schema.json`.
#[must_use]
pub fn schemas() -> Vec<(&'static str, Schema)> {
    vec![
        ("initiative", schema_of::<InitiativeFront>()),
        ("epic", schema_of::<EpicFront>()),
        ("leaf", schema_of::<LeafFront>()),
        ("plan", schema_of::<PlanFront>()),
        ("document-front-matter", schema_of::<DocFront>()),
        ("decision", schema_of::<DecisionFront>()),
        ("loom-toml", schema_of::<LoomToml>()),
        ("component-toml", schema_of::<ComponentToml>()),
        ("tickets-toml", schema_of::<Breakdown>()),
        ("state-mirror", schema_of::<StateMirror>()),
    ]
}

/// Writes `<name>.schema.json` for every schema into `dir` (pretty JSON, LF, trailing newline).
///
/// # Errors
/// Fails when `dir` can't be written.
pub fn write_schemas(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (name, schema) in schemas() {
        let mut text = serde_json::to_string_pretty(&schema).map_err(std::io::Error::other)?;
        text.push('\n');
        let path = dir.join(format!("{name}.schema.json"));
        std::fs::write(&path, text)?;
        written.push(path);
    }
    Ok(written)
}

/// Reads the YAML front matter of a Markdown file as JSON, so it can be checked against a schema.
///
/// Returns `Ok(None)` when the file has no front-matter block. Unquoted YAML scalars keep their
/// YAML type (`1.3` is a number), exactly as a schema validator sees them.
///
/// # Errors
/// Fails when the block is not valid YAML.
pub fn front_matter_json(text: &str) -> Result<Option<Value>, serde_saphyr::Error> {
    match split_front_matter(text).front {
        Some(block) => serde_saphyr::from_str(block.text).map(Some),
        None => Ok(None),
    }
}
