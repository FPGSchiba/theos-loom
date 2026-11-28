//! Reading each classified file into its model and sorting it into the parts of the tree.
//!
//! A file that does not parse becomes a finding and is left out; one bad file never hides the
//! others. Which part a file goes to is decided by [`classify`](super::classify).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::classify::{Container, FileKind};
use crate::finding::{Finding, Rule};
use crate::model::{
    Breakdown, ComponentToml, DecisionRecord, DirectiveFile, DirectiveKind, DocFront, Document,
    EpicFront, InitiativeFront, LeafFront, LeafKind, LoomToml, PlanFront, StateMirror,
};
use crate::parse::body::{EpicBody, Sections, criteria, requirement_lines};
use crate::parse::directives::parse_directives;
use crate::parse::front_matter::front_matter;
use crate::parse::parse_error;
use crate::repo::{EpicFile, InitiativeFile, LeafFile, PlanFile, StateMirrorFile, Vision};

/// The leaves and plans of one ticket folder, in path order.
#[derive(Debug, Default)]
pub(crate) struct Tickets {
    pub leaves: Vec<LeafFile>,
    pub plans: Vec<PlanFile>,
}

/// What was read of one epic folder.
#[derive(Debug, Default)]
pub(crate) struct EpicParts {
    pub file: Option<EpicFile>,
    pub body: Option<EpicBody>,
    pub breakdown: Option<Breakdown>,
    pub tickets: Tickets,
}

/// What was read of one initiative folder.
#[derive(Debug, Default)]
pub(crate) struct InitiativeParts {
    pub file: Option<InitiativeFile>,
    pub breakdown: Option<Breakdown>,
    pub epics: BTreeMap<String, EpicParts>,
    pub tickets: Tickets,
}

/// What was read of one component folder.
#[derive(Debug, Default)]
pub(crate) struct ComponentParts {
    pub settings: Option<ComponentToml>,
    pub engineering: Option<DirectiveFile>,
    pub design: Option<DirectiveFile>,
    pub initiatives: BTreeMap<String, InitiativeParts>,
    pub backlog: Tickets,
}

/// Everything read, before the tree is put together.
#[derive(Debug, Default)]
pub(crate) struct Parts {
    pub settings: Option<LoomToml>,
    pub vision: Option<Vision>,
    pub project_directives: Option<DirectiveFile>,
    pub decisions: Vec<DecisionRecord>,
    pub state_mirrors: Vec<StateMirrorFile>,
    pub documents: Vec<Document>,
    pub components: BTreeMap<String, ComponentParts>,
    pub findings: Vec<Finding>,
    /// Paths already named by a `layout` finding for a misplaced ticket.
    misplaced: BTreeSet<String>,
}

impl Parts {
    /// Makes sure the component, initiative and epic folders `segments` runs through exist as
    /// parts, so a folder holding only companions still counts. Only folders of `components`
    /// (those with a `component.toml`) are components.
    pub(crate) fn touch(&mut self, segments: &[&str], components: &BTreeSet<String>) {
        let ["components", component, rest @ ..] = segments else {
            return;
        };
        if !components.contains(*component) {
            return;
        }
        let Some((_, folders)) = rest.split_last() else {
            return;
        };
        let parts = self.components.entry((*component).to_owned()).or_default();
        if let [initiative, tail @ ..] = folders
            && super::classify::is_initiative_name(initiative)
        {
            let i = parts
                .initiatives
                .entry((*initiative).to_owned())
                .or_default();
            if let ["tickets", epic, ..] = tail
                && super::classify::is_epic_name(epic)
            {
                i.epics.entry((*epic).to_owned()).or_default();
            }
        }
    }

    /// Reads the file at `path` (repo-relative) of `kind` from its `bytes`.
    pub(crate) fn add(&mut self, path: &str, kind: FileKind, bytes: &[u8]) {
        match kind {
            FileKind::Document => self.documents.push(Document::from_bytes(path, bytes)),
            FileKind::Misplaced { report } => {
                if self.misplaced.insert(report.clone()) {
                    self.findings.push(Finding::error(
                        Rule::Layout,
                        &report,
                        format!(
                            "{report} is a ticket where the tree allows none: tickets.toml, leaf \
                             files and plans go in an initiative's or epic's tickets/ folder (or \
                             backlog/), epic folders in an initiative's tickets/ folder"
                        ),
                    ));
                }
                self.documents.push(Document::from_bytes(path, bytes));
            }
            FileKind::Vision => {
                let front = self
                    .text(path, bytes)
                    .and_then(|t| self.ok(DocFront::parse(path, t)));
                self.vision = Some(Vision {
                    document: Document::from_bytes(path, bytes),
                    front: front.flatten(),
                });
            }
            typed => {
                if let Some(text) = self.text(path, bytes) {
                    self.add_typed(path, typed, text);
                }
            }
        }
    }

    fn add_typed(&mut self, path: &str, kind: FileKind, text: &str) {
        match kind {
            FileKind::LoomToml => self.settings = self.ok(LoomToml::parse(path, text)),
            FileKind::ProjectDirectives => {
                self.project_directives = self.ok(parse_directives(path, text, None));
            }
            FileKind::Decision => {
                if let Some(d) = self.ok(DecisionRecord::parse(path, text)) {
                    self.decisions.push(d);
                }
            }
            FileKind::StateMirror { component, name } => {
                if let Some(mirror) = self.ok(StateMirror::parse(path, text)) {
                    self.state_mirrors.push(StateMirrorFile {
                        path: path.to_owned(),
                        component,
                        name,
                        mirror,
                    });
                }
            }
            FileKind::ComponentToml { component } => self.component_toml(path, &component, text),
            FileKind::ComponentDirectives { component, kind } => {
                let file = self.ok(parse_directives(path, text, Some(kind)));
                let parts = self.components.entry(component).or_default();
                match kind {
                    DirectiveKind::Engineering => parts.engineering = file,
                    DirectiveKind::Design => parts.design = file,
                }
            }
            FileKind::InitiativeFile {
                component,
                initiative,
            } => {
                let file = self.required(path, text, "an initiative", InitiativeFront::parse);
                self.initiative(&component, &initiative).file = file.map(|front| InitiativeFile {
                    path: path.to_owned(),
                    front,
                    requirements: requirement_lines(&Sections::from_file_text(text)),
                });
            }
            FileKind::EpicFile {
                component,
                initiative,
                epic,
            } => {
                let file = self.required(path, text, "an epic", EpicFront::parse);
                let parts = self.initiative(&component, &initiative).epics.entry(epic);
                let parts = parts.or_default();
                parts.body = Some(EpicBody::parse(text));
                parts.file = file.map(|front| EpicFile {
                    path: path.to_owned(),
                    front,
                });
            }
            FileKind::Breakdown { component, at } => {
                let breakdown = self.ok(Breakdown::parse(path, text));
                if breakdown.as_ref().is_some_and(|b| b.kinds() > 1) {
                    self.findings.push(Finding::error(
                        Rule::Layout,
                        path,
                        format!(
                            "{path} mixes [[initiative]], [[epic]] and [[entry]] tables; a \
                             tickets.toml holds one kind of table"
                        ),
                    ));
                }
                match at {
                    Container::Initiative { initiative } => {
                        self.initiative(&component, &initiative).breakdown = breakdown;
                    }
                    Container::Epic { initiative, epic } => {
                        let parts = self.initiative(&component, &initiative).epics.entry(epic);
                        parts.or_default().breakdown = breakdown;
                    }
                    // Never reached: `classify` sends `backlog/tickets.toml` to `Misplaced`, a
                    // `layout` finding (backlog leaves have no entries, R-n).
                    Container::Backlog => {}
                }
            }
            FileKind::Ticket { component, at } => self.ticket(path, &component, &at, text),
            FileKind::Document | FileKind::Misplaced { .. } | FileKind::Vision => {}
        }
    }

    fn component_toml(&mut self, path: &str, component: &str, text: &str) {
        let settings = self.ok(ComponentToml::parse(path, text));
        if let Some(settings) = &settings {
            self.findings.extend(settings.validate(path));
            if settings.component.id != component {
                self.findings.push(Finding::error(
                    Rule::Layout,
                    path,
                    format!(
                        "{path} says id = \"{}\" but sits in components/{component}/; a \
                         component's id is its folder name",
                        settings.component.id
                    ),
                ));
            }
        }
        let parts = self.components.entry(component.to_owned()).or_default();
        parts.settings = settings;
    }

    fn ticket(&mut self, path: &str, component: &str, at: &Container, text: &str) {
        let is_plan = path.ends_with("-plan.md") && !names_a_leaf_type(text);
        if is_plan {
            if let Some(front) = self.required(path, text, "a plan", PlanFront::parse) {
                self.tickets(component, at).plans.push(PlanFile {
                    path: path.to_owned(),
                    front,
                });
            }
            return;
        }
        if let Some(front) = self.required(path, text, "a story, spike or bug", LeafFront::parse) {
            let heading = match front.kind {
                LeafKind::Story => "Requirements",
                LeafKind::Spike | LeafKind::Bug => "Acceptance Criteria",
            };
            let found = criteria(&Sections::from_file_text(text), heading);
            let stem = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".md")
                .to_owned();
            self.tickets(component, at).leaves.push(LeafFile {
                path: path.to_owned(),
                stem,
                front,
                criteria: found,
            });
        }
    }

    fn tickets(&mut self, component: &str, at: &Container) -> &mut Tickets {
        match at {
            Container::Backlog => {
                &mut self
                    .components
                    .entry(component.to_owned())
                    .or_default()
                    .backlog
            }
            Container::Initiative { initiative } => {
                &mut self.initiative(component, initiative).tickets
            }
            Container::Epic { initiative, epic } => {
                &mut self
                    .initiative(component, initiative)
                    .epics
                    .entry(epic.clone())
                    .or_default()
                    .tickets
            }
        }
    }

    fn initiative(&mut self, component: &str, initiative: &str) -> &mut InitiativeParts {
        self.components
            .entry(component.to_owned())
            .or_default()
            .initiatives
            .entry(initiative.to_owned())
            .or_default()
    }

    /// The text of a typed file; a file that is not UTF-8 is a finding.
    fn text<'b>(&mut self, path: &str, bytes: &'b [u8]) -> Option<&'b str> {
        match std::str::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(err) => {
                self.findings
                    .push(parse_error(path, &format!("not UTF-8 text ({err})"), None));
                None
            }
        }
    }

    /// The value, or `None` with its finding recorded.
    fn ok<T>(&mut self, result: Result<T, Finding>) -> Option<T> {
        result.map_err(|f| self.findings.push(f)).ok()
    }

    /// The front matter a typed file must have; missing or broken front matter is a finding.
    fn required<T>(
        &mut self,
        path: &str,
        text: &str,
        what: &str,
        parse: fn(&str, &str) -> Result<Option<T>, Finding>,
    ) -> Option<T> {
        match parse(path, text) {
            Ok(Some(front)) => Some(front),
            Ok(None) => {
                self.findings.push(missing_front_matter(path, text, what));
                None
            }
            Err(finding) => {
                self.findings.push(finding);
                None
            }
        }
    }
}

/// The finding for a typed file whose front matter is absent, or whose `---` fence is broken.
fn missing_front_matter(path: &str, text: &str, what: &str) -> Finding {
    let detail = if text.trim_start_matches('\u{feff}').starts_with("---") {
        format!(
            "the front matter fence is broken: the opening `---` has no closing `---`, or the \
             block is empty; {what} file starts with a closed `---` block holding its keys"
        )
    } else {
        format!("it has no front matter; {what} file starts with a `---` block holding its keys")
    };
    parse_error(path, &detail, Some(1))
}

/// Front matter read only for its `type`, to tell a leaf named `…-plan.md` from a plan.
#[derive(Deserialize)]
struct TypePeek {
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// True when the front matter's `type` is a leaf type.
fn names_a_leaf_type(text: &str) -> bool {
    front_matter::<TypePeek>("", text)
        .ok()
        .and_then(|(peek, _)| peek?.kind)
        .is_some_and(|kind| matches!(kind.as_str(), "story" | "spike" | "bug"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leaf_named_like_a_plan_stays_a_leaf() {
        assert!(names_a_leaf_type("---\ntype: story\ntitle: x\n---\n"));
        assert!(!names_a_leaf_type("---\nticket: 1\ntype: feature\n---\n"));
        assert!(!names_a_leaf_type("no front matter"));
    }

    #[test]
    fn missing_and_broken_front_matter_say_which() {
        let broken = missing_front_matter("a.md", "---\nid: 1\n", "a story");
        assert!(broken.message.contains("fence"), "{}", broken.message);
        let missing = missing_front_matter("a.md", "# A\n", "a story");
        assert!(
            missing.message.contains("no front matter"),
            "{}",
            missing.message
        );
    }
}
