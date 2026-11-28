//! Free-form Markdown documents and the typed front matter of `vision.md` and directive files.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::finding::Finding;
use crate::parse::front_matter::{front_matter, split_front_matter};

/// A free-form file: its front matter is kept raw and not validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Repo-relative path.
    pub path: String,
    /// The raw YAML between the `---` fences, when there is one.
    pub front_matter: Option<String>,
    /// The body after the front matter; `None` when the file is not UTF-8.
    pub text: Option<String>,
}

impl Document {
    /// Builds a document from the bytes of the file at `path`.
    #[must_use]
    pub fn from_bytes(path: &str, bytes: &[u8]) -> Document {
        let (front, text) = match std::str::from_utf8(bytes) {
            Ok(text) => {
                let split = split_front_matter(text);
                (
                    split.front.map(|f| f.text.to_owned()),
                    Some(split.body.to_owned()),
                )
            }
            Err(_) => (None, None),
        };
        Document {
            path: path.to_owned(),
            front_matter: front,
            text,
        }
    }
}

/// The front matter of `vision.md` and of directive files.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocFront {
    /// `draft` while the document is not binding yet.
    pub status: Option<DocStatus>,
}

/// The status a vision or directive file may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DocStatus {
    /// Not binding yet.
    Draft,
}

impl DocFront {
    /// Reads the front matter of the Markdown file at `path`; `None` when it has none.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Option<DocFront>, Finding> {
        front_matter::<DocFront>(path, text).map(|(front, _)| front)
    }
}
