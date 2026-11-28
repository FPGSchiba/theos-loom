//! Directive files: the numbered, named rules of `project.md` and a component's directive files.

use serde::Serialize;

use super::document::DocFront;

/// Which directive file family a directive belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectiveKind {
    /// An engineering directive.
    Engineering,
    /// A design directive.
    Design,
}

/// One directive: a top-level item of an ordered list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The leading bold span without its trailing full stop; `None` when the item has none.
    pub name: Option<String>,
    /// Engineering or design.
    pub kind: DirectiveKind,
    /// The text of the item, marker removed.
    pub text: String,
    /// The name in a `Contradicts: <name>` line of the item.
    pub contradicts: Option<String>,
    /// 1-based file line of the item's first line.
    pub line: u32,
}

/// A directive file read into data; whether it is correct is for the check rules to judge.
#[derive(Debug, Clone)]
pub struct DirectiveFile {
    /// Repo-relative path.
    pub path: String,
    /// The front matter, when the file has one.
    pub front: Option<DocFront>,
    /// The directives in file order.
    pub directives: Vec<Directive>,
}
