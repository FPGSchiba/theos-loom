//! The `format` key of Loom's settings files.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

/// The format number of a Loom settings file. Loom reads format 1 only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u32")]
pub struct Format;

impl Format {
    /// The one format number Loom reads.
    pub const CURRENT: u32 = 1;
}

impl TryFrom<u32> for Format {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == Self::CURRENT {
            Ok(Format)
        } else {
            Err(format!(
                "format {value} is not supported; Loom reads format {}",
                Self::CURRENT
            ))
        }
    }
}

impl JsonSchema for Format {
    fn schema_name() -> Cow<'static, str> {
        "Format".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "const": 1 })
    }
}
