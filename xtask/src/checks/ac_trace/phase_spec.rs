//! The criteria a phase must prove, from the phase's own spec when there is one.
//!
//! A phase spec (`docs/specs/phases/PNN-<slug>.md`) lists in its section 2 the criteria the phase
//! builds. That list may be narrower than the umbrella's (the umbrella's criteria that the phase
//! cannot build alone), so `ac-trace` demands what the spec lists. Without a phase spec the
//! umbrella decides. A spec that lists nothing, or a criterion of another phase, is an error:
//! a wrong scope must fail loudly, not demand less.

use std::fs;
use std::path::Path;

use crate::branch::Phase;
use crate::files::extension;
use crate::markdown;
use crate::umbrella::{self, Umbrella};
use crate::{Error, Result};

/// The folder of the phase specs, relative to the repository root.
pub const PHASES_DIR: &str = "docs/specs/phases";

/// Where the criteria a phase must prove come from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Scope {
    /// The phase's entry in the umbrella spec.
    Umbrella,
    /// The list in section 2 of the phase spec `file`.
    PhaseSpec {
        /// The spec, repo-relative with `/`.
        file: String,
        /// The criteria it lists, in the order written, each once.
        acs: Vec<String>,
    },
}

/// The scope of `phase` in the repository at `root`.
///
/// # Errors
///
/// Fails when the folder or a spec cannot be read, with [`Error::Config`] when `phase` has more
/// than one spec, a spec lists no criterion in its section 2, or lists one that does not belong
/// to `phase` in the umbrella.
pub fn scope(root: &Path, umbrella: &Umbrella, phase: Phase) -> Result<Scope> {
    let specs = specs_of(root, phase)?;
    let file = match specs.as_slice() {
        [] => return Ok(Scope::Umbrella),
        [one] => one.clone(),
        many => {
            return Err(Error::Config {
                file: PHASES_DIR.to_owned(),
                message: format!("{phase} has more than one spec: {}", many.join(", ")),
            });
        }
    };
    let path = root.join(&file);
    let text = fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
    let acs = criteria_of(&text);
    if acs.is_empty() {
        return Err(Error::Config {
            file,
            message: "section 2 (`## 2. …`) lists no acceptance criterion (`- TU-NN.N …`)"
                .to_owned(),
        });
    }
    let own = umbrella.phase(phase).map(|info| &info.acs);
    for id in &acs {
        if !own.is_some_and(|own| own.contains(id)) {
            return Err(Error::Config {
                file,
                message: format!(
                    "section 2 lists {id}, which is not an acceptance criterion of {phase} in \
                     the umbrella spec ({})",
                    umbrella::PATH
                ),
            });
        }
    }
    Ok(Scope::PhaseSpec { file, acs })
}

/// The repo-relative paths of the files `PHASES_DIR/PNN-*.md`, sorted.
fn specs_of(root: &Path, phase: Phase) -> Result<Vec<String>> {
    let dir = root.join(PHASES_DIR);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Io { path: dir, source }),
    };
    let prefix = format!("{phase}-");
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && extension(&name) == Some("md") {
            found.push(format!("{PHASES_DIR}/{name}"));
        }
    }
    found.sort();
    Ok(found)
}

/// The ids that start a list item under a level-two heading starting `2.`, each once.
fn criteria_of(text: &str) -> Vec<String> {
    let mut acs: Vec<String> = Vec::new();
    for section in markdown::sections(text, 2)
        .iter()
        .filter(|section| section.heading.text.starts_with("2."))
    {
        for item in markdown::list_items(section.body) {
            let first = item.text.split_whitespace().next().unwrap_or_default();
            if umbrella::is_ac_id(first) && !acs.iter().any(|id| id == first) {
                acs.push(first.to_owned());
            }
        }
    }
    acs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::TestResult;

    const UMBRELLA: &str = "\
## 3. Phases

| Phase | Name | Slice | Depends on |
|---|---|---|---|
| P90 | Fixture phase | S2 | – |
| P91 | Other phase | S2 | – |

### P90 · Fixture phase

- TU-90.1 The first.
- TU-90.2 The second.
- TU-90.3 The third.

### P91 · Other phase

- TU-91.1 Another.
";

    const SPEC: &str = "\
# P90

## 1. Scope

- TU-90.3 is mentioned here, but this is not section 2.

## 2. Acceptance criteria

**TU-90 Fixtures**
- TU-90.1 The first,
  over two lines.
- TU-90.2 The second.
- not a criterion

Closing words.

## 3. Design
";

    fn umbrella() -> TestResult<Umbrella> {
        Ok(umbrella::parse(UMBRELLA)?)
    }

    fn repo(files: &[(&str, &str)]) -> TestResult<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        for (name, text) in files {
            let path = dir.path().join(PHASES_DIR).join(name);
            fs::create_dir_all(path.parent().ok_or("no parent")?)?;
            fs::write(path, text)?;
        }
        Ok(dir)
    }

    fn phase() -> TestResult<Phase> {
        Ok("P90".parse()?)
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_spec_lists_the_part_a_phase_builds() -> TestResult {
        let other = "## 2. Criteria\n- TU-91.1 x\n";
        let dir = repo(&[("P90-x.md", SPEC), ("P91-y.md", other)])?;
        let scope = scope(dir.path(), &umbrella()?, phase()?)?;
        assert_eq!(
            scope,
            Scope::PhaseSpec {
                file: "docs/specs/phases/P90-x.md".to_owned(),
                acs: vec!["TU-90.1".to_owned(), "TU-90.2".to_owned()],
            }
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn without_a_phase_spec_the_umbrella_decides() -> TestResult {
        let dir = repo(&[("P91-y.md", SPEC)])?;
        assert_eq!(scope(dir.path(), &umbrella()?, phase()?)?, Scope::Umbrella);
        let none = tempfile::tempdir()?;
        assert_eq!(scope(none.path(), &umbrella()?, phase()?)?, Scope::Umbrella);
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_spec_naming_a_criterion_of_another_phase_is_an_error() -> TestResult {
        let dir = repo(&[("P90-x.md", "## 2. Criteria\n- TU-90.1 a\n- TU-91.1 b\n")])?;
        let result = scope(dir.path(), &umbrella()?, phase()?);
        assert!(
            matches!(&result, Err(Error::Config { file, message })
                if file == "docs/specs/phases/P90-x.md" && message.contains("TU-91.1")),
            "{result:?}"
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn two_phase_specs_for_one_phase_are_an_error() -> TestResult {
        let dir = repo(&[("P90-x.md", SPEC), ("P90-y.md", SPEC)])?;
        let result = scope(dir.path(), &umbrella()?, phase()?);
        assert!(matches!(result, Err(Error::Config { .. })), "{result:?}");
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn a_phase_spec_without_a_criterion_in_section_2_is_an_error() -> TestResult {
        let text = "## 1. Scope\n- TU-90.1 not here\n\n## 2. Criteria\n- nothing\n";
        let dir = repo(&[("P90-x.md", text)])?;
        let result = scope(dir.path(), &umbrella()?, phase()?);
        assert!(
            matches!(&result, Err(Error::Config { file, .. }) if file.ends_with("P90-x.md")),
            "{result:?}"
        );
        Ok(())
    }

    // AC: TU-00.4
    #[test]
    fn p00_demands_its_four_criteria() -> TestResult {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let umbrella = umbrella::load(&root)?;
        let Scope::PhaseSpec { file, acs } = scope(&root, &umbrella, "P00".parse()?)? else {
            return Err("P00 has no phase spec".into());
        };
        assert!(file.starts_with("docs/specs/phases/P00-"), "{file}");
        assert_eq!(acs, ["TU-00.1", "TU-00.2", "TU-00.3", "TU-00.4"]);
        Ok(())
    }
}
