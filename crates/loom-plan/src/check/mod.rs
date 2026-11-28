//! `check`: every rule of spec §5.4 over a read planning repo.
//!
//! A rule is one module with a `check(&Context) -> Vec<Finding>` function and one line in
//! [`RULES`].

mod context;
mod cycles;
mod directives;
mod duplicates;
mod epic_incomplete;
mod missing_key;
mod readiness;
mod references;
mod story_without_acs;

use context::Context;

use crate::finding::Finding;
use crate::repo::PlanningRepo;

/// A check rule: every finding of its kind in the repo.
pub(crate) type RuleFn = fn(&Context<'_>) -> Vec<Finding>;

/// Every rule `check` runs.
pub(crate) const RULES: &[RuleFn] = &[
    references::check,
    duplicates::check,
    cycles::check,
    epic_incomplete::check,
    story_without_acs::check,
    missing_key::check,
    directives::check,
];

/// Checks `repo`: what reading found plus every rule's findings, errors before notes, then by
/// path and line.
#[must_use]
pub fn check(repo: &PlanningRepo) -> Vec<Finding> {
    let context = Context::new(repo);
    let mut findings = repo.read_findings.clone();
    for rule in RULES {
        findings.extend(rule(&context));
    }
    findings.sort_by(|a, b| (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line)));
    findings
}
