//! `cycle`: a cycle in the dependency graph (spec §5.5), naming every member.

use super::Context;
use crate::finding::{Finding, Rule};
use crate::graph::NodeId;

/// One finding per cycle of `cx.graph`.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    cx.graph
        .cycles()
        .iter()
        .filter_map(|members| finding(cx, members))
        .collect()
}

fn finding(cx: &Context<'_>, members: &[NodeId]) -> Option<Finding> {
    let first = members.first()?;
    let path = cx.graph.path_of(first)?;
    let names: Vec<String> = members.iter().map(|m| format!("`{m}`")).collect();
    let message = if let [only] = names.as_slice() {
        format!("{only} waits for itself; remove it from its own `after` or `needs`")
    } else {
        format!(
            "these items wait for each other in a cycle: {}; remove one `after` or `needs` to \
             break it",
            names.join(", ")
        )
    };
    Some(Finding::error(Rule::Cycle, path, message))
}
