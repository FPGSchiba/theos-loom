//! `epic-incomplete`: an epic without goal, scope or requirement line, saying which (spec §5.4).
//! An error when the epic is ready, else a note. Having no stories is never a finding (TU-01.2),
//! but an epic without stories is checked like any other.

use super::Context;
use super::readiness::{Item, finding, is_ready};
use crate::finding::{Finding, Rule};

/// Every incomplete epic, with or without stories.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for at in &cx.index.epics {
        let epic = at.epic;
        if epic.is_dropped() {
            continue;
        }
        let ready = is_ready(Item::Epic(epic));
        let (Some(file), Some(body)) = (&epic.file, &epic.body) else {
            out.push(finding(
                Rule::EpicIncomplete,
                false,
                &epic.path,
                "the epic has no file yet: pull it so it can get a goal, scope and requirement \
                 lines"
                    .to_owned(),
            ));
            continue;
        };
        let checks = [
            (
                body.goal_present,
                "has no goal: add text under `## Description` and `## Outcome`",
            ),
            (
                body.scope_present,
                "has no Boundaries section text: add its scope under `## Boundaries`",
            ),
            (
                !body.requirements.is_empty(),
                "has no requirement line: add numbered lines `R1: ...` under `## Requirements`",
            ),
        ];
        for (present, message) in checks {
            if !present {
                out.push(finding(
                    Rule::EpicIncomplete,
                    ready,
                    &file.path,
                    format!("the epic {message}"),
                ));
            }
        }
    }
    out
}
