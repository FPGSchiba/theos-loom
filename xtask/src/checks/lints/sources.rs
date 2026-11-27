//! Rule 1 of `lints`: library, binary and build-script code is compiled from the `src/` of its
//! crate, and holds no `include!`.
//!
//! `coverage` counts the lines of `crates/<name>/src/**` and `xtask/src/**` and nothing else
//! (`docs/gate.md` §3), so code that a library or a binary compiles from anywhere else, through
//! `#[path]`, `include!` or a `path` in the manifest, would be outside the floor. The view says
//! which files each artifact compiled (dep-info), so the rule needs no reading of paths: a file of
//! such an artifact that is not under `<manifest_dir>/src/` is a finding. A build script may be
//! its own `src_path` (`build.rs`). Test targets, examples and benchmarks are not measured code
//! and may compile anything.
//!
//! `include!` in the files of those artifacts is a finding too: what it compiles is a file
//! that no path says, and `include_str!` and `include_bytes!`, which read data, stay allowed. The
//! macro is looked for in the syntax tree and in the tokens of every macro (an `include` ident
//! followed by `!`), which over-reports a macro that writes the word without expanding it.

use std::collections::BTreeSet;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use super::{Inputs, Rule, at_file};
use crate::verdict::Finding;
use crate::view::{Artifact, Source, TargetKind, line_of};

/// The rule, as `RULES` lists it.
pub const RULE: Rule = Rule {
    name: "sources",
    run: |inputs| Ok(findings(inputs)),
};

fn findings(inputs: &Inputs<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    // A file that several artifacts compile is read once for its includes.
    let mut included = BTreeSet::new();
    for artifact in inputs.view.artifacts.iter().filter(|a| is_measured(a)) {
        let src = src_prefix(&artifact.manifest_dir);
        for file in artifact.files.iter().filter(|file| is_rust(file)) {
            let inside = file.starts_with(&src)
                || (artifact.kind == TargetKind::BuildScript && *file == artifact.src_path);
            if !inside {
                findings.push(at_file(
                    file,
                    format!(
                        "compiled into the {} `{}` of `{}` from outside its `src/`, where \
                         `coverage` measures library and binary code; move it under `src/`",
                        kind_name(artifact.kind),
                        artifact.name,
                        artifact.package
                    ),
                ));
            }
            if included.insert(file.as_str())
                && let Some(source) = inputs.view.source(file)
            {
                findings.extend(include_findings(source));
            }
        }
    }
    findings.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    findings.dedup();
    findings
}

/// Whether the artifact is library, binary or build-script code that is not a test build.
fn is_measured(artifact: &Artifact) -> bool {
    !artifact.test
        && matches!(
            artifact.kind,
            TargetKind::Lib | TargetKind::Bin | TargetKind::BuildScript
        )
}

/// The folder of the `src/` of a package whose manifest is in `manifest_dir`, with the `/` at
/// the end.
fn src_prefix(manifest_dir: &str) -> String {
    if manifest_dir.is_empty() {
        "src/".to_owned()
    } else {
        format!("{manifest_dir}/src/")
    }
}

fn is_rust(file: &str) -> bool {
    std::path::Path::new(file)
        .extension()
        .is_some_and(|ext| ext == "rs")
}

const fn kind_name(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Lib => "library",
        TargetKind::Bin => "binary",
        TargetKind::BuildScript => "build script",
        TargetKind::Test => "test",
        TargetKind::Bench => "benchmark",
        TargetKind::Example => "example",
    }
}

/// A finding at the line of every `include!` of `source`.
fn include_findings(source: &Source) -> Vec<Finding> {
    let Some(file) = &source.file else {
        return Vec::new();
    };
    let mut finder = IncludeFinder::default();
    finder.visit_file(file);
    finder
        .lines
        .into_iter()
        .map(|line| {
            Finding::at(
                &source.path,
                line,
                "`include!` in library, binary or build-script code: the file it compiles is not named by a \
                 path that the check can follow, and `coverage` measures only `src/`; declare \
                 a module instead (`include_str!` and `include_bytes!` are fine)",
            )
        })
        .collect()
}

/// Collects the lines of `include!` in the tree and in the tokens of every macro.
#[derive(Default)]
struct IncludeFinder {
    lines: BTreeSet<u32>,
}

impl<'ast> Visit<'ast> for IncludeFinder {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "include")
        {
            self.lines
                .insert(line_of(syn::spanned::Spanned::span(&mac.path)));
        }
        self.scan(&mac.tokens);
    }
}

impl IncludeFinder {
    /// An `include` ident followed by `!`, at any depth of the tokens.
    fn scan(&mut self, tokens: &TokenStream) {
        let mut iter = tokens.clone().into_iter().peekable();
        while let Some(tree) = iter.next() {
            match tree {
                TokenTree::Ident(ident) if ident == "include" => {
                    if matches!(iter.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '!') {
                        self.lines.insert(line_of(ident.span()));
                    }
                }
                TokenTree::Group(group) => {
                    self.scan(&group.stream());
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::parse_source;

    fn lines(text: &str) -> Vec<Option<u32>> {
        let (source, finding) = parse_source("crates/a/src/lib.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        include_findings(&source)
            .iter()
            .map(|finding| finding.line)
            .collect()
    }

    #[test]
    fn include_is_found_in_the_tree_and_in_macro_tokens() {
        let text = "include!(\"a.rs\");\nfn f() { include!(\"b.rs\"); }\n\
                    macro_rules! m { () => { include!(\"c.rs\"); } }\n\
                    m! { nested { include!(\"d.rs\") } }\n\
                    std::include!(\"e.rs\");\n";
        assert_eq!(lines(text), [Some(1), Some(2), Some(3), Some(4), Some(5)]);
    }

    #[test]
    fn data_macros_words_and_strings_are_not_includes() {
        let text = "const A: &str = include_str!(\"a.txt\");\n\
                    const B: &[u8] = include_bytes!(\"a.txt\");\n\
                    // include!(\"x.rs\");\n\
                    const C: &str = \"include!(x)\";\n\
                    fn include() {}\n";
        assert_eq!(lines(text), Vec::<Option<u32>>::new());
    }

    #[test]
    fn the_src_folder_of_a_root_package_is_src() {
        assert_eq!(src_prefix(""), "src/");
        assert_eq!(src_prefix("crates/a"), "crates/a/src/");
    }
}
