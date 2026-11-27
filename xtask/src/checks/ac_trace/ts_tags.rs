//! Tags in TypeScript sources: a `// AC: <id>` comment above a Vitest or Playwright test.
//!
//! The next line, after its indentation, must start a test: `test(`, `it(`, `test.each`,
//! `it.each`, `test.only(` or `it.only(`. A tag above `test.skip(`, `it.skip(` or `test.fixme(` is
//! not a finding, but it does not count. Anything else is a tag in the wrong place.

use super::{Comment, Tag, classify, dangling, line_number, malformed};
use crate::verdict::Finding;

/// The calls that declare a test that runs.
const TEST_CALLS: [&str; 6] = [
    "test(",
    "it(",
    "test.each",
    "it.each",
    "test.only(",
    "it.only(",
];

/// The calls that declare a test that does not run.
const SKIPPED_CALLS: [&str; 3] = ["test.skip(", "it.skip(", "test.fixme("];

/// Finds the tags in the TypeScript source `text` of the file `file`.
///
/// A line that starts like a tag and is not one, and a tag whose next line does not declare a
/// test, are findings. A tag above a test that is skipped is a [`Tag`] that does not count.
#[must_use]
pub fn scan(file: &str, text: &str) -> (Vec<Tag>, Vec<Finding>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut tags = Vec::new();
    let mut findings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let number = line_number(index);
        match classify(line) {
            Comment::Other => {}
            Comment::Malformed => findings.push(malformed(file, number)),
            Comment::Tag(ids) => {
                let next = lines.get(index + 1).map_or("", |next| next.trim());
                match call_of(next) {
                    Some(call) => tags.push(Tag {
                        file: file.to_owned(),
                        line: number,
                        ids,
                        test: call.title,
                        counts: call.counts,
                    }),
                    None => findings.push(dangling(
                        file,
                        number,
                        "the next line does not start a `test(…)` or `it(…)` call",
                    )),
                }
            }
        }
    }
    (tags, findings)
}

/// The test a line declares.
struct Call {
    /// The title, when the first argument is a plain string.
    title: Option<String>,
    /// Whether the test runs.
    counts: bool,
}

/// The test declared by `line`, which is trimmed, or `None` when it declares none.
fn call_of(line: &str) -> Option<Call> {
    let (prefix, counts) = TEST_CALLS
        .iter()
        .find(|prefix| line.starts_with(**prefix))
        .map(|prefix| (*prefix, true))
        .or_else(|| {
            SKIPPED_CALLS
                .iter()
                .find(|prefix| line.starts_with(**prefix))
                .map(|prefix| (*prefix, false))
        })?;
    // `test.each(table)('title', …)` has the title behind a second call.
    let title = if prefix.ends_with('(') {
        title_of(&line[prefix.len()..])
    } else {
        None
    };
    Some(Call { title, counts })
}

/// The string at the start of `arguments`, in `'…'`, `"…"` or backquotes, without its quotes;
/// `None` when the arguments do not start with a string that ends on the same line.
fn title_of(arguments: &str) -> Option<String> {
    let arguments = arguments.trim_start();
    let quote = arguments
        .chars()
        .next()
        .filter(|c| matches!(c, '\'' | '"' | '`'))?;
    let body = &arguments[1..];
    let mut escaped = false;
    for (offset, c) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some(body[..offset].to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_lines(lines: &[&str]) -> (Vec<Tag>, Vec<Finding>) {
        scan("editor-ui/e2e/editor.spec.ts", &lines.join("\n"))
    }

    // AC: TU-00.4
    #[test]
    fn ts_playwright_test_counts_inside_describe() {
        let (tags, findings) = scan_lines(&[
            "import { expect, test } from '@playwright/test';",
            "",
            "test.describe('editor', () => {",
            "  // AC: TU-11.1",
            "  test('opens a record', async ({ page }) => {",
            "    await page.goto('/');",
            "  });",
            "",
            "  test.describe('nested', () => {",
            "    // AC: TU-11.2, TU-11.3",
            "\tit.each([1, 2])('works %i', (n) => {});",
            "    // AC: TU-11.4",
            "    test.only(\"focused\", () => {});",
            "    // AC: TU-11.5",
            "    it.only(`template ${1}`, () => {});",
            "    // AC: TU-11.6",
            "    test.each([1])('x', () => {});",
            "    // AC: TU-11.7",
            "    it('vitest style', () => {});",
            "  });",
            "});",
        ]);
        assert!(findings.is_empty(), "{findings:?}");
        let found: Vec<(u32, &[String], Option<&str>, bool)> = tags
            .iter()
            .map(|tag| (tag.line, &tag.ids[..], tag.test.as_deref(), tag.counts))
            .collect();
        let ids =
            |list: &[&str]| -> Vec<String> { list.iter().map(|id| (*id).to_owned()).collect() };
        assert_eq!(
            found,
            [
                (4, &ids(&["TU-11.1"])[..], Some("opens a record"), true),
                (10, &ids(&["TU-11.2", "TU-11.3"])[..], None, true),
                (12, &ids(&["TU-11.4"])[..], Some("focused"), true),
                (14, &ids(&["TU-11.5"])[..], Some("template ${1}"), true),
                (16, &ids(&["TU-11.6"])[..], None, true),
                (18, &ids(&["TU-11.7"])[..], Some("vitest style"), true),
            ]
        );
        assert!(
            tags.iter()
                .all(|tag| tag.file == "editor-ui/e2e/editor.spec.ts")
        );
    }

    // AC: TU-00.4
    #[test]
    fn ts_skip_and_fixme_do_not_count() {
        let (tags, findings) = scan_lines(&[
            "// AC: TU-11.1",
            "test.skip('later', () => {});",
            "// AC: TU-11.2",
            "  it.skip('later', () => {});",
            "// AC: TU-11.3",
            "test.fixme('broken', () => {});",
        ]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(tags.len(), 3);
        assert!(tags.iter().all(|tag| !tag.counts), "{tags:?}");
        assert_eq!(tags[0].test.as_deref(), Some("later"));
    }

    // AC: TU-00.4
    #[test]
    fn ts_tag_above_anything_else_is_dangling() {
        for lines in [
            &["// AC: TU-11.1", "function helper() {}"][..],
            &["// AC: TU-11.1", "", "test('a', () => {});"],
            &["// AC: TU-11.1", "// a comment", "test('a', () => {});"],
            &["// AC: TU-11.1", "test.describe('group', () => {"],
            &["// AC: TU-11.1", "test.beforeEach(() => {});"],
            &["// AC: TU-11.1", "item('not a test call');"],
            &["// AC: TU-11.1"],
        ] {
            let (tags, findings) = scan_lines(lines);
            assert!(tags.is_empty(), "{lines:?}: {tags:?}");
            assert_eq!(findings.len(), 1, "{lines:?}: {findings:?}");
            assert_eq!(findings[0].line, Some(1));
            assert_eq!(
                findings[0].file.as_deref(),
                Some("editor-ui/e2e/editor.spec.ts")
            );
        }
    }

    // AC: TU-00.4
    #[test]
    fn ts_malformed_tags_are_findings() {
        for line in [
            "//AC: x",
            "// AC - TU-11.1",
            "// AC: TU-11.1,TU-11.2",
            "  // AC:",
        ] {
            let (tags, findings) = scan_lines(&[line, "test('a', () => {});"]);
            assert!(tags.is_empty(), "{line}: {tags:?}");
            assert_eq!(findings.len(), 1, "{line}: {findings:?}");
        }
        let (tags, findings) =
            scan_lines(&["// ACs are criteria", "/* AC: x */", "test('a', () => {});"]);
        assert!(tags.is_empty() && findings.is_empty());
    }

    #[test]
    fn crlf_line_ends_do_not_matter() {
        let (tags, findings) = scan("a.ts", "// AC: TU-11.1\r\ntest('a', () => {});\r\n");
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].test.as_deref(), Some("a"));
    }
}
