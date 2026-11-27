//! The nextest `JUnit` report (`target/nextest/<profile>/junit.xml`): which tests passed.
//!
//! `ac-trace` shows the result of every tagged test next to it. The report is read with
//! `quick-xml`; only the elements `testcase`, `failure`, `error` and `skipped` matter. Nothing
//! from the report goes into an error, only the byte position where reading stopped.

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::{Error, Result};

/// A `testcase` element that is being read.
struct Case {
    name: String,
    failed: bool,
    skipped: bool,
}

/// Reads the report `xml` into a map from test name to whether the test passed.
///
/// A test with a `failure` or `error` element failed. One with a `skipped` element did not run
/// and is left out. A test that failed and then passed on a retry (`flakyFailure`,
/// `rerunFailure`) passed. When a name is in the report more than once, such as the same test in
/// two binaries, it passed only if it passed every time.
///
/// # Errors
///
/// Fails when the text is not an XML document whose root is `testsuites` or `testsuite`, a
/// `testcase` has no name or is inside another one, or an element is cut off.
pub fn parse(xml: &str) -> Result<BTreeMap<String, bool>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut results = BTreeMap::new();
    let mut open: Option<Case> = None;
    let mut root_seen = false;
    let mut depth = 0_usize;
    loop {
        let event = reader.read_event().map_err(|e| {
            error(
                &reader,
                &format!("the XML is not well formed ({})", kind(&e)),
            )
        })?;
        match event {
            Event::Start(tag) => {
                check_root(&reader, &mut root_seen, &tag)?;
                depth += 1;
                if tag.local_name().as_ref() == b"testcase" {
                    open = Some(begin_case(&reader, open.is_some(), &tag)?);
                } else {
                    note(&mut open, &tag);
                }
            }
            Event::Empty(tag) => {
                check_root(&reader, &mut root_seen, &tag)?;
                if tag.local_name().as_ref() == b"testcase" {
                    record(&mut results, begin_case(&reader, open.is_some(), &tag)?);
                } else {
                    note(&mut open, &tag);
                }
            }
            Event::End(tag) => {
                depth = depth.saturating_sub(1);
                if tag.local_name().as_ref() == b"testcase"
                    && let Some(case) = open.take()
                {
                    record(&mut results, case);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen {
        return Err(error(&reader, "there is no XML element"));
    }
    if depth > 0 || open.is_some() {
        return Err(error(&reader, "an element is not closed"));
    }
    Ok(results)
}

/// An error that names where `reader` stopped, never the report's text.
fn error(reader: &Reader<&[u8]>, message: &str) -> Error {
    Error::Parse {
        what: "the JUnit report".to_owned(),
        message: format!("{message} at byte {}", reader.buffer_position()),
    }
}

/// A fixed description of a `quick-xml` error, which can quote the report.
fn kind(error: &quick_xml::Error) -> &'static str {
    match error {
        quick_xml::Error::Syntax(_) => "syntax",
        quick_xml::Error::IllFormed(_) => "ill-formed",
        quick_xml::Error::Encoding(_) => "encoding",
        _ => "other",
    }
}

/// Fails when the first element is not `testsuites` or `testsuite`.
fn check_root(reader: &Reader<&[u8]>, root_seen: &mut bool, tag: &BytesStart<'_>) -> Result<()> {
    if *root_seen {
        return Ok(());
    }
    if !matches!(tag.local_name().as_ref(), b"testsuites" | b"testsuite") {
        return Err(error(
            reader,
            "the root element is not `testsuites` or `testsuite`",
        ));
    }
    *root_seen = true;
    Ok(())
}

/// Starts the case of a `testcase` element.
fn begin_case(reader: &Reader<&[u8]>, inside_case: bool, tag: &BytesStart<'_>) -> Result<Case> {
    if inside_case {
        return Err(error(reader, "a `testcase` is inside another one"));
    }
    let name = tag
        .try_get_attribute("name")
        .map_err(|_| error(reader, "a `testcase` has an unreadable attribute"))?
        .ok_or_else(|| error(reader, "a `testcase` has no `name`"))?
        .normalized_value(XmlVersion::Implicit1_0)
        .map_err(|_| error(reader, "the `name` of a `testcase` has a bad entity"))?
        .into_owned();
    Ok(Case {
        name,
        failed: false,
        skipped: false,
    })
}

/// Marks the open case by a `failure`, `error` or `skipped` element.
fn note(open: &mut Option<Case>, tag: &BytesStart<'_>) {
    let Some(case) = open.as_mut() else {
        return;
    };
    match tag.local_name().as_ref() {
        b"failure" | b"error" => case.failed = true,
        b"skipped" => case.skipped = true,
        _ => {}
    }
}

/// Adds the finished `case` to the results.
fn record(results: &mut BTreeMap<String, bool>, case: Case) {
    if case.skipped && !case.failed {
        return;
    }
    let passed = !case.failed;
    results
        .entry(case.name)
        .and_modify(|before| *before = *before && passed)
        .or_insert(passed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    const REPORT: &str = include_str!("../../../tests/fixtures/ac-trace/nextest-junit.xml");

    // AC: TU-00.4
    #[test]
    fn junit_parses_pass_and_fail() -> TestResult {
        let results = parse(REPORT)?;
        let expected: BTreeMap<String, bool> = [
            ("umbrella::tests::passes", true),
            ("umbrella::tests::fails", false),
            ("umbrella::tests::times_out", false),
            ("flaky::passes_on_retry", true),
            // Passed in one binary and failed in the other.
            ("shared::name", false),
            ("cases::quote\"d<1>&A", true),
            ("top_level_test", true),
        ]
        .into_iter()
        .map(|(name, passed)| (name.to_owned(), passed))
        .collect();
        assert_eq!(results, expected);
        Ok(())
    }

    #[test]
    fn an_empty_report_has_no_results() -> TestResult {
        let xml =
            "<?xml version=\"1.0\"?><testsuites name=\"nextest-run\" tests=\"0\"></testsuites>";
        assert!(parse(xml)?.is_empty());
        assert!(parse("<testsuite name=\"x\"/>")?.is_empty());
        Ok(())
    }

    #[test]
    fn text_that_is_not_a_report_is_an_error() {
        for xml in [
            "",
            "not xml at all",
            "<html><body/></html>",
            "<testsuites><testcase name=\"cut off\"",
            "<testsuites><testcase name=\"a\"><failure></testsuites>",
            "<testsuites><!-- never closed",
            "<testsuites><testcase name=\"a\"><![CDATA[never closed",
            "<testsuites><testcase></testcase></testsuites>",
            "<testsuites><testcase name=\"a\"><testcase name=\"b\"/></testcase></testsuites>",
        ] {
            let result = parse(xml);
            assert!(
                matches!(result, Err(Error::Parse { .. })),
                "{xml}: {result:?}"
            );
        }
    }

    #[test]
    fn cdata_comments_and_a_declaration_inside_a_case_do_not_change_its_result() -> TestResult {
        let xml = "<testsuites><testsuite name=\"s\">\
            <testcase name=\"a\"><!-- <failure/> --><?note <failure/> ?>\
            <system-out><![CDATA[<failure/></testcase>]]></system-out></testcase>\
            <testcase name=\"b\"><!-- c --><failure/><![CDATA[<skipped/>]]></testcase>\
            </testsuite></testsuites>";
        let results = parse(xml)?;
        assert_eq!(results.get("a"), Some(&true));
        assert_eq!(results.get("b"), Some(&false));
        Ok(())
    }

    #[test]
    fn an_error_names_the_position_and_not_the_text() {
        for xml in [
            "<testsuites><testcase name=\"SECRETTEXT\"></wrong></testsuites>",
            "<testsuites><testcase name=\"SECRETTEXT\"><testcase name=\"b\"/></testcase></testsuites>",
            "<testsuites><testcase classname=\"SECRETTEXT\"/></testsuites>",
            "<SECRETTEXT/>",
        ] {
            let result = parse(xml);
            let Err(Error::Parse { what, message }) = result else {
                panic!("{xml}: {result:?}");
            };
            assert_eq!(what, "the JUnit report", "{xml}");
            assert!(message.contains("byte "), "{xml}: {message}");
            assert!(!message.contains("SECRETTEXT"), "{xml}: {message}");
            assert!(!message.contains("wrong"), "{xml}: {message}");
        }
    }
}
