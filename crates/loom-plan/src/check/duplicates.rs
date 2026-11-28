//! `duplicate-id`: two table rows with one id in a `tickets.toml`, two items with one `key` in
//! the project, two criteria or requirement lines with one number in a file, two decision
//! records with one number (spec §5.4). Two directives with one name is the directives rule's;
//! two plans for one ticket is found while reading.

use std::collections::BTreeMap;
use std::fmt::Display;

use super::Context;
use crate::finding::{Finding, Rule};
use crate::model::Breakdown;

/// Every duplicate id in the repo.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    keys(cx, &mut out);
    breakdowns(cx, &mut out);
    numbers(cx, &mut out);
    decisions(cx, &mut out);
    out
}

fn duplicate(path: &str, line: Option<u32>, message: String) -> Finding {
    Finding {
        line,
        ..Finding::error(Rule::DuplicateId, path, message)
    }
}

/// Each item after the first that carries a key.
fn keys(cx: &Context<'_>, out: &mut Vec<Finding>) {
    for (key, items) in &cx.index.keys {
        let Some((first, rest)) = items.split_first() else {
            continue;
        };
        out.extend(rest.iter().map(|item| {
            duplicate(
                &item.path,
                None,
                format!(
                    "the key `{key}` is already used by {}; a key names one epic or story in \
                     the project, give this one another",
                    first.path
                ),
            )
        }));
    }
}

/// Each `[[epic]]`, `[[entry]]` or `[[initiative]]` table after the first with an id.
fn breakdowns(cx: &Context<'_>, out: &mut Vec<Finding>) {
    let initiatives = cx
        .index
        .initiatives
        .iter()
        .map(|at| (&at.initiative.path, &at.initiative.breakdown));
    let epics = cx
        .index
        .epics
        .iter()
        .map(|at| (&at.epic.path, &at.epic.breakdown));
    for (container, breakdown) in initiatives.chain(epics) {
        if let Some(breakdown) = breakdown {
            breakdown_ids(&format!("{container}/tickets/tickets.toml"), breakdown, out);
        }
    }
}

fn breakdown_ids(path: &str, breakdown: &Breakdown, out: &mut Vec<Finding>) {
    let tables = [
        (
            "epic",
            breakdown.epic.iter().map(|r| &r.id).collect::<Vec<_>>(),
        ),
        ("entry", breakdown.entry.iter().map(|r| &r.id).collect()),
        (
            "initiative",
            breakdown.initiative.iter().map(|r| &r.id).collect(),
        ),
    ];
    for (table, ids) in tables {
        for (id, _) in repeats(ids.into_iter().map(|id| (id, ()))) {
            out.push(duplicate(
                path,
                None,
                format!(
                    "two [[{table}]] tables have the id `{id}`; an id is unique in its \
                     tickets.toml, give one of them the file's `next_id`"
                ),
            ));
        }
    }
}

/// Criteria numbers of story files and `R<n>` numbers of epic and initiative files.
fn numbers(cx: &Context<'_>, out: &mut Vec<Finding>) {
    for at in &cx.index.stories {
        if let Some(file) = &at.story.file {
            let found = file.criteria.iter().map(|c| (c.number, c.line));
            report_numbers(&file.path, "criterion", "", found, out);
        }
    }
    for at in &cx.index.epics {
        if let (Some(file), Some(body)) = (&at.epic.file, &at.epic.body) {
            let found = body.requirements.iter().map(|r| (r.number, r.line));
            report_numbers(&file.path, "requirement line", "R", found, out);
        }
    }
    for at in &cx.index.initiatives {
        if let Some(file) = &at.initiative.file {
            let found = file.requirements.iter().map(|r| (r.number, r.line));
            report_numbers(&file.path, "requirement line", "R", found, out);
        }
    }
}

fn report_numbers(
    path: &str,
    what: &str,
    prefix: &str,
    found: impl Iterator<Item = (u32, u32)>,
    out: &mut Vec<Finding>,
) {
    for (number, (first, line)) in repeats(found) {
        out.push(duplicate(
            path,
            Some(line),
            format!(
                "the {what} {prefix}{number} on line {line} has the same number as the one on \
                 line {first}; number each {what} once"
            ),
        ));
    }
}

/// Each decision record after the first with a number.
fn decisions(cx: &Context<'_>, out: &mut Vec<Finding>) {
    let found = cx.repo.decisions.iter().map(|d| (d.number, d));
    for (number, (first, again)) in repeats(found) {
        out.push(duplicate(
            &again.path,
            None,
            format!(
                "decision number {number:04} is already used by {}; give this record the next \
                 free number",
                first.path
            ),
        ));
    }
}

/// Each repeated id, as `(id, (what the first had, what this one has))`, in input order.
fn repeats<K: Ord + Display, V: Copy>(items: impl Iterator<Item = (K, V)>) -> Vec<(K, (V, V))> {
    let mut seen: BTreeMap<K, V> = BTreeMap::new();
    let mut out = Vec::new();
    for (id, value) in items {
        match seen.get(&id) {
            Some(first) => out.push((id, (*first, value))),
            None => {
                seen.insert(id, value);
            }
        }
    }
    out
}
