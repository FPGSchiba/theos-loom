//! Joining one ticket folder: entries with leaf files by `id`, plans with leaves by `ticket`
//! (the PO's rules in `tickets.py`), and the `drift` findings of R-o.

use std::cmp::Ordering;

use super::collect::Tickets;
use crate::finding::{Finding, Rule};
use crate::model::{EntryRow, Key, LeafFront, LoomMark, TicketId};
use crate::repo::{LeafFile, PlanFile, Story};

/// What a folder's join gives besides its stories.
pub(crate) struct Joined {
    /// Stories in build order.
    pub stories: Vec<Story>,
    /// Plans whose `ticket` names no story of the folder (R2).
    pub stray_plans: Vec<PlanFile>,
    /// `drift` and duplicate-plan findings.
    pub findings: Vec<Finding>,
}

/// Joins the folder at `folder` (repo-relative): its `[[entry]]` tables and its files.
///
/// Order is the PO's: entries in table order, then leaf files without an entry by id (numbers
/// before text), then leaf files without an id.
pub(crate) fn join(folder: &str, entries: &[EntryRow], tickets: Tickets) -> Joined {
    let mut findings = Vec::new();
    let mut stories: Vec<Story> = entries
        .iter()
        .map(|entry| Story {
            kind: entry.kind,
            id: Some(entry.id.clone()),
            entry: Some(entry.clone()),
            file: None,
            plan: None,
            order: 0,
            folder: folder.to_owned(),
        })
        .collect();
    let mut unlisted = Vec::new();
    let mut without_id = Vec::new();
    for leaf in tickets.leaves {
        let slot = leaf.front.id.as_ref().and_then(|id| {
            stories
                .iter_mut()
                .find(|s| s.entry.is_some() && s.file.is_none() && s.id.as_ref() == Some(id))
        });
        if let Some(story) = slot {
            if let Some(entry) = &story.entry {
                findings.extend(drift(folder, entry, &leaf));
            }
            story.kind = leaf.front.kind;
            story.file = Some(leaf);
        } else if leaf.front.id.is_some() {
            unlisted.push(file_story(folder, leaf));
        } else {
            without_id.push(file_story(folder, leaf));
        }
    }
    unlisted.sort_by(|a, b| id_order(a.id.as_ref(), b.id.as_ref()));
    stories.extend(unlisted);
    stories.extend(without_id);
    for (order, story) in stories.iter_mut().enumerate() {
        story.order = order;
    }
    let mut stray_plans = Vec::new();
    for plan in tickets.plans {
        attach(plan, &mut stories, &mut stray_plans, &mut findings);
    }
    Joined {
        stories,
        stray_plans,
        findings,
    }
}

fn file_story(folder: &str, leaf: LeafFile) -> Story {
    Story {
        kind: leaf.front.kind,
        id: leaf.front.id.clone(),
        entry: None,
        file: Some(leaf),
        plan: None,
        order: 0,
        folder: folder.to_owned(),
    }
}

/// Numeric ids by value first, then text ids by text (the PO's sort).
fn id_order(a: Option<&TicketId>, b: Option<&TicketId>) -> Ordering {
    let key = |id: Option<&TicketId>| {
        let text = id.map_or("", TicketId::as_str);
        (
            text.parse::<u64>().map_or((1, 0), |n| (0, n)),
            text.to_owned(),
        )
    };
    key(a).cmp(&key(b))
}

/// Attaches `plan` to the story its `ticket` names: an id, else a leaf file's stem.
fn attach(
    plan: PlanFile,
    stories: &mut [Story],
    stray: &mut Vec<PlanFile>,
    findings: &mut Vec<Finding>,
) {
    let ticket = plan.front.ticket.to_string();
    let by_id = stories
        .iter()
        .position(|s| s.id.as_ref().is_some_and(|id| id.as_str() == ticket));
    let found = by_id.or_else(|| {
        stories
            .iter()
            .position(|s| s.file.as_ref().is_some_and(|f| f.stem == ticket))
    });
    let Some(story) = found.and_then(|n| stories.get_mut(n)) else {
        stray.push(plan);
        return;
    };
    if let Some(first) = &story.plan {
        findings.push(Finding::error(
            Rule::DuplicateId,
            &plan.path,
            format!(
                "{} and {} are both plans for ticket {ticket}; a ticket has one plan",
                first.path, plan.path
            ),
        ));
    } else {
        story.plan = Some(plan);
    }
}

/// R-o: an entry that states `key`, `loom` or `needs` with another value than its file.
fn drift(folder: &str, entry: &EntryRow, leaf: &LeafFile) -> Vec<Finding> {
    let front: &LeafFront = &leaf.front;
    let mut out = Vec::new();
    let mut report = |field: &str, file: String, entry_value: String| {
        out.push(Finding::error(
            Rule::Drift,
            &leaf.path,
            format!(
                "{} and entry {} of {folder}/tickets.toml disagree on `{field}`: the file says \
                 {file}, the entry {entry_value}; the file counts, so change the entry's `{field}` \
                 or remove it",
                leaf.path, entry.id
            ),
        ));
    };
    if let Some(key) = &entry.key
        && front.key.as_ref() != Some(key)
    {
        report("key", show_key(front.key.as_ref()), key.to_string());
    }
    if let Some(mark) = entry.loom
        && front.loom != Some(mark)
    {
        report("loom", show_mark(front.loom), show_mark(Some(mark)));
    }
    if !entry.needs.is_empty() && entry.needs != front.needs {
        report("needs", show_keys(&front.needs), show_keys(&entry.needs));
    }
    out
}

fn show_key(key: Option<&Key>) -> String {
    key.map_or_else(|| "nothing".to_owned(), ToString::to_string)
}

fn show_mark(mark: Option<LoomMark>) -> String {
    match mark {
        Some(LoomMark::Ready) => "ready".to_owned(),
        Some(LoomMark::Paused) => "paused".to_owned(),
        None => "nothing".to_owned(),
    }
}

fn show_keys(keys: &[Key]) -> String {
    let list: Vec<String> = keys.iter().map(ToString::to_string).collect();
    format!("[{}]", list.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Option<TicketId> {
        TicketId::try_from(s.to_owned()).ok()
    }

    #[test]
    fn numeric_ids_sort_before_text_ids_and_by_value() {
        let mut ids = [id("b"), id("10"), id("2"), id("a")];
        ids.sort_by(|a, b| id_order(a.as_ref(), b.as_ref()));
        let text: Vec<&str> = ids.iter().flatten().map(TicketId::as_str).collect();
        assert_eq!(text, ["2", "10", "a", "b"]);
    }
}
