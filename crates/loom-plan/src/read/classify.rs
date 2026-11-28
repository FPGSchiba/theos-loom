//! What a repo-relative path is, from its place in the tree (spec §5.1) and the PO's names.
//!
//! The PO's `NAME_RE` and `EPIC_RE` (`tickets.py`) are matched with plain string operations: the
//! workspace has no regex crate, and `EPIC_RE`'s look-ahead is not supported by the `regex` crate
//! anyway. Both are a few lines and tested against the PO's own examples below.

use std::collections::BTreeSet;

use crate::model::DirectiveKind;

/// The ticket folder a leaf, plan or `tickets.toml` belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Container {
    /// The `tickets/` folder of an initiative.
    Initiative {
        /// The initiative's folder name.
        initiative: String,
    },
    /// The `tickets/` folder of an epic.
    Epic {
        /// The initiative's folder name.
        initiative: String,
        /// The epic's folder name.
        epic: String,
    },
    /// The component's `backlog/` folder.
    Backlog,
}

/// What a file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileKind {
    /// `loom.toml`.
    LoomToml,
    /// `vision.md`.
    Vision,
    /// `directives/project.md`.
    ProjectDirectives,
    /// `decisions/NNNN-<slug>.md`.
    Decision,
    /// `_state/<component>/<name>.toml`.
    StateMirror {
        /// The component folder name.
        component: String,
        /// The file stem: an epic key or `backlog`.
        name: String,
    },
    /// `components/<id>/component.toml`.
    ComponentToml {
        /// The component folder name.
        component: String,
    },
    /// `components/<id>/directives/{engineering,design}.md`.
    ComponentDirectives {
        /// The component folder name.
        component: String,
        /// Engineering or design.
        kind: DirectiveKind,
    },
    /// `components/<id>/<initiative>/<initiative>.md`.
    InitiativeFile {
        /// The component folder name.
        component: String,
        /// The initiative's folder name.
        initiative: String,
    },
    /// `components/<id>/<initiative>/tickets/<epic>/<epic>.md`.
    EpicFile {
        /// The component folder name.
        component: String,
        /// The initiative's folder name.
        initiative: String,
        /// The epic's folder name.
        epic: String,
    },
    /// A `tickets.toml` where the tree allows one.
    Breakdown {
        /// The component folder name.
        component: String,
        /// Whose breakdown it is.
        at: Container,
    },
    /// A leaf or plan file where the tree allows one.
    Ticket {
        /// The component folder name.
        component: String,
        /// The folder it sits in.
        at: Container,
    },
    /// A ticket-shaped path where the tree does not allow it (R-n); `report` is the path to name
    /// (the misplaced epic folder, or the file itself).
    Misplaced {
        /// The path a `layout` finding names.
        report: String,
    },
    /// Anything else (R-e).
    Document,
}

/// The PO's `NAME_RE`: `^(?:.+-)?(story|spike|bug)-(.+)\.md$`.
pub(crate) fn is_leaf_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    ["story-", "spike-", "bug-"].iter().any(|kind| {
        stem.match_indices(kind).any(|(at, _)| {
            let starts_ok = at == 0 || (at >= 2 && stem[..at].ends_with('-'));
            starts_ok && stem.len() > at + kind.len()
        })
    })
}

/// The PO's `EPIC_RE`: `^(?!(?:story|spike|bug)-)(?:.+-)?epic-[^/]+$`.
pub(crate) fn is_epic_name(name: &str) -> bool {
    if ["story-", "spike-", "bug-"]
        .iter()
        .any(|kind| name.starts_with(kind))
        || name.contains('/')
    {
        return false;
    }
    name.match_indices("epic-").any(|(at, _)| {
        let starts_ok = at == 0 || (at >= 2 && name[..at].ends_with('-'));
        starts_ok && name.len() > at + "epic-".len()
    })
}

/// An initiative folder: `initiative-<slug>`.
pub(crate) fn is_initiative_name(name: &str) -> bool {
    name.strip_prefix("initiative-")
        .is_some_and(|slug| !slug.is_empty())
}

/// Classifies the path given as its `/`-separated segments. `components` are the folders of
/// `components/` that hold a `component.toml` (R-c); any other folder there is no component.
pub(crate) fn classify(segments: &[&str], components: &BTreeSet<String>) -> FileKind {
    match segments {
        ["loom.toml"] => FileKind::LoomToml,
        ["vision.md"] => FileKind::Vision,
        ["directives", "project.md"] => FileKind::ProjectDirectives,
        ["decisions", name] if looks_like_decision(name) => FileKind::Decision,
        ["_state", component, name] if name.strip_suffix(".toml").is_some() => {
            FileKind::StateMirror {
                component: (*component).to_owned(),
                name: name.trim_end_matches(".toml").to_owned(),
            }
        }
        ["components", component, rest @ ..] if !rest.is_empty() => {
            if components.contains(*component) {
                in_component(component, rest)
            } else {
                misplaced_or_document(component, rest, false)
            }
        }
        _ => FileKind::Document,
    }
}

/// A decision record's name starts with four digits and a dash; the record model checks the rest.
fn looks_like_decision(name: &str) -> bool {
    let b = name.as_bytes();
    name.strip_suffix(".md").is_some()
        && b.len() > 5
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
}

fn in_component(component: &str, rest: &[&str]) -> FileKind {
    let component = component.to_owned();
    let initiative = |i: &str| Container::Initiative {
        initiative: i.to_owned(),
    };
    let epic_at = |i: &str, e: &str| Container::Epic {
        initiative: i.to_owned(),
        epic: e.to_owned(),
    };
    match rest {
        ["component.toml"] => FileKind::ComponentToml { component },
        ["directives", "engineering.md"] => FileKind::ComponentDirectives {
            component,
            kind: DirectiveKind::Engineering,
        },
        ["directives", "design.md"] => FileKind::ComponentDirectives {
            component,
            kind: DirectiveKind::Design,
        },
        [i, file] if is_initiative_name(i) && is_own_file(i, file) => FileKind::InitiativeFile {
            component,
            initiative: (*i).to_owned(),
        },
        [i, "tickets", "tickets.toml"] if is_initiative_name(i) => FileKind::Breakdown {
            component,
            at: initiative(i),
        },
        [i, "tickets", name] if is_initiative_name(i) && is_leaf_name(name) => FileKind::Ticket {
            component,
            at: initiative(i),
        },
        [i, "tickets", e, file]
            if is_initiative_name(i) && is_epic_name(e) && is_own_file(e, file) =>
        {
            FileKind::EpicFile {
                component,
                initiative: (*i).to_owned(),
                epic: (*e).to_owned(),
            }
        }
        [i, "tickets", e, "tickets", "tickets.toml"]
            if is_initiative_name(i) && is_epic_name(e) =>
        {
            FileKind::Breakdown {
                component,
                at: epic_at(i, e),
            }
        }
        [i, "tickets", e, "tickets", name]
            if is_initiative_name(i) && is_epic_name(e) && is_leaf_name(name) =>
        {
            FileKind::Ticket {
                component,
                at: epic_at(i, e),
            }
        }
        ["backlog", name] if is_leaf_name(name) => FileKind::Ticket {
            component,
            at: Container::Backlog,
        },
        _ => misplaced_or_document(&component, rest, true),
    }
}

fn is_own_file(folder: &str, file: &str) -> bool {
    file.strip_suffix(".md") == Some(folder)
}

/// A ticket-shaped path that no arm above accepted is misplaced; anything else is a document.
/// `in_tree` is false for a folder of `components/` that is no component: there no place
/// allows an epic folder.
fn misplaced_or_document(component: &str, rest: &[&str], in_tree: bool) -> FileKind {
    let Some((last, folders)) = rest.split_last() else {
        return FileKind::Document;
    };
    for (n, folder) in folders.iter().enumerate() {
        let allowed = in_tree && n == 2 && is_initiative_name(rest[0]) && rest[1] == "tickets";
        if is_epic_name(folder) && !allowed {
            return FileKind::Misplaced {
                report: format!("components/{component}/{}", rest[..=n].join("/")),
            };
        }
    }
    if is_leaf_name(last) || *last == "tickets.toml" {
        return FileKind::Misplaced {
            report: format!("components/{component}/{}", rest.join("/")),
        };
    }
    FileKind::Document
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> FileKind {
        let segments: Vec<&str> = path.split('/').collect();
        classify(&segments, &BTreeSet::from(["app".to_owned()]))
    }

    #[test]
    fn leaf_names_follow_name_re() {
        for good in [
            "story-cart.md",
            "spike-x.md",
            "bug-a-b.md",
            "SHOP-7-story-cart.md",
            "story-cart-plan.md",
        ] {
            assert!(is_leaf_name(good), "{good}");
        }
        for bad in [
            "story-.md",
            "story-cart.txt",
            "mystory-cart.md",
            "epic-cart.md",
            "-story-x.md",
        ] {
            assert!(!is_leaf_name(bad), "{bad}");
        }
    }

    #[test]
    fn epic_names_follow_epic_re() {
        for good in ["epic-cart", "SHOP-3-epic-cart", "epic-a-b"] {
            assert!(is_epic_name(good), "{good}");
        }
        for bad in [
            "epic-",
            "story-epic-cart",
            "bug-epic-x",
            "myepic-cart",
            "cart",
        ] {
            assert!(!is_epic_name(bad), "{bad}");
        }
    }

    #[test]
    fn paths_classify_by_their_place() {
        let c = "components/app";
        assert_eq!(kind("loom.toml"), FileKind::LoomToml);
        assert_eq!(kind("decisions/0001-a.md"), FileKind::Decision);
        assert_eq!(kind("decisions/README.md"), FileKind::Document);
        assert!(matches!(
            kind(&format!(
                "{c}/initiative-x/tickets/epic-a/tickets/story-b.md"
            )),
            FileKind::Ticket {
                at: Container::Epic { .. },
                ..
            }
        ));
        assert!(matches!(
            kind(&format!("{c}/backlog/bug-b.md")),
            FileKind::Ticket {
                at: Container::Backlog,
                ..
            }
        ));
        assert_eq!(
            kind(&format!("{c}/initiative-x/tickets/epic-a/notes.md")),
            FileKind::Document
        );
        assert_eq!(kind("canon/story-old.md"), FileKind::Document);
    }

    #[test]
    fn ticket_shapes_elsewhere_in_a_component_are_misplaced() {
        let c = "components/app";
        let misplaced = |path: &str, report: &str| {
            assert_eq!(
                kind(&format!("{c}/{path}")),
                FileKind::Misplaced {
                    report: format!("{c}/{report}")
                },
                "{path}"
            );
        };
        misplaced("initiative-x/story-a.md", "initiative-x/story-a.md");
        misplaced("initiative-x/tickets.toml", "initiative-x/tickets.toml");
        misplaced("initiative-x/epic-a/epic-a.md", "initiative-x/epic-a");
        misplaced(
            "initiative-x/tickets/epic-a/tickets/epic-b/epic-b.md",
            "initiative-x/tickets/epic-a/tickets/epic-b",
        );
        misplaced("backlog/tickets.toml", "backlog/tickets.toml");
        misplaced("notes/story-lost.md", "notes/story-lost.md");
    }
}
