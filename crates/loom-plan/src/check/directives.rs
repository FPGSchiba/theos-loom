//! Directive rules (spec §5.4, §5.6, decision 0023): `unnamed-directive`, `duplicate-id` for two
//! directives with one name in a file, `directive-contradiction` for a `Contradicts:` mark, and
//! `broken-reference` for a mark that names no project directive.
//!
//! Names match exactly, case included: a directive is identified by its name (decision 0023).
//! Both sides are read without markup and trailing full stop.

use std::collections::BTreeSet;

use super::Context;
use crate::finding::{Finding, Rule};
use crate::model::{Directive, DirectiveFile};

/// Every directive finding in the repo.
pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding> {
    let project = cx.repo.project_directives.as_ref();
    let project_names: BTreeSet<&str> = project
        .into_iter()
        .flat_map(|f| &f.directives)
        .filter_map(|d| d.name.as_deref())
        .collect();
    let mut out = Vec::new();
    for file in project.into_iter().chain(component_files(cx)) {
        names(file, &mut out);
    }
    if let Some(file) = project {
        for d in file.directives.iter().filter(|d| d.contradicts.is_some()) {
            out.push(at(
                Finding::error(
                    Rule::DirectiveContradiction,
                    &file.path,
                    format!(
                        "{} carries a `Contradicts:` mark; only a component directive may, \
                         change the directive or remove the mark",
                        label(d)
                    ),
                ),
                d,
            ));
        }
    }
    for file in component_files(cx) {
        marks(cx, file, &project_names, &mut out);
    }
    out
}

fn component_files<'a>(cx: &'a Context<'_>) -> impl Iterator<Item = &'a DirectiveFile> {
    cx.repo
        .components
        .iter()
        .flat_map(|c| [&c.engineering, &c.design])
        .flatten()
}

fn at(finding: Finding, directive: &Directive) -> Finding {
    Finding {
        line: Some(directive.line),
        ..finding
    }
}

/// How a directive is named in a message.
fn label(directive: &Directive) -> String {
    directive.name.as_ref().map_or_else(
        || "an unnamed directive".to_owned(),
        |n| format!("the directive `{n}`"),
    )
}

/// Unnamed directives, and every name used twice in one file.
fn names(file: &DirectiveFile, out: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    for d in &file.directives {
        match d.name.as_deref() {
            None => out.push(at(
                Finding::error(
                    Rule::UnnamedDirective,
                    &file.path,
                    "a directive has no bold name; start the item with `**Name.**` so it can be \
                     referred to"
                        .to_owned(),
                ),
                d,
            )),
            Some(name) if !seen.insert(name) => out.push(at(
                Finding::error(
                    Rule::DuplicateId,
                    &file.path,
                    format!(
                        "the directive name `{name}` is used twice in this file; names identify \
                         directives, give this one another"
                    ),
                ),
                d,
            )),
            Some(_) => {}
        }
    }
}

/// The `Contradicts:` marks of a component directive file.
fn marks(
    cx: &Context<'_>,
    file: &DirectiveFile,
    project_names: &BTreeSet<&str>,
    out: &mut Vec<Finding>,
) {
    let project_path = cx
        .repo
        .project_directives
        .as_ref()
        .map_or("directives/project.md", |f| f.path.as_str());
    for d in &file.directives {
        let Some(target) = d.contradicts.as_deref() else {
            continue;
        };
        let finding = if project_names.contains(target) {
            Finding::error(
                Rule::DirectiveContradiction,
                &file.path,
                format!(
                    "{} in {} contradicts the project directive `{target}` in {project_path}; \
                     change one of them, or make it an exception in the project file",
                    label(d),
                    file.path
                ),
            )
        } else {
            Finding::error(
                Rule::BrokenReference,
                &file.path,
                format!(
                    "{} says it contradicts `{target}`, but {project_path} has no directive with \
                     that name",
                    label(d)
                ),
            )
        };
        out.push(at(finding, d));
    }
}
