//! Behaviour tests of the dependency graph and the `cycle` rule.

use std::path::{Path, PathBuf};

use loom_plan::graph::{DependencyGraph, NodeId};
use loom_plan::{Finding, Rule, Severity, check, read};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The one error `check` reports on `invalid/<name>`; fails when there are more or none.
fn only_error(name: &str) -> Result<Finding, Box<dyn std::error::Error>> {
    let repo = read(&fixture(&format!("invalid/{name}")))?;
    let mut errors: Vec<Finding> = check(&repo)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{name}: {errors:#?}");
    errors.pop().ok_or_else(|| "no error".into())
}

/// Asserts `finding` is a `cycle` finding whose message names every one of `members`.
fn assert_cycle(finding: &Finding, members: &[&str]) {
    assert_eq!(finding.rule, Rule::Cycle, "{finding:#?}");
    for member in members {
        assert!(
            finding.message.contains(member),
            "{member:?} missing from {finding:#?}"
        );
    }
}

// AC: TU-01.2
#[test]
fn cycle_through_after_is_reported() -> TestResult {
    let finding = only_error("cycle-after")?;
    assert_cycle(&finding, &["APP-2", "APP-3"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn cycle_through_needs_across_components_is_reported() -> TestResult {
    let finding = only_error("cycle-needs")?;
    assert_cycle(&finding, &["APP-2", "API-2"]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn cycle_through_an_epics_prerequisites_is_reported() -> TestResult {
    // Epic A waits for epic B, a story of B waits for a story of A, and that story cannot start
    // before epic A may, that is before epic B is done.
    let finding = only_error("cycle-through-epic")?;
    assert_cycle(&finding, &["APP-2", "APP-3", "APP-4"]);
    Ok(())
}

#[test]
fn cycle_message_is_stable_and_graph_lists_the_cycle_once() -> TestResult {
    let repo = read(&fixture("invalid/cycle-needs"))?;
    let first = only_error("cycle-needs")?;
    let second = only_error("cycle-needs")?;
    assert_eq!(first.message, second.message);
    let cycles = DependencyGraph::of(&repo).cycles();
    assert_eq!(cycles, [vec![NodeId::new("API-2"), NodeId::new("APP-2")]]);
    Ok(())
}

// AC: TU-01.2
#[test]
fn dropped_prerequisite_still_blocks_but_is_no_cycle() -> TestResult {
    let repo = read(&fixture("cases/dropped-prerequisite-still-blocks"))?;
    let errors: Vec<Finding> = check(&repo)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert_eq!(errors, []);
    let graph = DependencyGraph::of(&repo);
    assert_eq!(graph.cycles(), Vec::<Vec<NodeId>>::new());
    // APP-2 is dropped; APP-3 still waits for it (R-r).
    assert!(
        graph
            .waits_for(&NodeId::new("APP-3"))
            .contains(&NodeId::new("APP-2"))
    );
    Ok(())
}

#[test]
fn epics_wait_for_their_stories_and_stories_for_their_epics_prerequisites() -> TestResult {
    let repo = read(&fixture("invalid/cycle-through-epic"))?;
    let graph = DependencyGraph::of(&repo);
    assert_eq!(
        graph.waits_for(&NodeId::new("APP-1")),
        [NodeId::new("APP-2"), NodeId::new("APP-3")]
    );
    // APP-3 sits in epic A, which waits for epic B (APP-2).
    assert_eq!(
        graph.waits_for(&NodeId::new("APP-3")),
        [NodeId::new("APP-2")]
    );
    Ok(())
}

#[test]
fn valid_tree_has_no_cycles() -> TestResult {
    let repo = read(&fixture("valid"))?;
    assert_eq!(
        DependencyGraph::of(&repo).cycles(),
        Vec::<Vec<NodeId>>::new()
    );
    Ok(())
}
