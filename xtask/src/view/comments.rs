//! The `//` comments of a source file.
//!
//! A comment is the part of the text that no token covers, so the scan looks only between the
//! byte ranges of the tokens `proc-macro2` found: a `//` in a string literal is inside a token,
//! and a doc comment (`///`, `//!`) is a token too.

use std::ops::Range;

use proc_macro2::{TokenStream, TokenTree};

/// A `//` comment that is not a doc comment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LineComment {
    /// The line, counting from 1.
    pub line: u32,
    /// The whole source line, without its line ending.
    pub text: String,
    /// Whether only whitespace precedes the `//` on the line.
    pub alone: bool,
}

/// The `//` comments in `text`, whose tokens are `tokens`, in order. Block comments are not
/// listed, and neither are doc comments.
#[must_use]
pub fn line_comments(text: &str, tokens: &TokenStream) -> Vec<LineComment> {
    let mut ranges = Vec::new();
    leaf_ranges(tokens, &mut ranges);
    let mut comments = Vec::new();
    let mut cursor = 0;
    for range in ranges {
        if range.start > cursor {
            scan_gap(text, cursor..range.start, &mut comments);
        }
        cursor = cursor.max(range.end);
    }
    scan_gap(text, cursor..text.len(), &mut comments);
    comments
}

/// The byte ranges of the leaf tokens of `tokens`: names, punctuation, literals, and the opening
/// and closing delimiter of each group.
fn leaf_ranges(tokens: &TokenStream, ranges: &mut Vec<Range<usize>>) {
    for tree in tokens.clone() {
        match tree {
            TokenTree::Group(group) => {
                ranges.push(group.span_open().byte_range());
                leaf_ranges(&group.stream(), ranges);
                ranges.push(group.span_close().byte_range());
            }
            TokenTree::Ident(ident) => ranges.push(ident.span().byte_range()),
            TokenTree::Punct(punct) => ranges.push(punct.span().byte_range()),
            TokenTree::Literal(literal) => ranges.push(literal.span().byte_range()),
        }
    }
}

/// Finds the comments in `text[gap]`, which holds only whitespace and comments.
fn scan_gap(text: &str, gap: Range<usize>, comments: &mut Vec<LineComment>) {
    let mut at = if gap.start == 0 {
        prelude_end(text, gap.end)
    } else {
        gap.start
    };
    while at < gap.end {
        let rest = &text[at..gap.end];
        if rest.starts_with("/*") {
            at += block_comment_len(rest);
        } else if rest.starts_with("//") {
            let len = rest.find('\n').unwrap_or(rest.len());
            if !is_doc(&rest[..len]) {
                comments.push(comment_at(text, at));
            }
            at += len;
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
}

/// Where a byte-order mark and a shebang line end at the start of the first gap, which ends at
/// `gap_end`. Whatever is left of the shebang's syntax is for the tokens: a `#!` that starts an
/// inner attribute is a token, so it is in no gap.
fn prelude_end(text: &str, gap_end: usize) -> usize {
    let mark = if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8().min(gap_end)
    } else {
        0
    };
    if gap_end > mark && text[mark..].starts_with("#!") {
        text[mark..gap_end]
            .find('\n')
            .map_or(gap_end, |newline| mark + newline)
    } else {
        mark
    }
}

/// Whether the comment is a doc comment (`///` but not `////`, or `//!`). Doc comments are
/// tokens, so one in a gap is only a comment that the tokenizer did not turn into a token.
fn is_doc(comment: &str) -> bool {
    (comment.starts_with("///") && !comment.starts_with("////")) || comment.starts_with("//!")
}

/// The length of the block comment at the start of `text`, which nests.
fn block_comment_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0_usize;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"/*") {
            depth += 1;
            at += 2;
        } else if bytes[at..].starts_with(b"*/") {
            depth = depth.saturating_sub(1);
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    bytes.len()
}

/// The comment that starts at byte `at` of `text`.
fn comment_at(text: &str, at: usize) -> LineComment {
    let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let end = text[at..]
        .find('\n')
        .map_or(text.len(), |newline| at + newline);
    let line = text[..at].matches('\n').count() + 1;
    LineComment {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        text: text[start..end].trim_end_matches('\r').to_owned(),
        alone: text[start..at].trim().is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::source::parse_source;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn comments(text: &str) -> TestResult<Vec<LineComment>> {
        let (source, finding) = parse_source("a.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        let tokens = source.tokens.ok_or("no tokens")?;
        Ok(line_comments(text, &tokens))
    }

    #[test]
    fn line_comments_are_the_text_between_tokens() -> TestResult {
        let text = [
            "//! Inner doc.",
            "// first",
            "/// outer doc",
            "fn a() -> &'static str {",
            "    let _s = \"// not a comment\";",
            "    /* // in a block */",
            "    1; // trailing",
            "    \"x\"",
            "}",
            "/* block",
            "// still block */",
            "//// four slashes",
            "",
        ]
        .join("\n");
        let found = comments(&text)?;
        let lines: Vec<(u32, &str, bool)> = found
            .iter()
            .map(|c| (c.line, c.text.as_str(), c.alone))
            .collect();
        assert_eq!(
            lines,
            [
                (2, "// first", true),
                (7, "    1; // trailing", false),
                (12, "//// four slashes", true),
            ]
        );
        Ok(())
    }

    #[test]
    fn crlf_sources_give_the_same_lines_as_lf() -> TestResult {
        let lf = "// one\nfn a() {} // two\n\n// three\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(comments(lf)?, comments(&crlf)?);
        assert_eq!(comments(&crlf)?.len(), 3);
        Ok(())
    }

    #[test]
    fn a_byte_order_mark_and_a_shebang_hide_nothing() -> TestResult {
        let found = comments("\u{feff}#!/bin/run // not a comment\n// yes\nfn a() {}\n")?;
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].line, found[0].text.as_str()), (2, "// yes"));
        Ok(())
    }
}
