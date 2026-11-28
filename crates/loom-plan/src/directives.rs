//! Directives resolved by layer (spec §5.6): the project's first, then the component's.

use serde::Serialize;

use crate::model::{DirectiveFile, DirectiveKind};
use crate::repo::PlanningRepo;

/// Where a directive comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// `directives/project.md`.
    Project,
    /// A component's `directives/engineering.md` or `design.md`.
    Component,
}

/// A directive with its layer and place, as a run's briefing shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedDirective {
    /// Project or component.
    pub layer: Layer,
    /// Engineering or design.
    pub kind: DirectiveKind,
    /// The bold name, when the directive has one.
    pub name: Option<String>,
    /// The text of the item, marker removed.
    pub text: String,
    /// Repo-relative path of the file it is in.
    pub path: String,
    /// 1-based line of the item in that file.
    pub line: u32,
    /// The project directive it is marked as contradicting; `check` reports it as an error but
    /// the directive is still returned.
    pub contradicts: Option<String>,
}

impl PlanningRepo {
    /// The directives that apply to `component`: project engineering, project design, component
    /// engineering, then component design, each in file order. A contradicted directive is
    /// returned with `contradicts` set. A component id that does not exist gives an empty list.
    #[must_use]
    pub fn directives_for(&self, component: &str) -> Vec<ResolvedDirective> {
        let mut out = Vec::new();
        let Some(found) = self.components.iter().find(|c| c.id == component) else {
            return out;
        };
        if let Some(file) = &self.project_directives {
            for kind in [DirectiveKind::Engineering, DirectiveKind::Design] {
                push(&mut out, Layer::Project, file, Some(kind));
            }
        }
        for file in [&found.engineering, &found.design].into_iter().flatten() {
            push(&mut out, Layer::Component, file, None);
        }
        out
    }
}

/// Adds the directives of `file` (only those of `kind` when given).
fn push(
    out: &mut Vec<ResolvedDirective>,
    layer: Layer,
    file: &DirectiveFile,
    kind: Option<DirectiveKind>,
) {
    out.extend(
        file.directives
            .iter()
            .filter(|d| kind.is_none_or(|k| d.kind == k))
            .map(|d| ResolvedDirective {
                layer,
                kind: d.kind,
                name: d.name.clone(),
                text: d.text.clone(),
                path: file.path.clone(),
                line: d.line,
                contradicts: d.contradicts.clone(),
            }),
    );
}
