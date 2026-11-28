//! Behaviour tests of `directives_for` and the directive rules of `check`.

use std::path::{Path, PathBuf};

use loom_plan::{Finding, Layer, Rule, Severity, check, read};

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

// AC: TU-01.3
#[test]
fn directives_resolve_project_first_then_component() -> TestResult {
    let repo = read(&fixture("valid"))?;
    let all = repo.directives_for("app");
    let got: Vec<(Layer, Option<&str>)> =
        all.iter().map(|d| (d.layer, d.name.as_deref())).collect();
    assert_eq!(
        got,
        [
            (Layer::Project, Some("Small modules")),
            (Layer::Project, Some("Tests first")),
            (Layer::Project, Some("Plain language")),
            (Layer::Component, Some("Money is typed")),
            (Layer::Component, Some("One primary button")),
        ]
    );
    assert_eq!(all[0].path, "directives/project.md");
    assert_eq!(all[0].line, 5);
    assert_eq!(repo.directives_for("no-such-component"), Vec::new());
    Ok(())
}

// AC: TU-01.3
#[test]
fn marked_contradiction_is_reported_with_both_directives() -> TestResult {
    let f = only_error("directive-contradiction-marked")?;
    assert_eq!(f.rule, Rule::DirectiveContradiction, "{f:#?}");
    for part in [
        "Big modules",
        "Small modules",
        "components/app/directives/engineering.md",
        "directives/project.md",
    ] {
        assert!(f.message.contains(part), "{part:?} missing from {f:#?}");
    }
    assert_eq!(f.path, "components/app/directives/engineering.md");
    assert_eq!(f.line, Some(3));
    Ok(())
}

// AC: TU-01.3
#[test]
fn mark_naming_no_project_directive_is_a_broken_reference() -> TestResult {
    let f = only_error("broken-reference-contradicts-unknown")?;
    assert_eq!(f.rule, Rule::BrokenReference, "{f:#?}");
    assert!(f.message.contains("Fast builds"), "{f:#?}");
    Ok(())
}

// AC: TU-01.3
#[test]
fn contradicted_directive_is_still_returned_flagged() -> TestResult {
    let repo = read(&fixture("invalid/directive-contradiction-marked"))?;
    let all = repo.directives_for("app");
    let names: Vec<_> = all.iter().map(|d| d.name.as_deref()).collect();
    assert_eq!(
        names,
        [
            Some("Small modules"),
            Some("Tests first"),
            Some("Big modules")
        ]
    );
    assert_eq!(all[2].layer, Layer::Component);
    assert_eq!(all[2].contradicts.as_deref(), Some("Small modules"));
    assert_eq!(all[0].contradicts, None);
    Ok(())
}

// AC: TU-01.3
#[test]
fn mark_in_project_file_is_an_error() -> TestResult {
    let f = only_error("directive-contradiction-in-project")?;
    assert_eq!(f.rule, Rule::DirectiveContradiction, "{f:#?}");
    assert_eq!(f.path, "directives/project.md");
    Ok(())
}

// AC: TU-01.3
#[test]
fn unnamed_directive_is_an_error() -> TestResult {
    let f = only_error("unnamed-directive-plain")?;
    assert_eq!(f.rule, Rule::UnnamedDirective, "{f:#?}");
    assert_eq!(f.path, "components/app/directives/engineering.md");
    assert_eq!(f.line, Some(4));
    Ok(())
}

// AC: TU-01.2
#[test]
fn duplicate_directive_name_is_an_error() -> TestResult {
    let f = only_error("duplicate-id-directive-name")?;
    assert_eq!(f.rule, Rule::DuplicateId, "{f:#?}");
    assert!(f.message.contains("Money is typed"), "{f:#?}");
    assert_eq!(f.line, Some(4));
    Ok(())
}
