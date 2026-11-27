//! Reads the export of `cargo llvm-cov --json` and sums the line coverage of each crate.
//!
//! Only the sources of the crates count: the files under `<root>/crates/<name>/src/` and
//! `<root>/xtask/src/`, with `xtask` as the name of the second. Integration tests (`tests/`),
//! fixtures, build scripts and dependencies are not counted, whatever cargo-llvm-cov lists. The
//! crate name is the name of its folder, which the workspace layout (P00 spec §5.1) makes the name
//! of the package.
//!
//! The export gives file names as the compiler wrote them: absolute, and on Windows with `\`.
//! Both sides of a comparison are first written with `/` and without the `\\?\` prefix, and on
//! Windows, whose file names do not care about case, the comparison ignores it.
//!
//! The paths of the export and the root must agree about where the repository is. If a root is
//! given through a link, a short name or a substituted drive they do not, and no file matches. An
//! export that lists files and has none that matches is an error, not an empty result, and so is
//! an export that lists no files: otherwise every crate would have no instrumented lines and the
//! check would pass without having looked at anything.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::repo_path::{Case, Climb, RepoPath};
use crate::{Error, Result};

/// The lines of one crate: how many have code, and how many of those ran.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct CrateCoverage {
    /// The lines with code that a test ran.
    pub covered: u64,
    /// The lines with code.
    pub count: u64,
}

impl CrateCoverage {
    /// The share of the lines that ran, in percent; `None` when no line has code.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "line counts are far below 2^53, so both convert exactly"
    )]
    pub fn percent(&self) -> Option<f64> {
        // Multiplying first keeps the result exact when it can be: 600 of 1000 is 60.0.
        (self.count > 0).then(|| self.covered as f64 * 100.0 / self.count as f64)
    }
}

/// The part of the export this check reads. Everything else in it is left alone.
#[derive(Deserialize)]
struct Export {
    #[serde(rename = "type")]
    kind: Option<String>,
    data: Vec<Data>,
}

#[derive(Deserialize)]
struct Data {
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    filename: String,
    summary: Summary,
}

#[derive(Deserialize)]
struct Summary {
    lines: Lines,
}

#[derive(Clone, Copy, Deserialize)]
struct Lines {
    count: u64,
    covered: u64,
}

/// What an export of `cargo llvm-cov --json` is.
const EXPORT_TYPE: &str = "llvm.coverage.json.export";

/// Sums `summary.lines` of the export `json` per crate, over the files under
/// `<root>/crates/<name>/src/` and `<root>/xtask/src/`. Separators are normalised, and the
/// comparison is case-insensitive on Windows.
///
/// A crate with no file in the export is not in the map, and a crate whose files have no
/// instrumented lines is there with a count of 0.
///
/// # Errors
///
/// Fails when `json` is not an export of `cargo llvm-cov`, when it lists no files at all, and when
/// it lists files but none of them is a source of a crate under `root`: its paths are then not the
/// ones of this root. Either way every crate would look empty. The message gives the place and kind
/// of the problem, or the number of files and the root, and never quotes the text, which can hold
/// file names.
pub fn parse_llvm_cov(json: &str, root: &Path) -> Result<BTreeMap<String, CrateCoverage>> {
    parse_with(json, root, cfg!(windows))
}

pub(crate) fn parse_with(
    json: &str,
    root: &Path,
    ignore_case: bool,
) -> Result<BTreeMap<String, CrateCoverage>> {
    let export: Export = serde_json::from_str(json).map_err(|error| {
        parse_error(&format!(
            "{:?} at line {} column {}",
            error.classify(),
            error.line(),
            error.column()
        ))
    })?;
    if export
        .kind
        .as_deref()
        .is_some_and(|kind| kind != EXPORT_TYPE)
    {
        return Err(parse_error("it is not an export of `cargo llvm-cov`"));
    }
    let root = normal(&root.to_string_lossy());
    let mut crates: BTreeMap<String, CrateCoverage> = BTreeMap::new();
    let mut listed = 0_usize;
    for file in export.data.iter().flat_map(|data| &data.files) {
        listed += 1;
        let Lines { count, covered } = file.summary.lines;
        if covered > count {
            return Err(parse_error("a file has more covered lines than lines"));
        }
        let name = crate_of(&normal(&file.filename), &root, ignore_case).map(str::to_owned);
        if let Some(name) = name {
            let total = crates.entry(name).or_default();
            total.count = total.count.saturating_add(count);
            total.covered = total.covered.saturating_add(covered);
        }
    }
    if listed == 0 {
        // A run that measured nothing: every crate would have no instrumented lines and pass.
        return Err(parse_error("it lists no files, so nothing was measured"));
    }
    if crates.is_empty() {
        // The export and the root disagree about where the repository is: a root given through
        // a link, a short name or a substituted drive. Taking that for crates without lines
        // would let the check pass without having looked at anything.
        return Err(parse_error(&format!(
            "none of its {listed} files is a source of a crate under `{root}`, so its paths do not \
             belong to that root"
        )));
    }
    Ok(crates)
}

fn parse_error(message: &str) -> Error {
    Error::Parse {
        what: "the export of `cargo llvm-cov`".to_owned(),
        message: message.to_owned(),
    }
}

/// `path` with `/` for every separator, without the `\\?\` prefix of verbatim paths and without
/// a trailing separator, and with `.` and `..` resolved lexically: llvm-cov lists files reached
/// through `#[path]` or `include!` as `crates/a/src/../outside/x.rs`. A `..` with nothing before
/// it to remove is kept.
fn normal(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.strip_prefix("//?/").unwrap_or(&path);
    RepoPath::of(path, Climb::Keep).text().replace('\\', "/")
}

/// The crate a source file belongs to: `<name>` for `<root>/crates/<name>/src/<file>` and
/// `xtask` for `<root>/xtask/src/<file>`. Both paths are normal, see [`normal`].
fn crate_of<'a>(file: &'a str, root: &str, ignore_case: bool) -> Option<&'a str> {
    let case = if ignore_case {
        Case::Ascii
    } else {
        Case::Exact
    };
    let same = |a: &str, b: &str| case.same(a, b);
    file.get(..root.len()).filter(|start| same(start, root))?;
    let rest = file.get(root.len()..)?.strip_prefix('/')?;
    let mut parts = rest.split('/');
    let first = parts.next()?;
    let name = if same(first, "xtask") {
        "xtask"
    } else if same(first, "crates") {
        parts.next().filter(|name| !name.is_empty())?
    } else {
        return None;
    };
    // The source folder, and then at least a file name.
    let source = parts.next()?;
    let has_file = parts.next().is_some_and(|part| !part.is_empty());
    (same(source, "src") && has_file).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use serde_json::json;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    /// An export with one file per `(filename, count, covered)`.
    fn export(files: &[(&str, u64, u64)]) -> String {
        let files: Vec<_> = files
            .iter()
            .map(|(filename, count, covered)| {
                json!({
                    "filename": filename,
                    "summary": { "lines": { "count": count, "covered": covered } },
                })
            })
            .collect();
        json!({
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [{ "files": files, "functions": [] }],
        })
        .to_string()
    }

    fn cov(covered: u64, count: u64) -> CrateCoverage {
        CrateCoverage { covered, count }
    }

    fn parse(files: &[(&str, u64, u64)], root: &str) -> TestResult {
        let _ = parse_with(&export(files), Path::new(root), false)?;
        Ok(())
    }

    #[test]
    fn percent_is_the_share_of_covered_lines() {
        assert_eq!(cov(0, 0).percent(), None);
        assert_eq!(cov(0, 10).percent(), Some(0.0));
        assert_eq!(cov(600, 1000).percent(), Some(60.0));
        assert_eq!(cov(799, 1000).percent(), Some(79.9));
        assert_eq!(cov(3, 3).percent(), Some(100.0));
    }

    #[test]
    fn lines_are_summed_per_crate_under_src() -> TestResult {
        let json = export(&[
            ("/r/crates/a/src/lib.rs", 10, 8),
            ("/r/crates/a/src/deep/er/mod.rs", 5, 1),
            ("/r/crates/b/src/main.rs", 4, 4),
            ("/r/xtask/src/lib.rs", 20, 10),
            ("/r/xtask/src/checks/x.rs", 20, 20),
        ]);
        let crates = parse_with(&json, Path::new("/r"), false)?;
        assert_eq!(
            crates,
            BTreeMap::from([
                ("a".to_owned(), cov(9, 15)),
                ("b".to_owned(), cov(4, 4)),
                ("xtask".to_owned(), cov(30, 40)),
            ])
        );
        Ok(())
    }

    #[test]
    fn only_sources_of_the_repository_count() -> TestResult {
        let json = export(&[
            ("/r/crates/a/tests/it.rs", 9, 9),
            ("/r/crates/a/benches/b.rs", 9, 9),
            ("/r/crates/a/build.rs", 9, 9),
            ("/r/crates/a/src", 9, 9),
            ("/r/crates/a/source/lib.rs", 9, 9),
            ("/r/crates/src/lib.rs", 9, 9),
            ("/r/crates/a", 9, 9),
            ("/r/xtask/tests/fixtures/f/src/lib.rs", 9, 9),
            ("/r/xtask/src", 9, 9),
            ("/r/other/src/lib.rs", 9, 9),
            ("/r/target/debug/build/x/out/gen.rs", 9, 9),
            ("/rr/crates/a/src/lib.rs", 9, 9),
            ("/r-other/xtask/src/lib.rs", 9, 9),
            ("/elsewhere/r/crates/a/src/lib.rs", 9, 9),
            ("/home/me/.cargo/registry/src/x/serde/src/lib.rs", 9, 9),
            ("crates/a/src/lib.rs", 9, 9),
            ("", 9, 9),
            ("/r/crates/ok/src/lib.rs", 2, 1),
        ]);
        assert_eq!(
            parse_with(&json, Path::new("/r"), false)?,
            BTreeMap::from([("ok".to_owned(), cov(1, 2))])
        );
        Ok(())
    }

    #[test]
    fn a_file_that_climbs_out_of_src_is_not_in_the_crate() {
        let of = |file: &str| crate_of(&normal(file), "/r", false).map(str::to_owned);
        let content = Some("loom-core".to_owned());
        assert_eq!(of("/r/crates/loom-core/src/../x.rs"), None);
        assert_eq!(of("/r/xtask/src/../tests/x.rs"), None);
        assert_eq!(of("/r/crates/loom-core/src/./a/../b.rs"), content);
        assert_eq!(of("/r/crates/loom-core/src/x.rs"), content);
    }

    #[test]
    fn separators_are_normalised() -> TestResult {
        let json = export(&[
            (r"C:\repo\crates\a\src\lib.rs", 10, 5),
            ("C:/repo/crates/a/src/other.rs", 10, 10),
            (r"C:\repo/xtask\src/lib.rs", 4, 4),
        ]);
        for root in [r"C:\repo", "C:/repo", r"C:\repo\", "C:/repo/"] {
            let crates = parse_with(&json, Path::new(root), false)?;
            assert_eq!(
                crates,
                BTreeMap::from([
                    ("a".to_owned(), cov(15, 20)),
                    ("xtask".to_owned(), cov(4, 4)),
                ]),
                "root {root}"
            );
        }
        Ok(())
    }

    #[test]
    fn the_verbatim_prefix_of_a_path_is_not_part_of_it() -> TestResult {
        let json = export(&[(r"\\?\C:\repo\crates\a\src\lib.rs", 2, 1)]);
        for root in [r"C:\repo", r"\\?\C:\repo"] {
            assert_eq!(
                parse_with(&json, Path::new(root), false)?,
                BTreeMap::from([("a".to_owned(), cov(1, 2))]),
                "root {root}"
            );
        }
        Ok(())
    }

    #[test]
    fn case_counts_unless_it_is_ignored() -> TestResult {
        let json = export(&[
            (r"D:\Repo\crates\a\src\lib.rs", 10, 5),
            (r"d:\repo\Crates\b\SRC\lib.rs", 10, 10),
        ]);
        let root = Path::new(r"d:\repo");
        assert!(parse_with(&json, root, false).is_err());
        assert_eq!(
            parse_with(&json, root, true)?,
            BTreeMap::from([("a".to_owned(), cov(5, 10)), ("b".to_owned(), cov(10, 10))])
        );
        Ok(())
    }

    #[test]
    fn the_real_function_follows_the_operating_system() -> TestResult {
        let json = export(&[("/R/crates/a/src/lib.rs", 4, 2)]);
        let result = parse_llvm_cov(&json, Path::new("/r"));
        if cfg!(windows) {
            assert_eq!(result?, BTreeMap::from([("a".to_owned(), cov(2, 4))]));
        } else {
            assert!(result.is_err(), "{result:?}");
        }
        Ok(())
    }

    #[test]
    fn several_data_entries_are_summed() -> TestResult {
        let json = json!({
            "data": [
                { "files": [{ "filename": "/r/crates/a/src/lib.rs",
                              "summary": { "lines": { "count": 4, "covered": 1 } } }] },
                { "files": [{ "filename": "/r/crates/a/src/lib.rs",
                              "summary": { "lines": { "count": 6, "covered": 2 } } }] },
            ]
        })
        .to_string();
        assert_eq!(
            parse_with(&json, Path::new("/r"), false)?,
            BTreeMap::from([("a".to_owned(), cov(3, 10))])
        );
        Ok(())
    }

    #[test]
    fn an_export_without_files_is_an_error_not_an_empty_map() {
        // A run that measured nothing, say because no test ran or the instrumentation was lost,
        // would otherwise leave every crate without lines, and every crate would pass.
        for json in [
            r#"{"data":[],"type":"llvm.coverage.json.export","version":"3.1.0"}"#,
            r#"{"data":[{"files":[],"functions":[]}],"type":"llvm.coverage.json.export"}"#,
            r#"{"data":[{"files":[]},{"files":[]}]}"#,
        ] {
            let result = parse_with(json, Path::new("/secret-root"), false);
            let Err(Error::Parse { what, message }) = &result else {
                panic!("{json}: {result:?}");
            };
            assert!(what.contains("llvm-cov"), "{what}");
            assert!(message.contains("no files"), "{message}");
            assert!(!message.contains("secret-root"), "{message}");
        }
    }

    #[test]
    fn files_that_all_belong_elsewhere_are_an_error_not_an_empty_map() {
        // The export and the root disagree about where the repository is, as they do for a root
        // given through a link or a short name. Every crate would look empty and the check
        // would pass, so that is an error.
        let files = [
            ("/work/repo/crates/a/src/lib.rs", 10, 9),
            ("/work/repo/xtask/src/lib.rs", 10, 9),
            ("/work/repo/secret-token/other.rs", 10, 9),
        ];
        for root in [
            "/other",
            "/work/rep",
            "/work/repo/crates",
            "work/repo",
            "/x",
        ] {
            let result = parse_with(&export(&files), Path::new(root), false);
            let Err(Error::Parse { what, message }) = &result else {
                panic!("{root}: {result:?}");
            };
            assert!(what.contains("llvm-cov"), "{what}");
            assert!(message.contains("3 files"), "{message}");
            assert!(message.contains(root), "{root}: {message}");
            assert!(!message.contains("secret-token"), "{message}");
            assert!(!message.contains("/work/repo/crates/a"), "{message}");
        }
    }

    #[test]
    fn one_matching_file_is_enough_to_have_a_map() -> TestResult {
        let json = export(&[
            ("/elsewhere/crates/a/src/lib.rs", 4, 4),
            ("/r/crates/b/src/lib.rs", 4, 2),
        ]);
        assert_eq!(
            parse_with(&json, Path::new("/r"), false)?,
            BTreeMap::from([("b".to_owned(), cov(2, 4))])
        );
        Ok(())
    }

    #[test]
    fn a_file_cannot_cover_more_lines_than_it_has() {
        let result = parse(&[("/r/crates/a/src/lib.rs", 3, 4)], "/r");
        assert!(result.is_err(), "{result:?}");
    }

    #[test]
    fn what_is_not_an_export_is_a_parse_error_that_quotes_nothing() {
        for text in [
            "",
            "not json {secret-token}",
            r#"{"data": "secret-token"}"#,
            r#"{"data":[{"files":[{"filename":"/r/crates/a/src/lib.rs","summary":{"lines":{"count":"secret-token","covered":1}}}]}]}"#,
            r#"{"nodata": []}"#,
            r#"{"type":"secret-token","data":[]}"#,
        ] {
            let result = parse_with(text, Path::new("/r"), false);
            let Err(Error::Parse { what, message }) = &result else {
                panic!("{text:?} gave {result:?}");
            };
            assert!(what.contains("llvm-cov"), "{what}");
            assert!(!message.contains("secret-token"), "{message}");
        }
    }
}
