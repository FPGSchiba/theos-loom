//! Readiness: the one place that decides whether an epic or story is ready (decision 0021,
//! R-q, R-r), and the severity the unfinished-work rules give it (decision 0022).

use super::Context;
use crate::finding::{Finding, Rule};
use crate::index::{StoryAt, StoryHome};
use crate::model::LoomMark;
use crate::repo::Epic;

/// An item whose readiness is asked.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Item<'a> {
    /// An epic.
    Epic(&'a Epic),
    /// A story, spike or bug with its place.
    Story(&'a StoryAt<'a>),
}

/// True when Loom may work on `item`: it carries `loom: ready` itself, or its epic does, and it
/// is not dropped. A story outside an epic is ready only by its own mark (R-q). `paused` is not
/// ready.
pub(crate) fn is_ready(_cx: &Context<'_>, item: Item<'_>) -> bool {
    match item {
        Item::Epic(epic) => epic_is_ready(epic),
        Item::Story(at) => {
            let own = at.story.loom() == Some(LoomMark::Ready);
            let by_epic = matches!(at.home, StoryHome::Epic(_, epic) if epic_is_ready(epic));
            (own || by_epic) && !at.story.is_dropped()
        }
    }
}

fn epic_is_ready(epic: &Epic) -> bool {
    epic.loom() == Some(LoomMark::Ready) && !epic.is_dropped()
}

/// A finding of `rule`: an error when `ready`, else a note.
pub(crate) fn finding(rule: Rule, ready: bool, path: &str, message: String) -> Finding {
    if ready {
        Finding::error(rule, path, message)
    } else {
        Finding::note(rule, path, message)
    }
}
