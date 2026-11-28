//! `broken-reference`: an `after`, `needs`, `covers`, `parent`, plan `ticket` or `_state` path
//! that names something the repo does not have (spec §5.4).

use std::collections::BTreeSet;

use super::Context;
use crate::finding::{Finding, Rule};
use crate::index::{
    EpicAt, InitiativeAt, ItemKind, StoryAt, StoryHome, epic_by_id, epic_requirements,
    initiative_requirements, resolve_after,
};
use crate::model::{AfterRef, EpicAfter, InitiativeAfter, Key, Scalar};
use crate::repo::{Initiative, PlanFile, StateMirrorFile, Story};

/// Every broken reference in the repo.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for at in &cx.index.initiatives {
        initiative(cx, at, &mut out);
    }
    for at in &cx.index.epics {
        epic(cx, at, &mut out);
    }
    for at in &cx.index.stories {
        story(cx, at, &mut out);
    }
    out.extend(cx.repo.stray_plans.iter().map(stray_plan));
    for mirror in &cx.repo.state_mirrors {
        state(cx, mirror, &mut out);
    }
    out
}

fn broken(path: &str, message: String) -> Finding {
    Finding::error(Rule::BrokenReference, path, message)
}

fn breakdown_path(container: &str) -> String {
    format!("{container}/tickets/tickets.toml")
}

/// An initiative's `parent`, `after` and `covers`, and the `covers` and `after` of the
/// `[[initiative]]` and `[[epic]]` tables in its `tickets.toml`.
fn initiative(cx: &Context<'_>, at: &InitiativeAt<'_>, out: &mut Vec<Finding>) {
    let me = at.initiative;
    if let Some(file) = &me.file {
        let expected = at.parent.map(|p| p.slug.as_str());
        out.extend(parent(&file.path, file.front.parent.as_deref(), expected));
        let known: BTreeSet<&str> = cx
            .index
            .initiatives
            .iter()
            .map(|i| i.initiative.slug.as_str())
            .collect();
        for name in &file.front.after {
            if !known.contains(name.as_str()) {
                out.push(broken(
                    &file.path,
                    format!(
                        "`after` names the initiative `{name}`, which no initiative folder has; \
                         name an initiative folder or remove it"
                    ),
                ));
            }
        }
        if let Some(parent) = at.parent {
            out.extend(covers(&file.path, &file.front.covers, parent));
        }
    }
    let Some(breakdown) = &me.breakdown else {
        return;
    };
    let path = breakdown_path(&me.path);
    for row in &breakdown.initiative {
        out.extend(covers(&path, &row.covers, me));
        for after in &row.after {
            let named = match after {
                InitiativeAfter::Ref(id) => id.as_str(),
                InitiativeAfter::Needs(table) => table.initiative.as_str(),
            };
            let known = breakdown
                .initiative
                .iter()
                .any(|r| r.id.as_str() == named || r.slug == named);
            if !known {
                out.push(broken(
                    &path,
                    format!(
                        "`after` of initiative {} names the initiative `{named}`, which is no \
                         id or slug of an [[initiative]] table here; use one of them",
                        row.slug
                    ),
                ));
            }
        }
    }
    for row in &breakdown.epic {
        out.extend(covers(&path, &row.covers, me));
    }
}

/// An epic's `parent`, `covers`, `after` (file and table) and `needs`.
fn epic(cx: &Context<'_>, at: &EpicAt<'_>, out: &mut Vec<Finding>) {
    let (initiative, epic) = (at.initiative, at.epic);
    if let Some(file) = &epic.file {
        let front = &file.front;
        out.extend(parent(
            &file.path,
            front.parent.as_deref(),
            Some(&initiative.slug),
        ));
        out.extend(covers(&file.path, &front.covers, initiative));
        for on in &front.after {
            out.extend(after(&file.path, on, None, Some(initiative)));
        }
        out.extend(needs(cx, &file.path, &front.needs));
    }
    if let Some(row) = &epic.row {
        let path = breakdown_path(&initiative.path);
        for entry in &row.after {
            let on = match entry {
                EpicAfter::Ref(on) => on,
                EpicAfter::Needs(table) => &table.epic,
            };
            out.extend(after(&path, on, None, Some(initiative)));
        }
    }
}

/// A story's `parent`, and the `after`, `covers` and `needs` of its file and its entry.
fn story(cx: &Context<'_>, at: &StoryAt<'_>, out: &mut Vec<Finding>) {
    let story: &Story = at.story;
    let mates = Some(at.mates());
    let initiative = at.initiative();
    let (expected_parent, covered): (Option<&str>, Option<Covered<'_>>) = match at.home {
        StoryHome::Epic(_, epic) => (
            Some(&epic.slug),
            Some(Covered {
                name: &epic.slug,
                numbers: epic_requirements(epic),
            }),
        ),
        StoryHome::Initiative(initiative) => (
            Some(&initiative.slug),
            Some(Covered::initiative(initiative)),
        ),
        StoryHome::Backlog => (None, None),
    };
    if let Some(file) = &story.file {
        let front = &file.front;
        out.extend(parent(&file.path, front.parent.as_deref(), expected_parent));
        for on in &front.after {
            out.extend(after(&file.path, on, mates, initiative));
        }
        if let Some(covered) = &covered {
            out.extend(covered.check(&file.path, &front.covers));
        }
        out.extend(needs(cx, &file.path, &front.needs));
    }
    if let Some(entry) = &story.entry {
        let path = format!("{}/tickets.toml", story.folder);
        for on in &entry.after {
            out.extend(after(&path, on, mates, initiative));
        }
        if let Some(covered) = &covered {
            out.extend(covered.check(&path, &entry.covers));
        }
        out.extend(needs(cx, &path, &entry.needs));
    }
}

/// `parent` must name the folder of the container the file sits in; none for a top-level
/// initiative or a backlog story.
fn parent(path: &str, written: Option<&str>, expected: Option<&str>) -> Option<Finding> {
    let written = written?;
    match expected {
        Some(expected) if expected == written => None,
        Some(expected) => Some(broken(
            path,
            format!(
                "`parent` is `{written}`, but the file sits in {expected}; set `parent` to \
                 {expected}"
            ),
        )),
        None => Some(broken(
            path,
            format!(
                "`parent` is `{written}`, but this file has no parent container; remove `parent`"
            ),
        )),
    }
}

/// The requirement lines a `covers` may name, and whose they are.
struct Covered<'a> {
    name: &'a str,
    /// `None` when the owner has no readable file: nothing to check against.
    numbers: Option<BTreeSet<u32>>,
}

impl<'a> Covered<'a> {
    fn initiative(initiative: &'a Initiative) -> Covered<'a> {
        Covered {
            name: &initiative.slug,
            numbers: initiative_requirements(initiative),
        }
    }

    fn check(&self, path: &str, covers: &[Scalar]) -> Vec<Finding> {
        let Some(numbers) = &self.numbers else {
            return Vec::new();
        };
        covers
            .iter()
            .filter(|c| !requirement_number(c).is_some_and(|n| numbers.contains(&n)))
            .map(|c| {
                let known: Vec<String> = numbers.iter().map(|n| format!("R{n}")).collect();
                let known = if known.is_empty() {
                    "it has none yet".to_owned()
                } else {
                    format!("use one of {}", known.join(", "))
                };
                broken(
                    path,
                    format!(
                        "`covers` names `{c}`, which is no requirement line of {}; {known}",
                        self.name
                    ),
                )
            })
            .collect()
    }
}

fn covers(path: &str, values: &[Scalar], owner: &Initiative) -> Vec<Finding> {
    Covered::initiative(owner).check(path, values)
}

/// The n of a `covers` value written `R<n>`.
fn requirement_number(value: &Scalar) -> Option<u32> {
    match value {
        Scalar::Text(text) => text.strip_prefix('R')?.parse().ok(),
        Scalar::Int(_) | Scalar::Float(_) => None,
    }
}

/// One `after` prerequisite. `mates` are the stories of the same folder for a story (a bare id
/// is a sibling story) and `None` for an epic (a bare id is an epic id of its initiative).
fn after(
    path: &str,
    on: &AfterRef,
    mates: Option<&[Story]>,
    initiative: Option<&Initiative>,
) -> Option<Finding> {
    if resolve_after(on, mates, initiative).is_some() {
        return None;
    }
    let scope = initiative.map_or("the backlog", |i| i.slug.as_str());
    let problem = match (on, mates) {
        (AfterRef::Sibling(id), Some(_)) => format!(
            "the story `{id}`, which is no id, tracker_id or file name of a story in this folder \
             and no tracker_id in {scope}"
        ),
        (AfterRef::Sibling(id), None) => {
            format!("the epic `{id}`, which is no [[epic]] id or slug and no tracker_id in {scope}")
        }
        (AfterRef::Cross { epic, .. }, _) => match initiative {
            None => format!("the story `{on}`, but a backlog story can only wait on a sibling"),
            Some(i) => match epic_by_id(i, epic) {
                None => format!(
                    "the story `{on}`, but {} lists no epic with id `{epic}`",
                    i.slug
                ),
                Some(e) => format!("the story `{on}`, but {} has no story with that id", e.slug),
            },
        },
        (AfterRef::Epic(slug), _) => match initiative {
            Some(i) => format!("the epic `{slug}`, which is no epic of {}", i.slug),
            None => format!("the epic `{slug}`, but a backlog story can only wait on a sibling"),
        },
        (AfterRef::Name(name), Some(_)) => format!(
            "`{name}`, which is no tracker_id of an epic or story in {scope} and no file name \
             (with or without .md) of a story in this folder"
        ),
        (AfterRef::Name(name), None) => format!(
            "`{name}`, which is no epic slug and no tracker_id of an epic or story in {scope}"
        ),
    };
    Some(broken(
        path,
        format!("`after` names {problem}; change or remove it"),
    ))
}

/// Every key of `needs` must be carried by an epic or story of the project.
fn needs(cx: &Context<'_>, path: &str, keys: &[Key]) -> Vec<Finding> {
    keys.iter()
        .filter(|key| !cx.index.keys.contains_key(key))
        .map(|key| {
            broken(
                path,
                format!(
                    "`needs` names the key `{key}`, which no epic or story in the project \
                     carries; use an existing key or remove it"
                ),
            )
        })
        .collect()
}

/// A plan whose `ticket` joined no story of its folder (R2).
fn stray_plan(plan: &PlanFile) -> Finding {
    let folder = plan.path.rsplit_once('/').map_or("", |(folder, _)| folder);
    broken(
        &plan.path,
        format!(
            "the plan's `ticket` is `{}`, which names no story in {folder}; set it to the id \
             or file name of the story it plans",
            plan.front.ticket
        ),
    )
}

/// `_state/<component>/<epic key or backlog>.toml` and the keys of its `[stories]` (R-p).
fn state(cx: &Context<'_>, mirror: &StateMirrorFile, out: &mut Vec<Finding>) {
    let path = &mirror.path;
    let component = &mirror.component;
    if !cx.repo.components.iter().any(|c| &c.id == component) {
        out.push(broken(
            path,
            format!(
                "`_state/{component}/` names the component `{component}`, but there is no \
                 components/{component}/component.toml; move the file under a component's id"
            ),
        ));
        return;
    }
    let name = &mirror.name;
    let is_epic = name.parse::<Key>().is_ok_and(|key| {
        cx.index
            .keys
            .get(&key)
            .into_iter()
            .flatten()
            .any(|i| i.kind == ItemKind::Epic && &i.component == component)
    });
    if name != "backlog" && !is_epic {
        out.push(broken(
            path,
            format!(
                "the file name names the epic `{name}`, but no epic of component {component} \
                 has that key; name the file after an epic key, or backlog.toml"
            ),
        ));
    }
    for key in mirror.mirror.stories.keys() {
        let is_story = cx
            .index
            .keys
            .get(key)
            .into_iter()
            .flatten()
            .any(|i| i.kind == ItemKind::Story);
        if !is_story {
            out.push(broken(
                path,
                format!(
                    "[stories] names the key `{key}`, which no story in the project carries; \
                     remove the line or fix the key"
                ),
            ));
        }
    }
}
