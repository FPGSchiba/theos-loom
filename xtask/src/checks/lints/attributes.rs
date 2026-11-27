//! Rule 3 of `lints`: no `allow` or `warn` attribute, no guard lint silenced, and no `expect` or
//! `ignore` without a reason.
//!
//! The lint levels forbid `#[allow]` and require a reason on `#[expect]` and `#[ignore]`, but
//! clippy sees only what the compiler builds on the machine that checks: not an attribute in the
//! tokens of a macro call or of a `macro_rules!` body, and not an item that a `cfg` strips (a
//! `#[cfg(windows)]` test on Linux). The rule reads the syntax tree and the macro tokens, through
//! `syn` and `proc-macro2`:
//!
//! - in the syntax tree (outer and inner attributes at every depth): every `allow` and every
//!   `warn` is a finding, with or without a reason, alone or inside `cfg_attr` (the lint levels
//!   come from `gate/lints.toml`, and a `warn` lets `cargo clippy -- -D warnings` pass with a
//!   denied lint printed as a warning; `deny` and `forbid` raise a level and pass), and every
//!   `expect(…)` without `reason = …` and every bare `ignore` is a finding too, alone or inside a
//!   `cfg_attr` (`ignore = "…"` passes);
//! - in the tokens of macro calls and macro definitions, at every depth: each `#` [`!`] bracket
//!   group is read as an attribute and judged the same way. A group that does not parse as an
//!   attribute (`#[$m]`, as in `$(#[$m:meta])*`) is judged by its identifiers: an `allow` or
//!   `warn` in it is a finding, anything else is left to the reviews (Ruling 9);
//! - everywhere, in the tree and in macro tokens, with or without a reason, an `allow` or `expect`
//!   that names one of the lints that guard the others (`unexpected_cfgs`,
//!   `clippy::allow_attributes`, `clippy::allow_attributes_without_reason`,
//!   `clippy::ignore_without_reason`) or one of the groups that contain them
//!   (`clippy::pedantic`, `clippy::restriction`) is a finding (spec §5.2): `forbid` on a lint
//!   stops rustc from honouring such an attribute, but clippy's guard lints are only `deny`.
//!
//! A string or a comment is one token that is never an attribute. An attribute that a macro
//! forwards through a metavariable is judged where the invocation writes it.

use proc_macro2::{TokenStream, TokenTree};
use syn::Meta;
use syn::Token;
use syn::punctuated::Punctuated;

use super::attrs::{Attribute, Body, attributes, split_args};
use super::{Inputs, Rule};
use crate::verdict::Finding;
use crate::view::{Source, has_ident};

/// The rule, as `RULES` lists it.
pub const RULE: Rule = Rule {
    name: "attributes",
    run: |inputs: &Inputs<'_>| Ok(inputs.view.sources().flat_map(findings).collect()),
};

/// What an `allow` is told.
const ALLOW: &str = "an `allow` attribute: use `#[expect(…, reason = \"…\")]`, or fix the code";

/// What a `warn` is told.
const WARN: &str = "a `warn` attribute: lint levels are set in `gate/lints.toml`; a lower level \
                    here lets `cargo clippy -- -D warnings` pass with the lint printed as a warning";

/// The attributes that lower a lint level below what `gate/lints.toml` sets, each with what it is
/// told. Every one is a finding wherever it stands; `deny` and `forbid` raise a level and pass.
const LOWERING: [(&str, &str); 2] = [("allow", ALLOW), ("warn", WARN)];

/// What an `expect` without a reason is told, wherever it stands.
const EXPECT: &str = "an `expect` without a reason: give one with `reason = \"…\"`";

/// What an `ignore` without a reason is told, wherever it stands.
const IGNORE: &str = "an `ignore` without a reason: give one with `#[ignore = \"…\"]`";

/// The lints that guard the others: `unexpected_cfgs` is `forbid`, the other three are `deny`.
const GUARDS: [&str; 4] = [
    "unexpected_cfgs",
    "clippy::allow_attributes",
    "clippy::allow_attributes_without_reason",
    "clippy::ignore_without_reason",
];

/// The lint groups that contain guard lints: `clippy::pedantic` and `clippy::restriction` hold
/// `ignore_without_reason` and the two allow lints, so naming them silences the guards as well.
const GROUPS: [&str; 2] = ["clippy::pedantic", "clippy::restriction"];

/// What a level-lowering attribute called `name` is told, if it is one.
fn lowering(name: &str) -> Option<&'static str> {
    LOWERING
        .iter()
        .find(|(lowering, _)| *lowering == name)
        .map(|(_, message)| *message)
}

/// The findings of one file, in line order.
#[must_use]
pub fn findings(source: &Source) -> Vec<Finding> {
    let Some(file) = &source.file else {
        return Vec::new();
    };
    let mut found: Vec<Finding> = attributes(file)
        .iter()
        .flat_map(|attribute| {
            judge(attribute)
                .into_iter()
                .map(|message| Finding::at(&source.path, attribute.line, message))
        })
        .collect();
    found.sort_by_key(|finding| finding.line);
    found
}

/// What is wrong with `attribute`: one message per problem.
fn judge(attribute: &Attribute) -> Vec<String> {
    match &attribute.body {
        Body::Meta(meta) => problems(meta),
        Body::Unparsed(tokens) => unparsed_problems(tokens),
    }
}

/// What is wrong with the attribute `meta`: one message per problem. An `expect` and an `ignore`
/// need a reason wherever they stand: clippy never sees a macro's tokens, a `cfg_attr` the
/// compiler may not apply, or an item that a `cfg` strips on the machine that checks.
fn problems(meta: &Meta) -> Vec<String> {
    let Some(name) = meta.path().get_ident().map(ToString::to_string) else {
        return Vec::new();
    };
    if let Some(message) = lowering(&name) {
        // An `allow` of a guard lint is told which guard it silences, which is the sharper message.
        let guards = match meta {
            Meta::List(list) if name == "allow" => guard_messages(&list.tokens),
            _ => Vec::new(),
        };
        return if guards.is_empty() {
            vec![message.to_owned()]
        } else {
            guards
        };
    }
    match (name.as_str(), meta) {
        ("cfg_attr", Meta::List(list)) => cfg_attr_problems(&list.tokens),
        ("expect", Meta::List(list)) => {
            let mut found = guard_messages(&list.tokens);
            if !has_reason(&list.tokens) {
                found.push(EXPECT.to_owned());
            }
            found
        }
        ("ignore", Meta::Path(_)) => vec![IGNORE.to_owned()],
        _ => Vec::new(),
    }
}

/// The problems of the attributes that `cfg_attr(predicate, attribute…)` applies, whose tokens are
/// `tokens`; they are judged for a reason wherever the `cfg_attr` stands. Arguments that do not
/// parse (a metavariable) are judged by their identifiers.
fn cfg_attr_problems(tokens: &TokenStream) -> Vec<String> {
    let parsed = syn::parse::Parser::parse2(
        Punctuated::<Meta, Token![,]>::parse_terminated,
        tokens.clone(),
    );
    match parsed {
        Ok(args) => args.iter().skip(1).flat_map(problems).collect(),
        Err(_) => unparsed_problems(tokens),
    }
}

/// An attribute that `syn` cannot read is judged by its identifiers (Ruling 9): an `allow` or a
/// `warn` anywhere in it is a finding.
fn unparsed_problems(tokens: &TokenStream) -> Vec<String> {
    LOWERING
        .iter()
        .find(|(name, _)| has_ident(tokens, name))
        .map(|(_, message)| (*message).to_owned())
        .into_iter()
        .collect()
}

/// Whether the arguments `tokens` of an `expect(…)` have a `reason = …` of their own.
fn has_reason(tokens: &TokenStream) -> bool {
    split_args(tokens).iter().any(|arg| {
        let mut trees = arg.clone().into_iter();
        matches!(trees.next(), Some(TokenTree::Ident(ident)) if ident == "reason")
            && matches!(trees.next(), Some(TokenTree::Punct(punct)) if punct.as_char() == '=')
    })
}

/// A message for each guard lint that the lint list `tokens` names.
fn guard_messages(tokens: &TokenStream) -> Vec<String> {
    split_args(tokens)
        .iter()
        .filter_map(lint_path)
        .filter_map(|lint| {
            if GUARDS.contains(&lint.as_str()) {
                Some(format!(
                    "an `allow` or `expect` of `{lint}`, one of the lints that guard the others \
                     (spec §5.2)"
                ))
            } else if GROUPS.contains(&lint.as_str()) {
                Some(format!(
                    "an `allow` or `expect` of `{lint}`, a lint group that holds lints that guard \
                     the others (spec §5.2)"
                ))
            } else {
                None
            }
        })
        .collect()
}

/// The lint `tokens` name, as `a::b`, when they are nothing but a path.
fn lint_path(tokens: &TokenStream) -> Option<String> {
    let mut path = String::new();
    for tree in tokens.clone() {
        match tree {
            TokenTree::Ident(ident) => path.push_str(&ident.to_string()),
            TokenTree::Punct(punct) if punct.as_char() == ':' => path.push(':'),
            _ => return None,
        }
    }
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::parse_source;

    /// The line and message of every finding for the source `text`.
    fn judged(text: &str) -> Vec<(u32, String)> {
        let (source, finding) = parse_source("crates/a/src/lib.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        findings(&source)
            .into_iter()
            .map(|finding| (finding.line.unwrap_or(0), finding.message))
            .collect()
    }

    fn lines(text: &str) -> Vec<u32> {
        judged(text).into_iter().map(|(line, _)| line).collect()
    }

    #[test]
    fn outer_and_inner_allow_are_findings_with_their_line() {
        let text = "#![allow(missing_docs)]\n\n#[allow(dead_code)]\nfn a() {}\n\
                    fn b() { #[allow(unused)] let _x = 1; }\n\
                    #[allow(\n    clippy::too_many_lines,\n    dead_code\n)]\nfn c() {}\n";
        let found = judged(text);
        assert_eq!(
            found.iter().map(|(line, _)| *line).collect::<Vec<_>>(),
            [1, 3, 5, 6]
        );
        assert!(
            found.iter().all(|(_, message)| message == ALLOW),
            "{found:?}"
        );
    }

    #[test]
    fn allow_inside_cfg_attr_is_a_finding() {
        assert_eq!(lines("#[cfg_attr(test, allow(unused))]\nfn a() {}\n"), [1]);
        assert_eq!(
            lines(
                "#[cfg_attr(\n    test,\n    derive(Debug),\n    allow(dead_code)\n)]\nstruct S;\n"
            ),
            [1]
        );
        assert_eq!(
            lines("#[cfg_attr(a, cfg_attr(b, allow(unused)))]\nfn a() {}\n"),
            [1]
        );
        assert_eq!(lines("#![cfg_attr(test, allow(unused))]\n"), [1]);
    }

    #[test]
    fn an_inner_allow_with_a_reason_in_a_one_line_module_is_a_finding() {
        assert_eq!(
            lines("mod m { #![allow(unused, reason = \"in one line\") ] }\n"),
            [1]
        );
        assert_eq!(
            lines("#[allow(dead_code, reason = \"why\")]\nfn a() {}\n"),
            [1]
        );
    }

    #[test]
    fn allow_in_a_macro_invocation_and_in_a_macro_rules_body_are_findings() {
        let text = "macro_rules! q { () => { #[allow(dead_code)] fn a() {} } }\n\
                    q! { }\n\
                    fwd! { #[allow(dead_code)] fn c() }\n\
                    fn f() { m!(nested { #![allow(unused)] }); }\n";
        assert_eq!(lines(text), [1, 3, 4]);
    }

    #[test]
    fn expect_and_ignore_without_reason_in_macro_tokens_are_findings_and_with_a_reason_pass() {
        let text = "macro_rules! q { () => { #[expect(dead_code)] fn a() {} \
                    #[test] #[ignore] fn b() {} } }\n";
        let found = judged(text);
        assert_eq!(
            found,
            [(1, EXPECT.to_owned()), (1, IGNORE.to_owned())],
            "{found:?}"
        );
        let passing = "macro_rules! q { () => { #[expect(dead_code, reason = \"kept\")] fn a() {} \
                       #[test] #[ignore = \"slow\"] fn b() {} } }\n\
                       m! { #[cfg_attr(test, expect(unused, reason = \"x\"))] fn c() }\n";
        assert_eq!(judged(passing), []);
        // Inside `cfg_attr` the same judgement applies.
        let nested = "m! { #[cfg_attr(test, expect(unused))] #[cfg_attr(test, ignore)] fn c() }\n";
        assert_eq!(
            judged(nested),
            [(1, EXPECT.to_owned()), (1, IGNORE.to_owned())]
        );
    }

    #[test]
    fn an_inner_attribute_in_a_function_body_is_judged() {
        assert_eq!(lines("fn f() {\n    #![allow(unused)]\n}\n"), [2]);
    }

    #[test]
    fn expect_and_ignore_in_a_tree_cfg_attr_need_a_reason() {
        let ignore = "#[test]\n#[cfg_attr(windows, ignore)]\nfn a() {}\n";
        assert_eq!(judged(ignore), [(2, IGNORE.to_owned())]);
        let expect = "#[cfg_attr(windows, expect(dead_code))]\nfn a() {}\n";
        assert_eq!(judged(expect), [(1, EXPECT.to_owned())]);
        let inner = "#![cfg_attr(windows, expect(dead_code))]\n";
        assert_eq!(judged(inner), [(1, EXPECT.to_owned())]);
        let passing = "#[test]\n#[cfg_attr(windows, ignore = \"slow\")]\n\
                       #[cfg_attr(windows, expect(dead_code, reason = \"kept\"))]\nfn a() {}\n";
        assert_eq!(judged(passing), []);
    }

    #[test]
    fn the_groups_that_hold_guard_lints_are_judged_like_guard_lints() {
        for lint in ["clippy::pedantic", "clippy::restriction"] {
            let message = format!(
                "an `allow` or `expect` of `{lint}`, a lint group that holds lints that guard \
                 the others (spec §5.2)"
            );
            for text in [
                format!("#[expect({lint}, reason = \"r\")]\nfn a() {{}}\n"),
                format!("#![expect({lint}, reason = \"r\")]\n"),
                format!("#[cfg_attr(test, expect({lint}, reason = \"r\"))]\nfn a() {{}}\n"),
                format!(
                    "macro_rules! m {{ () => {{ #[expect({lint}, reason = \"r\")] fn a() {{}} }} }}\n"
                ),
                format!("m! {{ #[expect({lint}, reason = \"r\")] fn a() }}\n"),
            ] {
                assert_eq!(judged(&text), [(1, message.clone())], "{text}");
            }
        }
    }

    // AC: TU-00.4
    #[test]
    fn expect_and_ignore_in_the_tree_need_a_reason_too() {
        let text = "#[expect(dead_code)]\nfn a() {}\n#[test]\n#[ignore]\nfn b() {}\n";
        assert_eq!(
            judged(text),
            [(1, EXPECT.to_owned()), (4, IGNORE.to_owned())]
        );
        let inner = "fn f() {\n    #![expect(unused)]\n}\n";
        assert_eq!(judged(inner), [(2, EXPECT.to_owned())]);
    }

    // AC: TU-00.4
    #[test]
    fn a_reasonless_expect_or_ignore_behind_a_cfg_is_a_finding() {
        // Clippy never sees an item that a `cfg` strips on the machine that checks.
        let windows = "#[cfg(windows)]\n#[test]\n#[ignore]\nfn a() {}\n";
        assert_eq!(judged(windows), [(3, IGNORE.to_owned())]);
        let linux = "#[cfg(target_os = \"linux\")]\n#[expect(dead_code)]\nfn a() {}\n";
        assert_eq!(judged(linux), [(2, EXPECT.to_owned())]);
        let macos = "#[cfg(target_os = \"macos\")]\n#[test]\n#[ignore]\nfn a() {}\n";
        assert_eq!(judged(macos), [(3, IGNORE.to_owned())]);
    }

    // AC: TU-00.4
    #[test]
    fn an_expect_or_ignore_with_a_reason_behind_a_cfg_passes() {
        let text = "#[cfg(windows)]\n#[test]\n#[ignore = \"why\"]\nfn a() {}\n\
                    #[cfg(target_os = \"linux\")]\n#[expect(dead_code, reason = \"why\")]\n\
                    fn b() {}\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn a_forwarded_attribute_is_not_judged() {
        let text = "macro_rules! fwd {\n    ($(#[$m:meta])* fn $n:ident()) => { $(#[$m])* fn $n() {} };\n    \
                    ($(#[$a:meta])+ struct $s:ident;) => { #[$a] struct $s; };\n}\n";
        assert_eq!(judged(text), []);
        // An `allow` ident in an attribute that cannot be read is still caught (Ruling 9).
        assert_eq!(
            lines("macro_rules! m { ($x:ty) => { #[allow($x)] fn a() {} } }\n"),
            [1]
        );
    }

    #[test]
    fn derives_doc_comments_and_cfg_attr_derive_pass() {
        let text = "//! Docs.\n/// Docs.\n#[derive(Debug, Clone)]\n#[cfg_attr(test, derive(PartialEq))]\n\
                    #[doc = \"allow\"]\n#[rustfmt::skip]\nstruct S;\n\
                    #[cfg(test)]\nmod tests {}\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn text_in_strings_and_comments_is_not_an_attribute() {
        let text = "const A: &str = \"#[allow(dead_code)]\";\n\
                    const B: &str = r#\"\n#![allow(dead_code)]\n\"#;\n\
                    // #[allow(dead_code)]\n/* #[allow(dead_code)] */\n/// #[allow(dead_code)]\n\
                    /* /* */ \" */ fn f() {} // \"\n\
                    fn g() -> char { '#' }\n\
                    m! { \"#[allow(x)]\" }\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn an_allow_or_expect_of_a_guard_lint_is_a_finding() {
        for lint in GUARDS {
            let message = format!(
                "an `allow` or `expect` of `{lint}`, one of the lints that guard the others \
                 (spec §5.2)"
            );
            for text in [
                format!("#[allow({lint}, reason = \"r\")]\nfn a() {{}}\n"),
                format!("#![allow({lint}, reason = \"r\")]\n"),
                format!("#[expect({lint}, reason = \"r\")]\nfn a() {{}}\n"),
                format!("#![expect({lint}, reason = \"r\")]\n"),
                format!("#[cfg_attr(test, expect({lint}, reason = \"r\"))]\nfn a() {{}}\n"),
                format!("#[cfg_attr(test, allow({lint}, reason = \"r\"))]\nfn a() {{}}\n"),
                format!(
                    "macro_rules! m {{ () => {{ #[expect({lint}, reason = \"r\")] fn a() {{}} }} }}\n"
                ),
                format!("m! {{ #[allow({lint}, reason = \"r\")] fn a() }}\n"),
                format!("#[expect(dead_code, {lint}, reason = \"r\")]\nfn a() {{}}\n"),
            ] {
                assert_eq!(judged(&text), [(1, message.clone())], "{text}");
            }
        }
        let other = "#[expect(clippy::unwrap_used, reason = \"r\")]\nfn a() {}\n\
                     m! { #[expect(clippy::unwrap_used, reason = \"r\")] fn b() }\n\
                     #[expect(my::unexpected_cfgs, reason = \"r\")]\nfn c() {}\n";
        assert_eq!(judged(other), []);
    }

    // AC: TU-00.4
    #[test]
    fn warn_attributes_are_findings_everywhere_allow_is() {
        for text in [
            "#![warn(warnings)]\n",
            "#[warn(clippy::unwrap_used)]\nfn a() {}\n",
            "#[cfg_attr(test, warn(unused))]\nfn a() {}\n",
            "#![cfg_attr(test, warn(unused))]\n",
            "macro_rules! q { () => { #[warn(dead_code)] fn a() {} } }\n",
            "m! { #[warn(dead_code)] fn c() }\n",
            "macro_rules! m { ($x:ty) => { #[warn($x)] fn a() {} } }\n",
        ] {
            assert_eq!(judged(text), [(1, WARN.to_owned())], "{text}");
        }
        let statement = "fn f() {\n    #[warn(clippy::unwrap_used)]\n    let _x = 1;\n}\n";
        assert_eq!(judged(statement), [(2, WARN.to_owned())]);
        let inner = "fn f() {\n    #![warn(unused)]\n}\n";
        assert_eq!(judged(inner), [(2, WARN.to_owned())]);
    }

    // AC: TU-00.4
    #[test]
    fn deny_and_forbid_raise_a_level_and_are_not_findings() {
        let text = "#![deny(warnings)]\n#![forbid(unsafe_code)]\n#[deny(clippy::unwrap_used)]\n\
                    fn a() {}\n#[cfg_attr(test, forbid(unused))]\nfn b() {}\n\
                    m! { #[deny(dead_code)] fn c() }\n";
        assert_eq!(judged(text), []);
    }

    #[test]
    fn findings_are_in_line_order_and_a_file_that_did_not_parse_has_none() {
        let text = "m! { #[allow(a)] fn a() }\n#[allow(b)]\nfn b() {}\nm! { #[allow(c)] fn c() }\n";
        assert_eq!(lines(text), [1, 2, 4]);
        let (source, _) = parse_source("a.rs", b"fn {");
        assert_eq!(findings(&source), []);
    }

    #[test]
    fn a_byte_order_mark_does_not_move_the_line() {
        let (source, _) = parse_source("a.rs", "\u{feff}#[allow(a)]\nfn a() {}\n".as_bytes());
        assert_eq!(findings(&source)[0].line, Some(1));
    }
}
