//! Rule 4 of `lints`: the configuration files of the repository.
//!
//! Nothing outside `gate/` can weaken the lints, the tool configuration or the test run:
//!
//! - a `[workspace.lints]` table in the root `Cargo.toml` that is not `gate/lints.toml`, compared
//!   as parsed TOML ([`workspace_lints_match`]);
//! - a `Cargo.toml` below the root that has no `[lints]` with `workspace = true`, so that its
//!   crate would not get the workspace lints, and a root `Cargo.toml` that is a package and has
//!   none;
//! - a root `clippy.toml` or `.config/nextest.toml` that is not a copy of its original in `gate/`,
//!   comparing the text but not the line ends ([`same_text`]);
//! - a clippy or rustfmt configuration, or a `.cargo` folder, anywhere else (the root `.cargo`
//!   and `gate/clippy.toml` are the only exceptions), because clippy and cargo find these files
//!   by walking up from the crate;
//! - a file called `rust-toolchain` anywhere, which rustup reads instead of the pinned
//!   `rust-toolchain.toml`.
//!
//! The rule reads file names and TOML only, so it needs no workspace view ([`findings`]).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::{Inputs, Rule, at_file};
use crate::files::is_fixture;
use crate::verdict::Finding;
use crate::{Error, Result};

/// The rule, as `RULES` lists it.
pub const RULE: Rule = Rule {
    name: "config-files",
    run: run_rule,
};

/// The root manifest, which holds `[workspace.lints]`.
const ROOT_MANIFEST: &str = "Cargo.toml";

/// The lints of the gate, in the shape of `[workspace.lints]`.
const GATE_LINTS: &str = "gate/lints.toml";

/// The files that must be copies, each with its original: the copy first.
const MIRRORS: [(&str, &str); 2] = [
    ("clippy.toml", "gate/clippy.toml"),
    (".config/nextest.toml", "gate/nextest.toml"),
];

/// The only `clippy.toml` files: the copy that clippy reads and its original.
const CLIPPY_FILES: [&str; 2] = ["clippy.toml", "gate/clippy.toml"];

/// The findings about the configuration files of the repository at `root`, whose files are
/// `files` (repo-relative, joined with `/`), sorted by file and line.
///
/// A file the rule needs and `files` lacks is a finding, and so is a file that is not TOML where
/// it should be.
///
/// # Errors
///
/// Fails when a file in `files` that the rule reads cannot be read.
pub fn findings(root: &Path, files: &[String]) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    lint_tables(root, files, &mut findings)?;
    crate_manifests(root, files, &mut findings)?;
    for (copy, original) in MIRRORS {
        mirror(root, files, copy, original, &mut findings)?;
    }
    findings.extend(stray_config(files));
    findings.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(findings)
}

fn run_rule(inputs: &Inputs<'_>) -> Result<Vec<Finding>> {
    findings(inputs.root, inputs.files)
}

/// Whether `[workspace.lints]` of the root manifest `root_manifest` holds exactly the lints of
/// `gate_lints`, the text of `gate/lints.toml`. The two are compared as parsed TOML, so a table
/// that is written differently but means the same is the same.
///
/// A manifest without the table does not match.
///
/// # Errors
///
/// Fails with [`Error::Parse`] when either text is not TOML. The message gives the line and
/// nothing of the text.
pub fn workspace_lints_match(root_manifest: &str, gate_lints: &str) -> Result<bool> {
    let manifest = parse(ROOT_MANIFEST, root_manifest)?;
    let gate = parse(GATE_LINTS, gate_lints)?;
    let table = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"));
    Ok(table == Some(&toml::Value::Table(gate)))
}

/// Whether the texts `a` and `b` are the same but for their line ends: CRLF and LF are alike.
#[must_use]
pub fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// The text of `path` when `files` lists it; else a finding says that it is missing.
fn load(
    root: &Path,
    files: &[String],
    path: &str,
    findings: &mut Vec<Finding>,
) -> Result<Option<String>> {
    if files.iter().any(|file| file == path) {
        read(root, path).map(Some)
    } else {
        findings.push(at_file(path, "is missing, and the gate needs it"));
        Ok(None)
    }
}

/// The text of the file `path` of the repository at `root`, which must be UTF-8.
fn read(root: &Path, path: &str) -> Result<String> {
    let full = root.join(path);
    fs::read_to_string(&full).map_err(|source| Error::Io { path: full, source })
}

/// The line of a TOML error in `text`, from 1, when the error knows where it is. Shared so that
/// no message quotes the text.
pub(super) fn error_line(error: &toml::de::Error, text: &str) -> Option<usize> {
    let span = error.span()?;
    let before = text.get(..span.start).unwrap_or(text);
    Some(before.matches('\n').count() + 1)
}

/// `text` as a TOML table. The error does not quote it; `what` names the file.
pub(crate) fn parse(what: &str, text: &str) -> Result<toml::Table> {
    text.parse().map_err(|error: toml::de::Error| {
        let message = error_line(&error, text).map_or_else(
            || "not valid TOML".to_owned(),
            |line| format!("not valid TOML, near line {line}"),
        );
        Error::Parse {
            what: what.to_owned(),
            message,
        }
    })
}

/// `[workspace.lints]` against `gate/lints.toml`.
fn lint_tables(root: &Path, files: &[String], findings: &mut Vec<Finding>) -> Result<()> {
    let manifest = load(root, files, ROOT_MANIFEST, findings)?;
    let gate = load(root, files, GATE_LINTS, findings)?;
    let (Some(manifest), Some(gate)) = (manifest, gate) else {
        return Ok(());
    };
    match workspace_lints_match(&manifest, &gate) {
        Ok(true) => {}
        Ok(false) => findings.push(at_file(
            ROOT_MANIFEST,
            "`[workspace.lints]` differs from `gate/lints.toml`: it must be the same lints at the \
             same levels; change `gate/lints.toml` in a gate pull request and copy it here",
        )),
        Err(Error::Parse { what, message }) => findings.push(at_file(&what, message)),
        Err(error) => return Err(error),
    }
    Ok(())
}

/// Every manifest below the root, and the root manifest if it is a package: each must inherit
/// the workspace lints.
fn crate_manifests(root: &Path, files: &[String], findings: &mut Vec<Finding>) -> Result<()> {
    let manifests = files
        .iter()
        .filter(|path| path.as_str() == ROOT_MANIFEST || path.ends_with("/Cargo.toml"))
        .filter(|path| !is_fixture(path));
    for path in manifests {
        let manifest = match parse(path, &read(root, path)?) {
            Ok(manifest) => manifest,
            // `lint_tables` has read the root manifest too, and reports it.
            Err(Error::Parse { .. }) if path == ROOT_MANIFEST => continue,
            Err(Error::Parse { message, .. }) => {
                findings.push(at_file(path, message));
                continue;
            }
            Err(error) => return Err(error),
        };
        if !inherits_lints(path, &manifest) {
            findings.push(at_file(
                path,
                "has no `[lints]` with `workspace = true`, so the crate does not get the \
                 workspace lints",
            ));
        }
    }
    Ok(())
}

/// Whether the manifest `manifest` of the file `path` has `[lints]` with `workspace = true`. The
/// root manifest of a workspace that is not a package has no crate to give lints to, and passes.
fn inherits_lints(path: &str, manifest: &toml::Table) -> bool {
    if path == ROOT_MANIFEST && !manifest.contains_key("package") {
        return true;
    }
    let workspace = manifest
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(toml::Value::as_bool);
    workspace == Some(true)
}

/// The file `copy` against its original `original`.
fn mirror(
    root: &Path,
    files: &[String],
    copy: &str,
    original: &str,
    findings: &mut Vec<Finding>,
) -> Result<()> {
    let copy_text = load(root, files, copy, findings)?;
    let original_text = load(root, files, original, findings)?;
    if let (Some(copy_text), Some(original_text)) = (copy_text, original_text)
        && !same_text(&copy_text, &original_text)
    {
        findings.push(at_file(
            copy,
            format!(
                "differs from `{original}`: it must be a copy; change `{original}` in a gate \
                 pull request and copy it here"
            ),
        ));
    }
    Ok(())
}

/// The clippy and rustfmt configuration files, the legacy `rust-toolchain` files and the `.cargo`
/// folders that must not exist: the ones that are not the root copy of `clippy.toml` and its
/// original, and not the root `.cargo`. A `rust-toolchain` file anywhere wins over the protected
/// `rust-toolchain.toml` in rustup, so none may exist.
///
/// File systems that ignore case find `Clippy.toml` as `clippy.toml`, so names are compared
/// without case. A `.cargo` folder is one finding, however many files it has.
fn stray_config(files: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut cargo_folders = BTreeSet::new();
    for path in files.iter().filter(|path| !is_fixture(path)) {
        let mut folders: Vec<&str> = path.split('/').collect();
        let name = folders.pop().unwrap_or_default().to_ascii_lowercase();
        match name.as_str() {
            "clippy.toml" | ".clippy.toml" if !CLIPPY_FILES.contains(&path.as_str()) => {
                findings.push(at_file(
                    path,
                    "a clippy configuration outside the root: clippy would read it for the \
                     crates below it; only the root `clippy.toml`, a copy of \
                     `gate/clippy.toml`, may exist",
                ));
            }
            "rustfmt.toml" | ".rustfmt.toml" => findings.push(at_file(
                path,
                "a rustfmt configuration: the gate has none, and one outside `gate/` could \
                 change what `fmt` checks",
            )),
            "rust-toolchain" => findings.push(at_file(
                path,
                "a legacy toolchain file: rustup reads it instead of `rust-toolchain.toml`, \
                 which holds the pin; remove it",
            )),
            _ => {}
        }
        for (index, folder) in folders.iter().enumerate() {
            let root_cargo = index == 0 && *folder == ".cargo";
            if folder.eq_ignore_ascii_case(".cargo") && !root_cargo {
                cargo_folders.insert(folders[..=index].join("/"));
            }
        }
    }
    findings.extend(cargo_folders.iter().map(|folder| {
        at_file(
            folder,
            "a `.cargo` folder outside the root: cargo would read its configuration for the \
             crates below it",
        )
    }));
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::lints::testing::paths;

    #[test]
    fn lints_tables_compare_parsed() -> crate::Result<()> {
        let gate = "[rust]\nunsafe_code = \"forbid\"\nrust_2018_idioms = { level = \"warn\", \
                    priority = -1 }\n\n[clippy]\npedantic = { level = \"warn\", priority = -1 }\n";
        let reformatted = "[workspace]\nmembers = [\"crates/*\"]\n\n\
                           [workspace.lints.clippy.pedantic]\nlevel = \"warn\"\npriority = -1\n\n\
                           [workspace.lints.rust]\n# a comment\nrust_2018_idioms = { priority = \
                           -1, level = \"warn\" }\nunsafe_code = \"forbid\"\n";
        assert!(workspace_lints_match(reformatted, gate)?);

        let weaker = reformatted.replace("forbid", "warn");
        assert!(!workspace_lints_match(&weaker, gate)?);
        let extra = format!(
            "{reformatted}\n[workspace.lints.rustdoc]\nbroken_intra_doc_links = \"deny\"\n"
        );
        assert!(!workspace_lints_match(&extra, gate)?);
        let fewer = "[workspace]\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n";
        assert!(!workspace_lints_match(fewer, gate)?);
        let none = "[workspace]\nmembers = []\n";
        assert!(!workspace_lints_match(none, gate)?);
        let package_lints = "[lints.rust]\nunsafe_code = \"forbid\"\n";
        assert!(!workspace_lints_match(package_lints, gate)?);
        Ok(())
    }

    #[test]
    fn a_table_that_is_not_toml_is_an_error_that_does_not_quote_it() {
        for (root, gate) in [
            ("secret = \n", "[rust]\n"),
            ("[workspace]\n", "hidden = = 1\n"),
        ] {
            let result = workspace_lints_match(root, gate);
            let Err(error @ Error::Parse { .. }) = result else {
                panic!("expected a parse error: {result:?}");
            };
            let text = error.to_string();
            assert!(
                !text.contains("secret") && !text.contains("hidden"),
                "{text}"
            );
            assert!(text.contains("line 1"), "{text}");
        }
    }

    #[test]
    fn crlf_mirror_equals_lf() {
        assert!(same_text("a = 1\r\nb = 2\r\n", "a = 1\nb = 2\n"));
        assert!(same_text("a = 1\nb = 2\n", "a = 1\nb = 2\n"));
        assert!(!same_text("a = 1\r\nb = 2\r\n", "a = 1\nb = 3\n"));
        // Only a line end is allowed to differ: not a missing last line end, not a lone CR.
        assert!(!same_text("a = 1\n", "a = 1"));
        assert!(!same_text("a = 1\r", "a = 1\n"));
        assert!(!same_text("a = 1\n", "a = 1\n\n"));
    }

    #[test]
    fn config_files_are_found_wherever_they_are_not_allowed() {
        let allowed = paths(&[
            "Cargo.toml",
            "clippy.toml",
            "gate/clippy.toml",
            ".config/nextest.toml",
            "gate/nextest.toml",
            ".cargo/config.toml",
            "crates/a/Cargo.toml",
            "crates/a/src/lib.rs",
            // Fixtures are test data; nothing reads their configuration.
            "xtask/tests/fixtures/lints/x/crates/a/clippy.toml",
            "tests/fixtures/rustfmt.toml",
            "crates/a/tests/fixtures/x/.cargo/config.toml",
            "rust-toolchain.toml",
            "tests/fixtures/rust-toolchain",
        ]);
        assert_eq!(stray_config(&allowed), Vec::<Finding>::new());

        // Each path, and the file or folder the finding names.
        for (stray, flagged) in [
            ("crates/a/clippy.toml", "crates/a/clippy.toml"),
            ("crates/a/.clippy.toml", "crates/a/.clippy.toml"),
            (".clippy.toml", ".clippy.toml"),
            ("gate/.clippy.toml", "gate/.clippy.toml"),
            ("crates/clippy.toml", "crates/clippy.toml"),
            ("rustfmt.toml", "rustfmt.toml"),
            (".rustfmt.toml", ".rustfmt.toml"),
            ("crates/a/rustfmt.toml", "crates/a/rustfmt.toml"),
            ("gate/rustfmt.toml", "gate/rustfmt.toml"),
            ("crates/a/.cargo/config.toml", "crates/a/.cargo"),
            ("crates/a/.cargo/config", "crates/a/.cargo"),
            ("a/b/.cargo/config.toml", "a/b/.cargo"),
            // On Windows and macOS the file system finds these under the lower case name.
            ("crates/a/Clippy.toml", "crates/a/Clippy.toml"),
            ("RUSTFMT.TOML", "RUSTFMT.TOML"),
            ("crates/a/.Cargo/config.toml", "crates/a/.Cargo"),
            (".Cargo/config.toml", ".Cargo"),
            ("rust-toolchain", "rust-toolchain"),
            ("crates/a/rust-toolchain", "crates/a/rust-toolchain"),
            ("Rust-Toolchain", "Rust-Toolchain"),
        ] {
            let findings = stray_config(&paths(&[stray]));
            assert_eq!(findings.len(), 1, "{stray}: {findings:?}");
            assert_eq!(findings[0].file.as_deref(), Some(flagged), "{stray}");
        }
    }

    #[test]
    fn a_cargo_folder_is_one_finding_however_many_files_it_has() {
        let findings = stray_config(&paths(&[
            "crates/a/.cargo/config.toml",
            "crates/a/.cargo/other",
            "crates/b/.cargo/config.toml",
        ]));
        let files: Vec<Option<&str>> = findings
            .iter()
            .map(|finding| finding.file.as_deref())
            .collect();
        assert_eq!(files, [Some("crates/a/.cargo"), Some("crates/b/.cargo")]);
    }
}
