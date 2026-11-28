//! What every rule gets: the repo and the lookups built once from it.

use crate::graph::DependencyGraph;
use crate::index::Index;
use crate::repo::PlanningRepo;

/// The shared inputs of the check rules, built once per `check`.
pub(crate) struct Context<'a> {
    /// The repo being checked.
    pub repo: &'a PlanningRepo,
    /// Its lookups.
    pub index: Index<'a>,
    /// The dependency graph of its epics and stories.
    pub graph: DependencyGraph,
}

impl<'a> Context<'a> {
    /// Builds the context of `repo`.
    pub(crate) fn new(repo: &'a PlanningRepo) -> Context<'a> {
        let index = Index::build(repo);
        let graph = DependencyGraph::from_index(&index);
        Context { repo, index, graph }
    }
}
