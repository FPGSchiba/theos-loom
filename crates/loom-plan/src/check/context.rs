//! What every rule gets: the repo and the lookups built once from it.

use crate::index::Index;
use crate::repo::PlanningRepo;

/// The shared inputs of the check rules, built once per `check`.
pub(crate) struct Context<'a> {
    /// The repo being checked.
    pub repo: &'a PlanningRepo,
    /// Its lookups.
    pub index: Index<'a>,
}

impl<'a> Context<'a> {
    /// Builds the context of `repo`.
    pub(crate) fn new(repo: &'a PlanningRepo) -> Context<'a> {
        Context {
            repo,
            index: Index::build(repo),
        }
    }
}
