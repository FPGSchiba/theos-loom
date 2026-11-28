//! Finding and deserializing the YAML front matter of a Markdown file.

use pulldown_cmark::{Event, MetadataBlockKind, Options, Parser, Tag, TagEnd};
use serde::de::DeserializeOwned;

use super::{line_at, parse_error, unknown_key};
use crate::finding::Finding;

/// The YAML text of a front-matter block and where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrontBlock<'a> {
    /// The YAML between the fences.
    pub text: &'a str,
    /// 1-based line of the first line of `text` in the file.
    pub line: u32,
}

/// A file cut into its front matter and its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Split<'a> {
    /// The front matter, when the file has a closed block at its very start.
    pub front: Option<FrontBlock<'a>>,
    /// Everything after the closing fence (the whole text, without BOM, if there is no block).
    pub body: &'a str,
    /// 1-based line of the first line of `body` in the file.
    pub body_line: u32,
}

/// Cuts `text` into front matter and body.
///
/// Detection is `pulldown-cmark`'s YAML metadata block (`---` fences). A leading BOM is stripped.
/// pulldown-cmark reads CRLF like LF, so no normalisation is needed. What each case yields:
/// - no block, or a block that does not start at the first line: `front: None`, body is the
///   whole text;
/// - an unclosed `---`: not a metadata block for pulldown-cmark, so `front: None` and the whole
///   text (the `---` line included) is the body;
/// - `---` directly followed by `---` (also with only blank lines between): pulldown-cmark reads
///   two thematic breaks, not a block, so `front: None` and the whole text is the body;
/// - a closed block: `front: Some`, the body starts after the closing fence line.
pub(crate) fn split_front_matter(text: &str) -> Split<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let none = Split {
        front: None,
        body: text,
        body_line: 1,
    };
    let mut events =
        Parser::new_ext(text, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS).into_offset_iter();
    let Some((Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle)), start)) =
        events.next()
    else {
        return none;
    };
    if start.start != 0 {
        return none;
    }
    let mut content: Option<(usize, usize)> = None;
    for (event, range) in events {
        match event {
            Event::Text(_) => {
                // One text event per line: the content spans the first to the last.
                content = Some(content.map_or((range.start, range.end), |(s, _)| (s, range.end)));
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                let body_start = skip_line_end(text, range.end);
                return Split {
                    front: Some(FrontBlock {
                        text: content.map_or("", |(s, e)| &text[s..e]),
                        line: 2,
                    }),
                    body: &text[body_start..],
                    body_line: line_at(text, body_start),
                };
            }
            _ => {}
        }
    }
    none
}

/// Moves past one line break at `at`, when there is one.
pub(crate) fn skip_line_end(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    if rest.starts_with("\r\n") {
        at + 2
    } else if rest.starts_with('\n') {
        at + 1
    } else {
        at
    }
}

/// Peels the snippet and alias wrappers off a `serde-saphyr` error.
fn unwrapped(mut err: &serde_saphyr::Error) -> &serde_saphyr::Error {
    while let serde_saphyr::Error::WithSnippet { error, .. }
    | serde_saphyr::Error::AliasError { error, .. } = err
    {
        err = error;
    }
    err
}

/// Reads the front matter of the file at `path` into `T`; returns it with the body.
///
/// `None` when the file has no front matter (see [`split_front_matter`]; this includes an
/// unclosed and an empty `---` block, which the caller reports as missing front matter). A
/// non-map block, or any other YAML problem, is a `parse` finding. An unknown key is an `unknown-key` finding, taken
/// from `serde-saphyr`'s structured `SerdeUnknownField` error. Lines are file lines.
pub(crate) fn front_matter<'a, T: DeserializeOwned>(
    path: &str,
    text: &'a str,
) -> Result<(Option<T>, &'a str), Finding> {
    let split = split_front_matter(text);
    let Some(block) = split.front else {
        return Ok((None, split.body));
    };
    let offset = block.line - 1;
    match serde_saphyr::from_str::<T>(block.text) {
        Ok(value) => Ok((Some(value), split.body)),
        Err(err) => {
            let line = err
                .location()
                .and_then(|l| u32::try_from(l.line()).ok())
                .filter(|l| *l > 0)
                .map_or(block.line, |l| l + offset);
            Err(match unwrapped(&err) {
                serde_saphyr::Error::SerdeUnknownField {
                    field, expected, ..
                } => unknown_key(path, field, expected, Some(line)),
                _ => parse_error(path, &err.to_string(), Some(line)),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Rule;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct T {
        a: u32,
    }

    fn err(text: &str) -> Result<Finding, Box<dyn std::error::Error>> {
        front_matter::<T>("x/y.md", text)
            .err()
            .ok_or_else(|| "expected an error".into())
    }

    // AC: TU-01.1
    #[test]
    fn unknown_front_matter_key_names_file_and_key() -> TestResult {
        let f = err("---\na: 1\nzz: 2\n---\nbody")?;
        assert_eq!(f.rule, Rule::UnknownKey);
        assert!(
            f.message.contains("zz") && f.message.contains("x/y.md"),
            "{}",
            f.message
        );
        assert!(f.message.contains("`a`"), "{}", f.message);
        assert_eq!(f.line, Some(3));
        Ok(())
    }

    #[test]
    fn no_front_matter_gives_none_and_the_whole_body() -> TestResult {
        let (v, body) = front_matter::<T>("x.md", "# Title\n\ntext\n").map_err(|f| f.message)?;
        assert!(v.is_none());
        assert_eq!(body, "# Title\n\ntext\n");
        let (v, body) =
            front_matter::<T>("x.md", "text\n---\na: 1\n---\n").map_err(|f| f.message)?;
        assert!(v.is_none(), "a block not at the start is no front matter");
        assert!(body.starts_with("text"));
        Ok(())
    }

    #[test]
    fn front_matter_and_body_split_with_lines() {
        let s = split_front_matter("---\na: 1\n---\n# H\n");
        assert_eq!(s.front.map(|b| b.text), Some("a: 1\n"));
        assert_eq!(s.body, "# H\n");
        assert_eq!(s.body_line, 4);
    }

    #[test]
    fn crlf_front_matter_reads_like_lf() -> TestResult {
        let lf = "---\na: 1\n---\nbody\n";
        let crlf = lf.replace('\n', "\r\n");
        let (v, body) = front_matter::<T>("x.md", &crlf).map_err(|f| f.message)?;
        assert_eq!(v.map(|t| t.a), Some(1));
        assert_eq!(body.trim_end(), "body");
        assert_eq!(
            split_front_matter(&crlf).body_line,
            split_front_matter(lf).body_line
        );
        let f = err(&"---\na: 1\nzz: 2\n---\n".replace('\n', "\r\n"))?;
        assert_eq!((f.rule, f.line), (Rule::UnknownKey, Some(3)));
        Ok(())
    }

    #[test]
    fn bom_before_front_matter_is_ignored() -> TestResult {
        let (v, body) =
            front_matter::<T>("x.md", "\u{feff}---\na: 1\n---\nbody").map_err(|f| f.message)?;
        assert_eq!(v.map(|t| t.a), Some(1));
        assert_eq!(body, "body");
        Ok(())
    }

    #[test]
    fn unclosed_or_non_map_front_matter_is_a_parse_finding() -> TestResult {
        // Unclosed: not a block, so no front matter and the whole text is the body.
        let (v, body) = front_matter::<T>("x.md", "---\na: 1\nbody").map_err(|f| f.message)?;
        assert!(v.is_none());
        assert_eq!(body, "---\na: 1\nbody");
        // Empty block: two thematic breaks for pulldown-cmark, so no front matter.
        let (v, body) = front_matter::<T>("x.md", "---\n---\nbody").map_err(|f| f.message)?;
        assert!(v.is_none());
        assert_eq!(body, "---\n---\nbody");
        // A list is not a map.
        assert_eq!(err("---\n- a\n---\n")?.rule, Rule::Parse);
        // Broken YAML.
        assert_eq!(err("---\na: [1\n---\n")?.rule, Rule::Parse);
        Ok(())
    }
}
