//! `missing-key`: an epic or story without `key`, so its criteria have no ids (spec §5.4). An
//! error when the item is ready, else a note. A keyless story is reported here and nowhere else.
//! An epic without stories is never a finding (TU-01.2).

use super::Context;
use super::readiness::{Item, finding, is_ready};
use crate::finding::{Finding, Rule};

/// Every epic and story without a key.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for at in &cx.index.epics {
        let epic = at.epic;
        let Some(file) = &epic.file else { continue };
        if epic.key().is_none() && !epic.stories.is_empty() && !epic.is_dropped() {
            out.push(finding(
                Rule::MissingKey,
                is_ready(cx, Item::Epic(epic)),
                &file.path,
                "the epic has no `key`: add `key: <COMPONENT>-<n>` to its front matter".to_owned(),
            ));
        }
    }
    for at in &cx.index.stories {
        let story = at.story;
        if story.key().is_none() && !story.is_dropped() {
            let ids = if story.criteria().is_empty() {
                ""
            } else {
                ", so its criteria have no ids"
            };
            out.push(finding(
                Rule::MissingKey,
                is_ready(cx, Item::Story(at)),
                &story.path(),
                format!(
                    "the story \"{}\" has no `key`{ids}: add `key: <COMPONENT>-<n>`",
                    story.title()
                ),
            ));
        }
    }
    out
}
