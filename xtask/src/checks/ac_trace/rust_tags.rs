//! Tags in Rust sources: a `// AC: <id>` comment above a test.
//!
//! `syn` finds the tests (every `fn` with an outer test attribute, at any depth: inline modules
//! and function bodies included) and [`line_comments`] the `//` comments (a line inside a string
//! is not one), so nothing here lexes Rust. The comments directly above a test are the run of
//! `//` comments, each alone on its line, that ends on the line above the test's first outer
//! attribute or doc comment; a blank line or a code line ends the run. Every tag line in the run
//! is a tag of the test, so an explanation may stand between a tag and the attributes. A test
//! counts as proof unless it is switched off:
//!
//! - `#[ignore…]`, or a `#[cfg_attr(…, …ignore…)]` which may ignore it on some targets;
//! - a `#[cfg(…)]` whose predicate names anything but the target (`CfgNames::for_target`), or that
//!   is `any()`, which is always false;
//! - the same `#[cfg(…)]` rule on an enclosing module or function, or the file's own inner
//!   `#![cfg(…)]`, except that `test` is allowed there (`#[cfg(test)] mod tests` is the ordinary
//!   pattern).
//!
//! A file that no test target compiles proves nothing: its tags are dangling (Ruling 11).

use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{AttrStyle, Attribute, ItemFn, ItemMod, Meta};

use super::{Comment, Tag, classify, dangling, malformed};
use crate::checks::lints::CfgNames;
use crate::checks::lints::cfg_names::judge;
use crate::verdict::Finding;
use crate::view::{LineComment, Source, has_ident, line_comments, line_of};

/// Finds the tags in the Rust file `source`; `in_test_target` says whether a test target compiles
/// it.
///
/// A comment that starts like a tag and is not one, and a tag that is not among the comments
/// directly above a test, are findings; so is every tag of a file that no test target compiles.
/// A tag above a test that is switched off is a [`Tag`] that does not count. A file that did not
/// parse has none: the view reports it.
#[must_use]
pub fn scan(source: &Source, in_test_target: bool) -> (Vec<Tag>, Vec<Finding>) {
    let (Some(file), Some(tokens)) = (&source.file, &source.tokens) else {
        return (Vec::new(), Vec::new());
    };
    // The file is the outermost module: its inner `#![cfg(…)]` decides like a module's.
    let mut finder = TestFinder {
        switched_off: usize::from(!encloses_running_tests(&file.attrs)),
        ..TestFinder::default()
    };
    finder.visit_file(file);
    let comments = line_comments(&source.text, tokens);
    let owners = finder.owners(&comments);
    let mut tags = Vec::new();
    let mut findings = Vec::new();
    for comment in &comments {
        match classify(&comment.text) {
            Comment::Other => {}
            Comment::Malformed => findings.push(malformed(&source.path, comment.line)),
            Comment::Tag(ids) => match (owners.get(&comment.line), in_test_target) {
                (_, false) => findings.push(dangling(
                    &source.path,
                    comment.line,
                    "the file is one that no test target compiles",
                )),
                (Some(test), true) => tags.push(Tag {
                    file: source.path.clone(),
                    line: comment.line,
                    ids,
                    test: Some(test.name.clone()),
                    counts: test.counts,
                }),
                (None, true) => findings.push(dangling(
                    &source.path,
                    comment.line,
                    "no test follows: a tag must be in the run of `//` comment lines, each \
                     alone on its line, that ends directly above a test's first attribute or \
                     doc comment",
                )),
            },
        }
    }
    (tags, findings)
}

/// A test function.
struct Test {
    name: String,
    counts: bool,
}

/// The tests of a file, by the line of their first outer attribute or doc comment.
#[derive(Default)]
struct TestFinder {
    tests: BTreeMap<u32, Test>,
    /// How many enclosing files, modules and functions carry a `cfg` that some build does not
    /// satisfy.
    switched_off: usize,
}

impl TestFinder {
    /// The test each comment line is above, for the lines of the run of comments (alone on their
    /// line) that ends directly above the test.
    fn owners(&self, comments: &[LineComment]) -> BTreeMap<u32, &Test> {
        let alone: BTreeMap<u32, bool> = comments
            .iter()
            .map(|comment| (comment.line, comment.alone))
            .collect();
        let mut owners = BTreeMap::new();
        for (start, test) in &self.tests {
            let mut line = *start;
            while line > 1 && alone.get(&(line - 1)) == Some(&true) {
                line -= 1;
                owners.insert(line, test);
            }
        }
        owners
    }
}

impl<'ast> Visit<'ast> for TestFinder {
    fn visit_item_fn(&mut self, function: &'ast ItemFn) {
        // `syn` lists the inner attributes of the body too; they say nothing about the test.
        let attributes: Vec<&Attribute> = function
            .attrs
            .iter()
            .filter(|attribute| attribute.style == AttrStyle::Outer)
            .collect();
        if let Some(first) = attributes.first()
            && attributes
                .iter()
                .any(|attribute| is_test_attribute(attribute))
        {
            self.tests
                .entry(line_of(first.span()))
                .or_insert_with(|| Test {
                    name: function.sig.ident.to_string(),
                    counts: self.switched_off == 0
                        && attributes
                            .iter()
                            .all(|attribute| keeps_test_running(attribute)),
                });
        }
        let off = !encloses_running_tests(&function.attrs);
        self.switched_off += usize::from(off);
        syn::visit::visit_item_fn(self, function);
        self.switched_off -= usize::from(off);
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        let off = !encloses_running_tests(&module.attrs);
        self.switched_off += usize::from(off);
        syn::visit::visit_item_mod(self, module);
        self.switched_off -= usize::from(off);
    }
}

/// Whether the `cfg` attributes of an enclosing module or function leave the tests inside in every
/// build of a target: they name only `test` and the target, and none is `any()`.
fn encloses_running_tests(attributes: &[Attribute]) -> bool {
    attributes.iter().all(
        |attribute| match (&attribute.meta, attribute.path().is_ident("cfg")) {
            (Meta::List(list), true) => names_only(&list.tokens, true),
            _ => true,
        },
    )
}

/// Whether the last segment of the attribute's path is `test`, `rstest` or `test_case`.
fn is_test_attribute(attribute: &Attribute) -> bool {
    attribute.path().segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "test" | "rstest" | "test_case"
        )
    })
}

/// Whether the attribute leaves its test running: it is not `ignore`, no `cfg_attr` can add an
/// `ignore`, and a `cfg` names only the target.
fn keeps_test_running(attribute: &Attribute) -> bool {
    let path = attribute.path();
    if path.is_ident("ignore") {
        return false;
    }
    let Meta::List(list) = &attribute.meta else {
        return true;
    };
    if path.is_ident("cfg_attr") {
        !has_ident(&list.tokens, "ignore")
    } else if path.is_ident("cfg") {
        names_only(&list.tokens, false)
    } else {
        true
    }
}

/// Whether the predicate `tokens` of a `cfg` names only the target (and `test` when
/// `allow_test`), as `lints::cfg_names::judge` reads it with the names of
/// [`CfgNames::for_target`]. An `any()` never counts, and neither does a predicate that cannot be
/// read.
fn names_only(tokens: &TokenStream, allow_test: bool) -> bool {
    syn::parse2::<Meta>(tokens.clone())
        .is_ok_and(|predicate| judge(&predicate, &CfgNames::for_target(allow_test)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::parse_source;

    /// The lines joined into the text of a source file.
    fn source(lines: &[&str]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    /// Scans `text` as `src/lib.rs`, which a test target compiles when `in_test_target`.
    fn scan_text(text: &str, in_test_target: bool) -> (Vec<Tag>, Vec<Finding>) {
        let (source, finding) = parse_source("src/lib.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        scan(&source, in_test_target)
    }

    fn scan_lines(lines: &[&str]) -> (Vec<Tag>, Vec<Finding>) {
        scan_text(&source(lines), true)
    }

    /// The tag of a scan that found exactly one tag and nothing else.
    fn only_tag(lines: &[&str]) -> Tag {
        let (mut tags, findings) = scan_lines(lines);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(tags.len(), 1, "{tags:?}");
        tags.remove(0)
    }

    /// The tag above `attributes` and a function, and whether it counts.
    fn counts(attributes: &[&str]) -> bool {
        let mut lines = vec!["// AC: TU-00.1"];
        lines.extend_from_slice(attributes);
        lines.push("fn a_test() {}");
        only_tag(&lines).counts
    }

    // AC: TU-00.4
    #[test]
    fn rust_tag_above_test_counts() {
        let tag = only_tag(&["// AC: TU-00.1", "#[test]", "fn works() {}"]);
        assert_eq!(tag.file, "src/lib.rs");
        assert_eq!(tag.line, 1);
        assert_eq!(tag.ids, ["TU-00.1"]);
        assert_eq!(tag.test.as_deref(), Some("works"));
        assert!(tag.counts);
    }

    // AC: TU-00.4
    #[test]
    fn rust_tag_with_doc_and_attrs_counts() {
        let tag = only_tag(&[
            "mod tests {",
            "    // AC: TU-00.1, TU-00.2",
            "    /// What it proves.",
            "    #[tokio::test(flavor = \"multi_thread\")]",
            "    #[should_panic(",
            "        expected = \"a ] bracket\"",
            "    )]",
            "    pub async fn proves_it() {}",
            "}",
        ]);
        assert_eq!(tag.line, 2);
        assert_eq!(tag.ids, ["TU-00.1", "TU-00.2"]);
        assert_eq!(tag.test.as_deref(), Some("proves_it"));
        assert!(tag.counts);
    }

    #[test]
    fn the_attribute_and_the_function_may_share_a_line() {
        let tag = only_tag(&["// AC: TU-00.1", "#[test] fn one_line() {}"]);
        assert_eq!(tag.test.as_deref(), Some("one_line"));
        let tag = only_tag(&[
            "// AC: TU-00.1",
            "#[test]",
            "#[ignore] pub(crate) fn two() {}",
        ]);
        assert_eq!((tag.test.as_deref(), tag.counts), (Some("two"), false));
    }

    #[test]
    fn crlf_line_ends_do_not_matter() {
        let text = "// AC: TU-00.1\r\n#[test]\r\nfn works() {}\r\n";
        let (tags, findings) = scan_text(text, true);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].line, 1);
    }

    #[test]
    fn test_attributes_are_recognised_by_their_last_path_segment() {
        for attribute in [
            "#[test]",
            "#[tokio::test]",
            "#[tokio::test(flavor = \"current_thread\")]",
            "#[rstest]",
            "#[test_case(1, 2)]",
        ] {
            assert!(counts(&[attribute]), "{attribute}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn rust_tag_above_helper_is_dangling() {
        for lines in [
            &["// AC: TU-00.1", "fn helper() {}"][..],
            &["// AC: TU-00.1", "#[inline]", "fn helper() {}"],
            &["// AC: TU-00.1", "#[derive(Debug)]", "struct Case;"],
            &["// AC: TU-00.1", "#[test]", "struct NotAFunction;"],
            &["// AC: TU-00.1"],
            &[
                "// AC: TU-00.1",
                "fn helper() {}",
                "#[test]",
                "fn works() {}",
            ],
        ] {
            let (tags, findings) = scan_lines(lines);
            assert!(tags.is_empty(), "{lines:?}: {tags:?}");
            assert_eq!(findings.len(), 1, "{lines:?}: {findings:?}");
            assert_eq!(findings[0].file.as_deref(), Some("src/lib.rs"));
            assert_eq!(findings[0].line, Some(1));
        }
    }

    /// Whether the tag above the test in `lines` counts.
    fn tag_counts(lines: &[&str]) -> bool {
        only_tag(lines).counts
    }

    // AC: TU-00.4
    #[test]
    fn a_test_in_a_module_counts_only_when_the_module_cfg_names_test_or_the_target() {
        let cases = [
            ("#[cfg(test)]", true),
            ("#[cfg(windows)]", true),
            ("#[cfg(all(test, unix))]", true),
            ("#[cfg(feature = \"never\")]", false),
            ("#[cfg(any())]", false),
            ("#[cfg(all(test, feature = \"x\"))]", false),
            ("#[cfg(miri)]", false),
        ];
        for (cfg, expected) in cases {
            let lines = [
                cfg,
                "mod t {",
                "    // AC: TU-00.1",
                "    #[test]",
                "    fn inner() {}",
                "}",
            ];
            assert_eq!(tag_counts(&lines), expected, "{cfg}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn only_the_target_os_and_family_keys_leave_a_test_counting() {
        // The spec: a test does not count when its cfg names anything besides the target OS.
        for (cfg, expected) in [
            ("#[cfg(target_os = \"linux\")]", true),
            ("#[cfg(target_family = \"unix\")]", true),
            ("#[cfg(not(target_os = \"macos\"))]", true),
            ("#[cfg(target_env = \"msvc\")]", false),
            ("#[cfg(target_arch = \"aarch64\")]", false),
            ("#[cfg(all(unix, target_pointer_width = \"64\"))]", false),
            ("#[cfg(false)]", false),
        ] {
            assert_eq!(counts(&[cfg, "#[test]"]), expected, "{cfg}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn a_cfg_on_any_enclosing_module_decides() {
        for (outer, inner, expected) in [
            ("#[cfg(test)]", "#[cfg(feature = \"never\")]", false),
            ("#[cfg(feature = \"never\")]", "#[cfg(test)]", false),
            ("#[cfg(test)]", "#[cfg(windows)]", true),
        ] {
            let lines = [
                outer,
                "mod a {",
                inner,
                "    mod b {",
                "        // AC: TU-00.1",
                "        #[test]",
                "        fn inner() {}",
                "    }",
                "}",
            ];
            assert_eq!(tag_counts(&lines), expected, "{outer} {inner}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn a_test_in_a_file_counts_only_when_the_file_cfg_names_test_or_the_target() {
        let cases = [
            ("#![cfg(test)]", true),
            ("#![cfg(windows)]", true),
            ("#![cfg(feature = \"x\")]", false),
            ("#![cfg(any())]", false),
            ("#![cfg(all(test, feature = \"x\"))]", false),
        ];
        for (cfg, expected) in cases {
            let lines = [
                "//! A file with a doc comment before its cfg.",
                cfg,
                "",
                "// AC: TU-00.1",
                "#[test]",
                "fn inner() {}",
            ];
            assert_eq!(tag_counts(&lines), expected, "{cfg}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn a_test_in_a_function_body_follows_the_cfg_of_the_function() {
        for (cfg, expected) in [
            ("#[cfg(windows)]", true),
            ("#[cfg(feature = \"never\")]", false),
            ("#[cfg(any())]", false),
        ] {
            let lines = [
                cfg,
                "fn outer() {",
                "    // AC: TU-00.1",
                "    #[test]",
                "    fn inner() {}",
                "}",
            ];
            assert_eq!(tag_counts(&lines), expected, "{cfg}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn a_module_without_cfg_does_not_stop_a_test_from_counting() {
        assert!(tag_counts(&[
            "mod t {",
            "    // AC: TU-00.1",
            "    #[test]",
            "    fn inner() {}",
            "}",
        ]));
    }

    #[test]
    fn the_dangling_message_reads_as_one_line_of_prose() {
        let (_, findings) = scan_lines(&["// AC: TU-00.1", "fn helper() {}"]);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("no test follows"),
            "{findings:?}"
        );
        assert!(!findings[0].message.contains("  "), "{findings:?}");
    }

    // AC: TU-00.4
    #[test]
    fn rust_tag_with_blank_line_is_dangling() {
        for lines in [
            &["// AC: TU-00.1", "", "#[test]", "fn works() {}"][..],
            &[
                "// AC: TU-00.1",
                "",
                "// a plain comment",
                "#[test]",
                "fn works() {}",
            ],
            &[
                "// AC: TU-00.1",
                "fn helper() {}",
                "#[test]",
                "fn works() {}",
            ],
        ] {
            let (tags, findings) = scan_lines(lines);
            assert!(tags.is_empty(), "{lines:?}: {tags:?}");
            assert_eq!(findings.len(), 1, "{lines:?}: {findings:?}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn a_tag_among_the_comments_above_a_test_counts() {
        // The whole run of comments above the test is read.
        for lines in [
            &[
                "// AC: TU-00.1",
                "// an explanation",
                "#[test]",
                "fn works() {}",
            ][..],
            &[
                "// AC: TU-00.1",
                "//// not a doc line",
                "#[test]",
                "fn works() {}",
            ],
            &[
                "// an explanation",
                "// AC: TU-00.1",
                "#[test]",
                "fn works() {}",
            ],
        ] {
            let tag = only_tag(lines);
            assert_eq!(tag.test.as_deref(), Some("works"), "{lines:?}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn two_tag_lines_in_a_row_above_a_test_both_count() {
        let (tags, findings) = scan_lines(&[
            "// AC: TU-00.1",
            "// AC: TU-00.2, TU-00.3",
            "#[test]",
            "fn works() {}",
        ]);
        assert!(findings.is_empty(), "{findings:?}");
        let found: Vec<(u32, usize)> = tags.iter().map(|tag| (tag.line, tag.ids.len())).collect();
        assert_eq!(found, [(1, 1), (2, 2)]);
    }

    // AC: TU-00.4
    #[test]
    fn a_tag_below_a_doc_comment_is_inside_the_test_and_dangling() {
        let (tags, findings) = scan_lines(&[
            "/// What it proves.",
            "// AC: TU-00.1",
            "#[test]",
            "fn works() {}",
        ]);
        assert!(tags.is_empty(), "{tags:?}");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].line, Some(2));
    }

    // AC: TU-00.4
    #[test]
    fn blank_lines_and_comments_inside_the_item_do_not_matter() {
        for lines in [
            &["// AC: TU-00.1", "#[test]", "", "fn works() {}"][..],
            &[
                "// AC: TU-00.1",
                "#[test]",
                "// a note",
                "#[cfg(windows)]",
                "fn works() {}",
            ],
        ] {
            assert!(only_tag(lines).counts, "{lines:?}");
        }
    }

    // AC: TU-00.4
    #[test]
    fn inner_attributes_of_the_body_do_not_hide_the_test() {
        // `syn` lists a body's inner attributes among the function's attributes.
        let tag = only_tag(&[
            "// AC: TU-00.1",
            "#[test]",
            "fn works() {",
            "    #![expect(unused_variables, reason = \"a probe\")]",
            "    let x = 1;",
            "}",
        ]);
        assert!(tag.counts);
        // An `ignore` in the body is not the test's attribute.
        let tag = only_tag(&[
            "// AC: TU-00.1",
            "#[test]",
            "fn works() {",
            "    #![ignore]",
            "}",
        ]);
        assert!(tag.counts);
    }

    // AC: TU-00.4
    #[test]
    fn a_file_that_did_not_parse_has_no_tags() {
        let (source, finding) = parse_source(
            "src/lib.rs",
            b"// AC: TU-00.1
#[test
",
        );
        assert!(finding.is_some());
        let (tags, findings) = scan(&source, true);
        assert!(tags.is_empty() && findings.is_empty());
    }

    // AC: TU-00.4
    #[test]
    fn ignored_test_does_not_count() {
        assert!(!counts(&["#[test]", "#[ignore]"]));
        assert!(!counts(&["#[test]", "#[ignore = \"slow\"]"]));
        assert!(!counts(&["#[ignore = \"slow\"]", "#[test]"]));
        assert!(!counts(&["#[test]", "#[cfg_attr(windows, ignore)]"]));
        assert!(!counts(&[
            "#[test]",
            "#[cfg_attr(not(windows), ignore = \"x\")]"
        ]));
        // Other conditional attributes do not stop a test from running.
        assert!(counts(&["#[test]", "#[cfg_attr(windows, should_panic)]"]));
    }

    // AC: TU-00.4
    #[test]
    fn cfg_any_test_does_not_count_cfg_windows_does() {
        let cases = [
            ("#[cfg(any(test, feature = \"x\"))]", false),
            ("#[cfg(feature = \"x\")]", false),
            ("#[cfg(test)]", false),
            ("#[cfg(any())]", false),
            ("#[cfg(all(unix, debug_assertions))]", false),
            ("#[cfg(windows)]", true),
            ("#[cfg(unix)]", true),
            ("#[cfg(not(windows))]", true),
            ("#[cfg(target_os = \"linux\")]", true),
            ("#[cfg(all(unix, target_family = \"unix\"))]", true),
            ("#[cfg(any(windows, target_os = \"macos\"))]", true),
            ("#[cfg(\n    windows\n)]", true),
        ];
        for (attribute, expected) in cases {
            let lines: Vec<&str> = attribute.lines().collect();
            for order in [[&lines[..], &["#[test]"]], [&["#[test]"], &lines[..]]] {
                let attributes: Vec<&str> = order.concat();
                assert_eq!(counts(&attributes), expected, "{attribute}");
            }
        }
    }

    // AC: TU-00.4
    #[test]
    fn malformed_tags_are_findings() {
        for line in [
            "//AC: x",
            "// AC - TU-00.1",
            "// AC: TU-00.1,TU-00.2",
            "// AC:TU-00.1",
            "// AC: ",
            "// AC:",
            "//  AC: TU-00.1",
            "// AC: TU-00.1 and more",
            "// AC: TU-00.1, ",
            "// AC: CE 08.1",
            "\t//AC",
        ] {
            let (tags, findings) = scan_lines(&[line, "#[test]", "fn works() {}"]);
            assert!(tags.is_empty(), "{line}: {tags:?}");
            assert_eq!(findings.len(), 1, "{line}: {findings:?}");
            assert_eq!(findings[0].line, Some(1), "{line}");
        }
    }

    #[test]
    fn comments_that_are_not_tags_are_left_alone() {
        let (tags, findings) = scan_lines(&[
            "//! AC: TU-00.1",
            "// ACs are acceptance criteria",
            "// ACL rules",
            "// the AC: TU-00.1 tag",
            "fn helper() {",
            "    let text = \"// AC: TU-00.1\";",
            "    drop(text);",
            "}",
            "/// AC: TU-00.1",
            "#[test]",
            "fn works() {}",
        ]);
        assert!(tags.is_empty(), "{tags:?}");
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn every_tag_of_a_file_is_found_with_its_line() {
        let (tags, findings) = scan_lines(&[
            "// AC: TU-00.1",
            "#[test]",
            "fn first() {}",
            "",
            "    // AC: TU-00.2",
            "    #[test]",
            "    fn second() {}",
            "// AC: TU-00.3",
            "// AC: TU-00.4",
            "#[test]",
            "fn third() {}",
        ]);
        // Both tag lines above `third` count.
        let found: Vec<(u32, &str)> = tags
            .iter()
            .map(|tag| (tag.line, tag.ids[0].as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (1, "TU-00.1"),
                (5, "TU-00.2"),
                (8, "TU-00.3"),
                (9, "TU-00.4")
            ]
        );
        assert!(findings.is_empty(), "{findings:?}");
    }

    // AC: TU-00.4
    #[test]
    fn a_tag_line_inside_a_string_is_not_a_tag() {
        let (tags, findings) = scan_lines(&[
            "const S: &str = \"",
            "// AC: TU-00.1",
            "//AC: broken",
            "\";",
            "",
            "// AC: TU-00.2",
            "#[test]",
            "fn works() {}",
        ]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(tags.len(), 1, "{tags:?}");
        assert_eq!(
            (tags[0].line, tags[0].ids.as_slice()),
            (6, &["TU-00.2".to_owned()][..])
        );
    }

    // AC: TU-00.4
    #[test]
    fn a_tag_above_a_test_in_an_inline_module_counts() {
        let tag = only_tag(&[
            "#[cfg(test)]",
            "mod tests {",
            "    // AC: TU-00.1",
            "    #[test]",
            "    fn inner() {}",
            "}",
        ]);
        assert_eq!(
            (tag.line, tag.test.as_deref(), tag.counts),
            (3, Some("inner"), true)
        );
    }

    // AC: TU-00.4
    #[test]
    fn a_tag_above_a_test_in_a_function_body_counts() {
        let tag = only_tag(&[
            "fn outer() {",
            "    // AC: TU-00.1",
            "    #[test]",
            "    fn inner() {}",
            "}",
        ]);
        assert_eq!(
            (tag.line, tag.test.as_deref(), tag.counts),
            (2, Some("inner"), true)
        );
    }

    // AC: TU-00.4
    #[test]
    fn a_trailing_comment_after_an_attribute_keeps_the_tag() {
        let tag = only_tag(&["// AC: TU-00.1", "#[test] // slow", "fn works() {}"]);
        assert_eq!(
            (tag.line, tag.test.as_deref(), tag.counts),
            (1, Some("works"), true)
        );
    }

    // AC: TU-00.4
    #[test]
    fn tags_in_a_file_no_test_target_compiles_are_dangling() {
        let text = source(&["// AC: TU-00.1", "#[test]", "fn works() {}", "//AC: broken"]);
        let (tags, findings) = scan_text(&text, false);
        assert!(tags.is_empty(), "{tags:?}");
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert_eq!(findings[0].line, Some(1));
        assert!(
            findings[0].message.contains("no test target compiles"),
            "{findings:?}"
        );
        // A malformed tag is a finding wherever it is.
        assert_eq!(findings[1].line, Some(4));
    }
}
