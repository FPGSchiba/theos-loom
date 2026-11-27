//! One Rust file as the checks read it: its text, its syntax tree and its tokens.
//!
//! `syn` and `proc-macro2` do the reading (`proc-macro2` with span locations, so every token
//! knows its line and its byte range in the text); nothing here lexes Rust.

use std::borrow::Cow;
use std::str::FromStr;

use proc_macro2::{Span, TokenStream, TokenTree};

use crate::verdict::Finding;

/// A Rust file, read once.
pub struct Source {
    /// The file, repo-relative with `/`.
    pub path: String,
    /// The text, with a byte-order mark and shebang line as they are in the file.
    pub text: String,
    /// The syntax tree, or `None` when the text does not parse. `syn` reads the text without its
    /// byte-order mark and shebang line, so only the lines of its spans match [`Source::text`],
    /// not their byte ranges; take byte ranges from [`Source::tokens`].
    pub file: Option<syn::File>,
    /// The tokens of the text, or `None` when it does not parse. Their byte ranges and lines
    /// match [`Source::text`].
    pub tokens: Option<TokenStream>,
}

/// Reads `bytes` as the Rust file `path`. The finding says the file does not parse; its message
/// never quotes the text.
#[must_use]
pub fn parse_source(path: &str, bytes: &[u8]) -> (Source, Option<Finding>) {
    let unreadable = |text: String, line: Option<u32>| {
        let source = Source {
            path: path.to_owned(),
            text,
            file: None,
            tokens: None,
        };
        let place = line.map_or_else(String::new, |line| format!(" (line {line})"));
        let finding = Finding {
            file: Some(path.to_owned()),
            line,
            message: format!(
                "this file does not parse as Rust{place}; rustc would not compile it either"
            ),
        };
        (source, Some(finding))
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return unreadable(String::from_utf8_lossy(bytes).into_owned(), None);
    };
    let file = match syn::parse_file(text) {
        Ok(file) => file,
        Err(error) => return unreadable(text.to_owned(), Some(line_of(error.span()))),
    };
    let tokens = match TokenStream::from_str(&blank_prelude(text, file.shebang.as_deref())) {
        Ok(tokens) => tokens,
        Err(error) => return unreadable(text.to_owned(), Some(line_of(error.span()))),
    };
    let source = Source {
        path: path.to_owned(),
        text: text.to_owned(),
        file: Some(file),
        tokens: Some(tokens),
    };
    (source, None)
}

/// The line of the start of `span`, counting from 1.
#[must_use]
pub fn line_of(span: Span) -> u32 {
    u32::try_from(span.start().line).unwrap_or(u32::MAX)
}

/// Whether the identifier `word` is anywhere in `tokens`, at any depth. Strings are literals, so
/// the words in them are not identifiers.
#[must_use]
pub fn has_ident(tokens: &TokenStream, word: &str) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        TokenTree::Ident(ident) => ident == word,
        TokenTree::Group(group) => has_ident(&group.stream(), word),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// `text` with a leading byte-order mark and the shebang line `syn` found (`shebang` is
/// `syn::File::shebang`, so syn's and rustc's rule decides what a shebang is) replaced by spaces
/// of the same byte length; the tokens of the result sit at the same byte offsets and lines as
/// in `text`.
fn blank_prelude<'a>(text: &'a str, shebang: Option<&str>) -> Cow<'a, str> {
    let mark_len = if text.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let blank = mark_len + shebang.map_or(0, str::len);
    if blank == 0 {
        return Cow::Borrowed(text);
    }
    Cow::Owned(format!("{}{}", " ".repeat(blank), &text[blank..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn a_file_that_does_not_parse_is_a_finding_with_its_line() -> TestResult {
        let (source, finding) = parse_source("crates/a/src/lib.rs", b"fn ok() {}\n\nfn {\n");
        assert!(source.file.is_none() && source.tokens.is_none());
        let finding = finding.ok_or("no finding")?;
        assert_eq!(finding.file.as_deref(), Some("crates/a/src/lib.rs"));
        assert_eq!(finding.line, Some(3));
        assert_eq!(
            finding.message,
            "this file does not parse as Rust (line 3); rustc would not compile it either"
        );
        Ok(())
    }

    #[test]
    fn a_file_that_is_not_utf8_is_a_finding() -> TestResult {
        let (source, finding) = parse_source("a.rs", b"fn a() {} // \xff\n");
        assert!(source.file.is_none());
        let finding = finding.ok_or("no finding")?;
        assert_eq!(finding.line, None);
        assert!(
            finding
                .message
                .starts_with("this file does not parse as Rust;")
        );
        Ok(())
    }

    #[test]
    fn a_byte_order_mark_and_a_shebang_are_blanked_with_their_length() {
        let text = "\u{feff}#!/usr/bin/env run\nfn a() {}\n";
        let (source, finding) = parse_source("a.rs", text.as_bytes());
        assert!(finding.is_none() && source.tokens.is_some());
        let shebang = source.file.and_then(|file| file.shebang);
        assert_eq!(shebang.as_deref(), Some("#!/usr/bin/env run"));
        let blank = blank_prelude(text, shebang.as_deref());
        assert_eq!(blank.len(), text.len());
        assert!(blank.starts_with(' ') && blank.ends_with("\nfn a() {}\n"));
        assert!(matches!(
            blank_prelude("#![deny(x)]\n", None),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn an_inner_attribute_after_a_comment_is_not_a_shebang() -> TestResult {
        // `syn` and rustc skip the comment, so this is an inner attribute, and its tokens stay.
        let text = "#!/*c*/[allow(dead_code)]\nfn a() {}\n";
        let (source, finding) = parse_source("a.rs", text.as_bytes());
        assert!(finding.is_none(), "{finding:?}");
        assert!(source.file.ok_or("no file")?.shebang.is_none());
        let first = source
            .tokens
            .ok_or("no tokens")?
            .into_iter()
            .next()
            .ok_or("no token")?;
        assert_eq!(&text[first.span().byte_range()], "#");
        Ok(())
    }

    #[test]
    fn an_identifier_is_found_at_any_depth_and_not_in_a_string() -> TestResult {
        let tokens: TokenStream = "a(b, c[d]) \"e\"".parse().map_err(|e| format!("{e}"))?;
        assert!(has_ident(&tokens, "a") && has_ident(&tokens, "d"));
        assert!(!has_ident(&tokens, "e") && !has_ident(&tokens, "z"));
        Ok(())
    }

    #[test]
    fn tokens_know_their_byte_range_in_the_text() -> TestResult {
        let text = "\u{feff}fn a() {}\n";
        let (source, _) = parse_source("a.rs", text.as_bytes());
        let first = source
            .tokens
            .ok_or("no tokens")?
            .into_iter()
            .next()
            .ok_or("no token")?;
        assert_eq!(&text[first.span().byte_range()], "fn");
        Ok(())
    }
}
