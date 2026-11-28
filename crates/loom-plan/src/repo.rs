//! The planning repo as one value: what `read` returns and the check rules walk.

mod story;

use std::path::PathBuf;

pub use story::{LeafFile, PlanFile, Story};

use crate::finding::Finding;
use crate::model::{
    AfterRef, Breakdown, ComponentToml, ContainerStatus, DecisionRecord, DirectiveFile, DocFront,
    Document, EpicAfter, EpicFront, EpicRow, InitiativeFront, Key, LoomMark, LoomToml, StateMirror,
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

/// One prerequisite of an epic: the epic it waits on and, when its `[[epic]]` table says so,
/// what it needs from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicPrerequisite {
    /// What the epic waits on, in the PO's `after` forms (an id, `"<epic>.<entry>"` or
    /// `"epic-<slug>"`).
    pub on: AfterRef,
    /// The `needs` text of an `{ epic, needs }` table.
    pub needs: Option<String>,
    /// Where it is written.
    pub source: PrerequisiteSource,
}

/// Where an epic prerequisite is written: it decides what the prerequisite may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrerequisiteSource {
    /// The epic file's `after`: an epic or a story.
    File,
    /// The `after` of the epic's `[[epic]]` table: an epic only (`tickets.py` `declared_after`).
    Table,
}

impl Epic {
    /// Every prerequisite of the epic: its file's `after` first, then its `[[epic]]` table's
    /// `after` (an `{ epic = 1, needs = ".." }` table becomes the id `1` with its `needs`). Both
    /// sources count (spec §5.5); a prerequisite named in both appears twice.
    #[must_use]
    pub fn after(&self) -> Vec<EpicPrerequisite> {
        let from_file = self
            .file
            .iter()
            .flat_map(|f| &f.front.after)
            .map(|on| EpicPrerequisite {
                on: on.clone(),
                needs: None,
                source: PrerequisiteSource::File,
            });
        let from_row = self.row.iter().flat_map(|r| &r.after).map(|a| match a {
            EpicAfter::Ref(on) => EpicPrerequisite {
                on: on.clone(),
                needs: None,
                source: PrerequisiteSource::Table,
            },
            EpicAfter::Needs(table) => EpicPrerequisite {
                on: table.epic.clone(),
                needs: table.needs.clone(),
                source: PrerequisiteSource::Table,
            },
        });
        from_file.chain(from_row).collect()
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TicketId;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn epic_after_joins_file_and_table() -> TestResult {
        let front = EpicFront::parse(
            "e.md",
            "---
type: epic
title: E
after: [epic-a]
---
",
        )
        .map_err(|f| f.message)?
        .ok_or("no front matter")?;
        let breakdown = Breakdown::parse(
            "tickets.toml",
            "[[epic]]
id = 3
slug = \"epic-e\"
title = \"E\"
after = [2, { epic = 1, needs = \"the price\" }]
",
        )
        .map_err(|f| f.message)?;
        let epic = Epic {
            slug: "epic-e".to_owned(),
            path: "x/epic-e".to_owned(),
            file: Some(EpicFile {
                path: "x/epic-e/epic-e.md".to_owned(),
                front,
            }),
            body: None,
            row: breakdown.epic.into_iter().next(),
            order: 0,
            breakdown: None,
            stories: Vec::new(),
        };
        let id = |s: &str| TicketId::try_from(s.to_owned());
        assert_eq!(
            epic.after(),
            [
                EpicPrerequisite {
                    on: AfterRef::Epic("epic-a".to_owned()),
                    needs: None,
                    source: PrerequisiteSource::File,
                },
                EpicPrerequisite {
                    on: AfterRef::Sibling(id("2")?),
                    needs: None,
                    source: PrerequisiteSource::Table,
                },
                EpicPrerequisite {
                    on: AfterRef::Sibling(id("1")?),
                    needs: Some("the price".to_owned()),
                    source: PrerequisiteSource::Table,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn epic_without_file_or_table_has_no_prerequisites() {
        let epic = Epic {
            slug: "epic-e".to_owned(),
            path: "x/epic-e".to_owned(),
            file: None,
            body: None,
            row: None,
            order: 0,
            breakdown: None,
            stories: Vec::new(),
        };
        assert_eq!(epic.after(), []);
    }
}
