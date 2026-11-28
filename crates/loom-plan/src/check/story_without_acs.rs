//! `story-without-acs`: a story, spike or bug, file or entry only, without numbered criteria
//! (spec §5.4). An error when the story or its epic is ready, else a note.

use super::Context;
use super::readiness::{Item, finding, is_ready};
use crate::finding::{Finding, Rule};

/// Every story without criteria.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    cx.index
        .stories
        .iter()
        .filter(|at| !at.story.is_dropped() && at.story.criteria().is_empty())
        .map(|at| {
            let story = at.story;
            let (what, add) = if story.file.is_some() {
                (
                    "has no numbered criteria",
                    "add them under its Requirements or Acceptance Criteria heading",
                )
            } else {
                (
                    "has only a tickets.toml entry and no criteria",
                    "pull its file and add numbered criteria",
                )
            };
            finding(
                Rule::StoryWithoutAcs,
                is_ready(Item::Story(at)),
                &story.path(),
                format!("the story \"{}\" {what}: {add}", story.title()),
            )
        })
        .collect()
}
