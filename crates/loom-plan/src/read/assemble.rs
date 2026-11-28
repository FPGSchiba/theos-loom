//! Putting the read parts together into components, initiatives, epics and stories.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::collect::{ComponentParts, EpicParts, InitiativeParts, Parts, Tickets};
use super::join::join;
use crate::finding::{Finding, Rule};
use crate::model::EpicRow;
use crate::repo::{Component, Epic, Initiative, PlanFile, PlanningRepo, Story};

/// What building the tree adds beside the tree itself.
#[derive(Default)]
struct Sink {
    findings: Vec<Finding>,
    stray_plans: Vec<PlanFile>,
}

impl Sink {
    fn stories(
        &mut self,
        folder: &str,
        entries: &[crate::model::EntryRow],
        t: Tickets,
    ) -> Vec<Story> {
        let joined = join(folder, entries, t);
        self.findings.extend(joined.findings);
        self.stray_plans.extend(joined.stray_plans);
        joined.stories
    }
}

/// Builds the [`PlanningRepo`] from what was read.
pub(crate) fn assemble(root: PathBuf, parts: Parts) -> PlanningRepo {
    let mut sink = Sink::default();
    let components = parts
        .components
        .into_iter()
        .map(|(id, c)| component(id, c, &mut sink))
        .collect();
    let mut read_findings = parts.findings;
    read_findings.extend(sink.findings);
    PlanningRepo {
        root,
        settings: parts.settings,
        vision: parts.vision,
        project_directives: parts.project_directives,
        components,
        decisions: parts.decisions,
        state_mirrors: parts.state_mirrors,
        documents: parts.documents,
        stray_plans: sink.stray_plans,
        read_findings,
    }
}

fn component(id: String, mut parts: ComponentParts, sink: &mut Sink) -> Component {
    let path = format!("components/{id}");
    let (initiative, extra_initiatives) = initiatives(&path, &mut parts.initiatives, sink);
    let backlog_path = format!("{path}/backlog");
    let backlog = sink.stories(&backlog_path, &[], parts.backlog);
    Component {
        id,
        path,
        settings: parts.settings,
        engineering: parts.engineering,
        design: parts.design,
        initiative,
        extra_initiatives,
        backlog,
    }
}

/// The one top-level initiative with its children, and the names of further top-level ones
/// (R-j). A folder is a child when a `[[initiative]]` table of another folder names it.
fn initiatives(
    component: &str,
    folders: &mut BTreeMap<String, InitiativeParts>,
    sink: &mut Sink,
) -> (Option<Initiative>, Vec<String>) {
    let children: BTreeSet<String> = folders
        .iter()
        .flat_map(|(slug, i)| {
            i.breakdown
                .iter()
                .flat_map(|b| b.initiative.iter())
                .filter(move |row| row.slug != *slug)
                .map(|row| row.slug.clone())
        })
        .collect();
    let mut top: Vec<String> = folders
        .keys()
        .filter(|slug| !children.contains(*slug))
        .cloned()
        .collect();
    if top.is_empty() {
        top.extend(folders.keys().next().cloned());
    }
    let mut top = top.into_iter();
    let Some(first) = top.next() else {
        return (None, Vec::new());
    };
    let extra: Vec<String> = top.collect();
    for slug in &extra {
        sink.findings.push(Finding::error(
            Rule::Layout,
            &format!("{component}/{slug}"),
            format!(
                "{component}/ has a second top-level initiative, {slug}, beside {first}; a \
                 component has one initiative folder (a parent initiative lists its children in \
                 its tickets.toml)"
            ),
        ));
    }
    let main = initiative(component, &first, folders, sink);
    (main, extra)
}

/// Builds the initiative `slug` and, through its `[[initiative]]` tables, its children. Each
/// folder is taken out of `folders` once, so a loop of children ends.
fn initiative(
    component: &str,
    slug: &str,
    folders: &mut BTreeMap<String, InitiativeParts>,
    sink: &mut Sink,
) -> Option<Initiative> {
    let parts = folders.remove(slug)?;
    let path = format!("{component}/{slug}");
    let tickets = format!("{path}/tickets");
    let (epic_rows, entries, child_rows) = parts.breakdown.as_ref().map_or_else(
        || (Vec::new(), Vec::new(), Vec::new()),
        |b| (b.epic.clone(), b.entry.clone(), b.initiative.clone()),
    );
    let epics = epics(&tickets, epic_rows, parts.epics, sink);
    let stories = sink.stories(&tickets, &entries, parts.tickets);
    let children = child_rows
        .iter()
        .filter_map(|row| initiative(component, &row.slug, folders, sink))
        .collect();
    Some(Initiative {
        slug: slug.to_owned(),
        path,
        file: parts.file,
        breakdown: parts.breakdown,
        epics,
        stories,
        children,
    })
}

/// Epics in build order: `[[epic]]` tables first (with their folder when there is one), then
/// folders no table names, by name.
fn epics(
    tickets: &str,
    rows: Vec<EpicRow>,
    mut folders: BTreeMap<String, EpicParts>,
    sink: &mut Sink,
) -> Vec<Epic> {
    let mut listed: Vec<(String, Option<EpicRow>, EpicParts)> = rows
        .into_iter()
        .map(|row| {
            let parts = folders.remove(&row.slug).unwrap_or_default();
            (row.slug.clone(), Some(row), parts)
        })
        .collect();
    listed.extend(folders.into_iter().map(|(slug, parts)| (slug, None, parts)));
    listed
        .into_iter()
        .enumerate()
        .map(|(order, (slug, row, parts))| {
            let path = format!("{tickets}/{slug}");
            let entries = parts
                .breakdown
                .as_ref()
                .map(|b| b.entry.clone())
                .unwrap_or_default();
            let stories = sink.stories(&format!("{path}/tickets"), &entries, parts.tickets);
            Epic {
                slug,
                path,
                file: parts.file,
                body: parts.body,
                row,
                order,
                breakdown: parts.breakdown,
                stories,
            }
        })
        .collect()
}
