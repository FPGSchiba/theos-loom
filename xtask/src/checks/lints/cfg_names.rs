//! Rule 2 of `lints`: `cfg` and `cfg_attr` attributes may use only the names of
//! `gate/cfg-names.toml`.
//!
//! Each check builds the workspace under its own cfgs: clippy sets `clippy`, cargo-llvm-cov sets
//! `coverage`, a profile sets `debug_assertions` and `panic`, `miri` sets `miri`, and a `build.rs`
//! can set any name. Code under `#[cfg(not(clippy))]` is never seen by clippy and
//! `#[cfg(not(coverage))]` is never counted, while both ship. `unexpected_cfgs` at `forbid` already
//! refuses every name that rustc and cargo do not know; this rule refuses the known names that
//! differ between the builds the checks make. It is an allowlist: the bare `names` of the file
//! (`test`, `doc`, `unix`, `windows`) and the `keys` that take a value (`feature`, `target_os`, …,
//! which rustc checks), combined with `all`, `any` and `not`. A predicate of any other shape,
//! such as a literal (`cfg(false)` hides code from every check) or a path with `::`, is a
//! finding too.
//!
//! The attributes come from `syn`: the syntax tree holds every attribute that is code, including
//! the `cfg` nested in a `cfg_attr`, and the tokens of a macro invocation or definition are read
//! for a `#` `[` … `]` group (inner or outer) that parses as a `cfg` or `cfg_attr` attribute. A
//! predicate with a macro metavariable (`#[cfg($p)]`) is not judged (Ruling 6): `unexpected_cfgs`
//! refuses unknown names, and the invocation is judged where it writes the predicate itself.
//!
//! `cfg!(…)` in an expression is not an attribute: both of its branches are compiled and
//! measured, so it is out of scope (Ruling 7).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use serde::Deserialize;
use syn::{Expr, ExprLit, Lit, Meta};

use super::attrs::{Body, attributes, split_args};
use super::config_files::error_line;
use super::{Inputs, Rule};
use crate::verdict::Finding;
use crate::{Error, Result};

/// The file that lists the names.
const FILE: &str = "gate/cfg-names.toml";

/// The longest word that a finding repeats.
const WORD_LIMIT: usize = 40;

/// The rule, as `RULES` lists it.
pub const RULE: Rule = Rule {
    name: "cfg-names",
    run: |inputs| Ok(findings(inputs)),
};

/// The cfg names that code may test, from `gate/cfg-names.toml`.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CfgNames {
    /// Names that may stand alone: `#[cfg(test)]`.
    pub names: BTreeSet<String>,
    /// Names that take a value: `#[cfg(target_os = "windows")]`.
    pub keys: BTreeSet<String>,
}

impl CfgNames {
    /// The names that leave a test running in every build of a target (`ac-trace` counts a test
    /// only then): `unix`, `windows`, the keys `target_os` and `target_family`, and `test` when
    /// `with_test` (an enclosing module or function may be `#[cfg(test)]`). It does not read
    /// `gate/cfg-names.toml`: the other `target_*` keys name more than the target OS, and a test
    /// under them is not a proof on every target.
    #[must_use]
    pub fn for_target(with_test: bool) -> Self {
        let set = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
        let mut names: BTreeSet<String> = set(&["unix", "windows"]);
        if with_test {
            names.insert("test".to_owned());
        }
        Self {
            names,
            keys: set(&["target_os", "target_family"]),
        }
    }

    /// Reads `gate/cfg-names.toml` of the repository at `root`.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Config`] when the file is missing or is not what is expected.
    pub fn load(root: &Path) -> Result<Self> {
        let config = |message: String| Error::Config {
            file: FILE.to_owned(),
            message,
        };
        let text = fs::read_to_string(root.join(FILE))
            .map_err(|error| config(format!("cannot be read: {error}")))?;
        toml::from_str(&text).map_err(|error| {
            // The error quotes the text; the line is all that is shown.
            let place = error_line(&error, &text)
                .map_or_else(String::new, |line| format!(" near line {line}"));
            config(format!(
                "is not a table of `names` and `keys`, two arrays of strings{place}"
            ))
        })
    }
}

fn findings(inputs: &Inputs<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for source in inputs.view.sources() {
        let Some(file) = &source.file else { continue };
        for attribute in attributes(file) {
            // A macro-token attribute that `syn` cannot read (`#[$m]`) is not judged (Ruling 6).
            if let Body::Meta(meta) = &attribute.body
                && let Some(message) = attribute_problem(meta, inputs.cfg_names)
            {
                findings.push(Finding::at(&source.path, attribute.line, message));
            }
        }
    }
    findings.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    findings
}

/// Judges the predicate of a `cfg(…)`: a bare name that is listed, a listed key with a string,
/// or `all`/`any`/`not` over those. An empty `any()` is refused, as `cfg(false)` is: it hides
/// code from every check.
///
/// # Errors
///
/// The message says what is refused. It repeats a rejected word as ASCII letters, digits and `_`
/// (every other character as `?`), at most 40 characters of it.
pub fn judge(predicate: &Meta, names: &CfgNames) -> std::result::Result<(), String> {
    match predicate {
        Meta::Path(path) => match path.get_ident() {
            Some(ident) if names.names.contains(&ident.to_string()) => Ok(()),
            Some(ident) => Err(refused_word(&ident.to_string())),
            None => Err(unreadable()),
        },
        Meta::NameValue(pair) => {
            let is_string = matches!(
                &pair.value,
                Expr::Lit(ExprLit {
                    lit: Lit::Str(_),
                    ..
                })
            );
            match pair.path.get_ident() {
                Some(ident) if names.keys.contains(&ident.to_string()) && is_string => Ok(()),
                Some(ident) if is_string => Err(refused_word(&ident.to_string())),
                _ => Err(unreadable()),
            }
        }
        Meta::List(list) => {
            let head = list.path.get_ident().map(ToString::to_string);
            match head.as_deref() {
                Some(combinator @ ("all" | "any" | "not")) => {
                    let inner = split_args(&list.tokens);
                    let wrong_count = match combinator {
                        "not" => inner.len() != 1,
                        "any" => inner.is_empty(),
                        _ => false,
                    };
                    if wrong_count {
                        return Err(unreadable());
                    }
                    inner.into_iter().try_for_each(|tokens| {
                        let meta = parse_predicate(tokens)?;
                        judge(&meta, names)
                    })
                }
                Some(other) => Err(refused_word(other)),
                None => Err(unreadable()),
            }
        }
    }
}

/// `tokens` as a predicate. `Meta` does not read a keyword on its own (`false`, `true`), so a
/// lone ident is refused by name; anything else that does not parse cannot be read.
fn parse_predicate(tokens: TokenStream) -> std::result::Result<Meta, String> {
    syn::parse2::<Meta>(tokens.clone()).map_err(|_| {
        let mut trees = tokens.into_iter();
        match (trees.next(), trees.next()) {
            (Some(TokenTree::Ident(word)), None) => refused_word(&word.to_string()),
            _ => unreadable(),
        }
    })
}

/// What is wrong with the attribute `meta`, when it is a `cfg` or `cfg_attr` that breaks the
/// rule; `None` for every other attribute.
///
/// A predicate with a macro metavariable (`$p`) is not judged (Ruling 6), and neither is a
/// nested attribute of a `cfg_attr` that holds one: the rest of the attribute still is.
fn attribute_problem(meta: &Meta, names: &CfgNames) -> Option<String> {
    let path = meta.path();
    let is_cfg = path.is_ident("cfg");
    if !is_cfg && !path.is_ident("cfg_attr") {
        return None;
    }
    let Meta::List(list) = meta else {
        return Some(unreadable());
    };
    let mut args = split_args(&list.tokens).into_iter();
    let Some(predicate) = args.next() else {
        return Some(unreadable());
    };
    if is_cfg && args.next().is_some() {
        return Some(unreadable());
    }
    if !has_dollar(&predicate) {
        let judged = parse_predicate(predicate).and_then(|predicate| judge(&predicate, names));
        if let Err(message) = judged {
            return Some(message);
        }
    }
    // `cfg_attr(predicate, attribute…)`: the attributes it applies are judged the same way.
    args.filter(|nested| !has_dollar(nested))
        .filter_map(|nested| syn::parse2::<Meta>(nested).ok())
        .find_map(|nested| attribute_problem(&nested, names))
}

/// The message for a refused name or key.
fn refused_word(word: &str) -> String {
    format!(
        "`{}` in a `cfg(…)` or `cfg_attr(…)` predicate is not a cfg name of \
         `gate/cfg-names.toml`: use only those, with `all`, `any` and `not`, so that every check \
         builds and measures the same code; another cfg needs a gate change approved by Jann",
        echo(word)
    )
}

/// The message for a predicate of a shape that is not judged.
fn unreadable() -> String {
    "a `cfg(…)` or `cfg_attr(…)` predicate that cannot be read: only a name, `key = \"value\"` \
     and `all`, `any` and `not` over those are allowed, with the names of \
     `gate/cfg-names.toml`; another cfg needs a gate change approved by Jann"
        .to_owned()
}

/// `word` as a finding repeats it: ASCII letters, digits and `_` as they are, every other
/// character as `?`, cut at 40 characters. No format character, bidi control or `::` reaches the
/// output.
fn echo(word: &str) -> String {
    word.chars()
        .take(WORD_LIMIT)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// Whether a `$` is anywhere in `tokens`.
fn has_dollar(tokens: &TokenStream) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        TokenTree::Punct(punct) => punct.as_char() == '$',
        TokenTree::Group(group) => has_dollar(&group.stream()),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::parse_source;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn names() -> CfgNames {
        let set = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
        CfgNames {
            names: set(&["test", "doc", "unix", "windows"]),
            keys: set(&["feature", "target_os", "target_env"]),
        }
    }

    /// The messages and lines of the rule over the Rust text `text`.
    fn judged(text: &str) -> Vec<(u32, String)> {
        let (source, finding) = parse_source("a.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        let names = names();
        let mut findings = Vec::new();
        if let Some(file) = &source.file {
            for attribute in attributes(file) {
                if let Body::Meta(meta) = &attribute.body
                    && let Some(message) = attribute_problem(meta, &names)
                {
                    findings.push(Finding::at("a.rs", attribute.line, message));
                }
            }
        }
        findings
            .into_iter()
            .map(|finding| (finding.line.unwrap_or(0), finding.message))
            .collect()
    }

    fn lines(text: &str) -> Vec<u32> {
        judged(text).into_iter().map(|(line, _)| line).collect()
    }

    #[test]
    fn ordinary_predicates_pass() {
        let text = "#![cfg(doc)]\n#[cfg(test)]\nmod t {}\n#[cfg(not(test))]\nfn a() {}\n\
                    #[cfg(windows)]\nfn b() {}\n#[cfg(target_os = \"macos\")]\nfn c() {}\n\
                    #[cfg(all(unix, not(target_env = \"musl\")))]\nfn d() {}\n\
                    #[cfg(feature = \"extra\")]\nfn e() {}\n#[cfg(all())]\nfn f() {}\n\
                    #[cfg(all(test, ))]\nfn g() {}\n#[cfg(feature = r#\"x\"#)]\nfn h() {}\n\
                    #[cfg_attr(test, derive(Debug))]\nstruct S;\n\
                    #[cfg_attr(feature = \"x\", derive(Debug), derive(Clone),)]\nstruct T;\n\
                    #[cfg_attr(test)]\nstruct U;\n\
                    #[cfg_attr(test, unsafe(no_mangle))]\nfn i() {}\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn a_literal_or_a_path_with_segments_is_refused() {
        for predicate in [
            "false",
            "true",
            "a::b",
            "target_os = 1",
            "feature",
            "target_os",
            "not(test, doc)",
            "not()",
            "any()",
            "any(test, any())",
            "foo(test)",
            "all(test, miri)",
            "test, doc",
            "",
        ] {
            let text = format!("#[cfg({predicate})]\nfn a() {{}}\n");
            assert_eq!(lines(&text), [1], "{predicate}");
        }
        assert_eq!(lines("#[cfg = \"x\"]\nfn a() {}\n"), [1]);
        assert_eq!(lines("#[cfg(\"x\")]\nfn a() {}\n"), [1]);
        assert_eq!(lines("#[cfg_attr()]\nfn a() {}\n"), [1]);
        assert_eq!(lines("#[cfg_attr(false, inline)]\nfn a() {}\n"), [1]);
    }

    #[test]
    fn a_keyword_predicate_is_refused_by_name() {
        for (text, word) in [
            ("false", "false"),
            ("not(true)", "true"),
            ("all(self)", "self"),
        ] {
            let found = judged(&format!("#[cfg({text})]\nfn a() {{}}\n"));
            assert_eq!(found.len(), 1, "{text}");
            assert!(
                found[0].1.contains(&format!("`{word}`")),
                "{text}: {}",
                found[0].1
            );
        }
    }
    #[test]
    fn a_path_is_never_echoed_with_its_colons() -> TestResult {
        for text in ["clippy_::x", "a::b(test)", "std::cfg(test)", "r#try"] {
            let predicate: Meta = syn::parse_str(text)?;
            let message = judge(&predicate, &names()).err().ok_or("no refusal")?;
            assert!(!message.contains("::"), "{text}: {message}");
            assert!(!message.contains("clippy_"), "{text}: {message}");
        }
        Ok(())
    }

    #[test]
    fn nested_cfg_attr_and_combinators_are_judged() {
        let text = "#[cfg_attr(test, cfg(not(clippy)))]\nfn a() {}\n\
                    #[cfg_attr(not(clippy), allow(dead_code))]\nfn b() {}\n\
                    #[cfg_attr(test, cfg_attr(miri, inline))]\nfn c() {}\n\
                    #[cfg_attr(test, derive(Debug), cfg(any(unix, docsrs)))]\nstruct D;\n\
                    #[cfg(any(windows, all(unix, not(coverage))))]\nfn e() {}\n";
        let found = judged(text);
        let words: Vec<&str> = ["clippy", "clippy", "miri", "docsrs", "coverage"]
            .into_iter()
            .collect();
        assert_eq!(found.len(), words.len(), "{found:?}");
        for ((line, message), word) in found.iter().zip(words) {
            assert!(message.contains(&format!("`{word}`")), "{line}: {message}");
        }
        let at: Vec<u32> = found.iter().map(|(line, _)| *line).collect();
        assert_eq!(at, [1, 3, 5, 7, 9]);
    }

    #[test]
    fn inner_attributes_of_the_file_are_judged() {
        assert_eq!(lines("#![cfg(miri)]\nfn f() {}\n"), [1]);
        assert_eq!(lines("//! Docs.\n#![cfg_attr(coverage, inline)]\n"), [2]);
        assert_eq!(lines("#![cfg(feature = \"x\")]\n"), Vec::<u32>::new());
    }

    #[test]
    fn macro_bodies_are_judged_where_they_write_the_attribute() {
        let text = "macro_rules! m { () => { #[cfg(not(clippy))] fn g() {} } }\n\
                    macro_rules! n { () => { #![cfg(miri)] } }\n\
                    m! { #[cfg(docsrs)] fn h() {} }\n\
                    fn f() { m!(nested { #[cfg_attr(coverage, inline)] }); }\n";
        assert_eq!(lines(text), [1, 2, 3, 4]);
    }

    #[test]
    fn a_metavariable_in_a_predicate_is_not_judged() {
        let text = "macro_rules! p { ($p:meta) => { #[cfg($p)] fn k() {} } }\n\
                    macro_rules! q { ($p:meta) => { #[cfg_attr($p, inline)] fn k() {} } }\n\
                    macro_rules! r { ($(#[$m:meta])* $v:vis fn $n:ident()) => { $(#[$m])* $v fn $n() {} } }\n\
                    macro_rules! s { ($m:meta) => { #[$m] fn k() {} } }\n\
                    p!(not(clippy));\nr! { #[inline] fn h() }\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn only_the_part_with_a_metavariable_is_not_judged() {
        // The predicate is judged when a nested attribute holds the metavariable, and the other
        // nested attributes are judged when the predicate does.
        let text = "macro_rules! q { ($a:meta) => { #[cfg_attr(not(clippy), $a)] fn q() {} } }\n\
                    macro_rules! r { ($p:meta) => { #[cfg_attr($p, cfg(miri))] fn r() {} } }\n\
                    macro_rules! s { ($p:meta) => { #[cfg_attr($p, inline)] fn s() {} } }\n";
        assert_eq!(lines(text), [1, 2]);
    }

    #[test]
    fn a_name_in_a_string_or_a_comment_is_not_judged() {
        let text = "// #[cfg(not(clippy))]\n/// #[cfg(miri)]\n/* #[cfg(coverage)] */\n\
                    const S: &str = \"#[cfg(not(clippy))]\";\n\
                    const T: &str = r#\"#![cfg(miri)]\"#;\nfn f() -> bool { cfg!(clippy) }\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn the_echoed_word_is_ascii_and_short() -> TestResult {
        let predicate: Meta = syn::parse_str("test\u{e9}")?;
        let message = judge(&predicate, &names()).err().ok_or("no refusal")?;
        assert!(message.contains("`test?`"), "{message}");

        let long = "a".repeat(100);
        let predicate: Meta = syn::parse_str(&long)?;
        let message = judge(&predicate, &names()).err().ok_or("no refusal")?;
        assert!(
            message.contains(&format!("`{}`", "a".repeat(40))),
            "{message}"
        );
        assert!(!message.contains(&"a".repeat(41)), "{message}");

        let predicate: Meta = syn::parse_str("r#try")?;
        let message = judge(&predicate, &names()).err().ok_or("no refusal")?;
        assert!(message.contains("`r?try`"), "{message}");
        Ok(())
    }

    #[test]
    fn the_names_file_must_be_a_table_of_two_arrays() -> TestResult {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("gate"))?;
        let load = |text: &str| -> TestResult<Result<CfgNames>> {
            fs::write(dir.path().join(FILE), text)?;
            Ok(CfgNames::load(dir.path()))
        };
        let loaded = load("names = [\"test\"]\nkeys = [\"feature\"]\n")??;
        assert!(loaded.names.contains("test") && loaded.keys.contains("feature"));
        for bad in [
            "names = [\"test\"]\n",
            "names = 1\nkeys = []\n",
            "x = = 1\n",
        ] {
            assert!(matches!(load(bad)?, Err(Error::Config { .. })), "{bad}");
        }
        let missing = tempfile::tempdir()?;
        assert!(matches!(
            CfgNames::load(missing.path()),
            Err(Error::Config { .. })
        ));
        Ok(())
    }
}
