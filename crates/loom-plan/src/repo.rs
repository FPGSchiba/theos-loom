//! The planning repo as one value: what `read` returns and the check rules walk.

mod story;

use std::path::PathBuf;

pub use story::{LeafFile, PlanFile, Story};

use crate::finding::Finding;
use crate::model::{
    Breakdown, ComponentToml, ContainerStatus, DecisionRecord, DirectiveFile, DocFront, Document,
    EpicFront, EpicRow, InitiativeFront, Key, LoomMark, LoomToml, StateMirror,
};
use crate::parse::body::{EpicBody, RequirementLine};

/// A whole planning repo (spec §5.1).
#[derive(Debug, Clone)]
pub struct PlanningRepo {
    /// The folder it was read from, as given.
    pub root: PathBuf,
    /// `loom.toml`, when present and readable.
    pub settings: Option<LoomToml>,
    /// `vision.md`, when present.
    pub vision: Option<Vision>,
    /// `directives/project.md`, when present and readable.
    pub project_directives: Option<DirectiveFile>,
    /// The `components/<id>/` folders, by id.
    pub components: Vec<Component>,
    /// `decisions/NNNN-<slug>.md`, by file name.
    pub decisions: Vec<DecisionRecord>,
    /// `_state/<component>/<name>.toml`, by path.
    pub state_mirrors: Vec<StateMirrorFile>,
    /// Free-form files (R-e), by path.
    pub documents: Vec<Document>,
    /// Plan files whose `ticket` joins no leaf or entry of their folder (R2), kept so a rule can
    /// report them.
    pub stray_plans: Vec<PlanFile>,
    /// What reading found: files that do not parse, unknown keys, layout and drift.
    pub read_findings: Vec<Finding>,
}

/// `vision.md`: a document with an optional `status: draft`.
#[derive(Debug, Clone)]
pub struct Vision {
    /// The file as a document.
    pub document: Document,
    /// Its front matter, when it has one that parses.
    pub front: Option<DocFront>,
}

/// One `_state/<component>/<name>.toml` with its place (R-p).
#[derive(Debug, Clone)]
pub struct StateMirrorFile {
    /// Repo-relative path.
    pub path: String,
    /// The component folder it sits in.
    pub component: String,
    /// The file stem: an epic key, or `backlog`.
    pub name: String,
    /// The contents.
    pub mirror: StateMirror,
}

/// One `components/<id>/` folder.
#[derive(Debug, Clone)]
pub struct Component {
    /// The folder name.
    pub id: String,
    /// Repo-relative path of the folder.
    pub path: String,
    /// `component.toml`, when present and readable.
    pub settings: Option<ComponentToml>,
    /// `directives/engineering.md`.
    pub engineering: Option<DirectiveFile>,
    /// `directives/design.md`.
    pub design: Option<DirectiveFile>,
    /// The component's initiative: the one top-level initiative folder (R-j).
    pub initiative: Option<Initiative>,
    /// Folder names of further top-level initiatives; each is a `layout` finding (R-j).
    pub extra_initiatives: Vec<String>,
    /// The leaves of `backlog/` (R-q).
    pub backlog: Vec<Story>,
}

/// An `initiative-<slug>/` folder.
#[derive(Debug, Clone)]
pub struct Initiative {
    /// The folder name.
    pub slug: String,
    /// Repo-relative path of the folder.
    pub path: String,
    /// `initiative-<slug>.md`, when present and readable.
    pub file: Option<InitiativeFile>,
    /// `tickets/tickets.toml`, when present and readable.
    pub breakdown: Option<Breakdown>,
    /// Its epics in build order: `[[epic]]` table order, then unlisted folders by name.
    pub epics: Vec<Epic>,
    /// Leaves directly in its `tickets/` folder (an initiative without epics).
    pub stories: Vec<Story>,
    /// Child initiatives in `[[initiative]]` table order (folders beside this one).
    pub children: Vec<Initiative>,
}

/// An initiative's own file.
#[derive(Debug, Clone)]
pub struct InitiativeFile {
    /// Repo-relative path.
    pub path: String,
    /// Its front matter.
    pub front: InitiativeFront,
    /// The `R<n>` lines under its Requirements.
    pub requirements: Vec<RequirementLine>,
}

/// An `epic-<slug>/` folder, or an `[[epic]]` table without a folder yet.
#[derive(Debug, Clone)]
pub struct Epic {
    /// The folder name (the table's `slug`).
    pub slug: String,
    /// Repo-relative path of the folder.
    pub path: String,
    /// `epic-<slug>.md`, when present and readable.
    pub file: Option<EpicFile>,
    /// What the epic checks need of its body, when the file is text.
    pub body: Option<EpicBody>,
    /// Its `[[epic]]` table in the initiative's `tickets.toml`.
    pub row: Option<EpicRow>,
    /// Position in build order within its initiative, from 0.
    pub order: usize,
    /// `tickets/tickets.toml`, when present and readable.
    pub breakdown: Option<Breakdown>,
    /// Its stories in build order.
    pub stories: Vec<Story>,
}

/// An epic's own file.
#[derive(Debug, Clone)]
pub struct EpicFile {
    /// Repo-relative path.
    pub path: String,
    /// Its front matter.
    pub front: EpicFront,
}

impl Epic {
    /// The epic's `key`.
    #[must_use]
    pub fn key(&self) -> Option<&Key> {
        self.file.as_ref().and_then(|f| f.front.key.as_ref())
    }

    /// The epic's `loom` mark.
    #[must_use]
    pub fn loom(&self) -> Option<LoomMark> {
        self.file.as_ref().and_then(|f| f.front.loom)
    }

    /// The epic's cross-component prerequisites.
    #[must_use]
    pub fn needs(&self) -> &[Key] {
        self.file.as_ref().map_or(&[], |f| f.front.needs.as_slice())
    }

    /// True when the epic file says `status: dropped`.
    #[must_use]
    pub fn is_dropped(&self) -> bool {
        self.file
            .as_ref()
            .is_some_and(|f| f.front.status == Some(ContainerStatus::Dropped))
    }
}
