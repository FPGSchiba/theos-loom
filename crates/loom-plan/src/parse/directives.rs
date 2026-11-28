//! Reading directive files: the numbered, named items of ordered lists.
//!
//! Only the front matter can fail here. Whether names are present and unique, or whether a mark
//! is allowed, is for the check rules to judge from the data.

use super::body::Sections;
use super::front_matter::split_front_matter;
use super::items::{Item, walk};
use crate::finding::Finding;
use crate::model::{Directive, DirectiveFile, DirectiveKind, DocFront};

/// Reads a directive file.
///
/// With `default_kind` every top-level ordered-list item of the body is a directive of that kind.
/// With `None` (`project.md`) the `## Engineering` and `## Design` sections set the kind and
/// lists elsewhere are not directives.
///
/// # Errors
/// A `parse` or `unknown-key` [`Finding`] for the front matter.
pub fn parse_directives(
    path: &str,
    text: &str,
    default_kind: Option<DirectiveKind>,
) -> Result<DirectiveFile, Finding> {
    let front = DocFront::parse(path, text)?;
    let split = split_front_matter(text);
    let mut directives = Vec::new();
    if let Some(kind) = default_kind {
        collect(split.body, split.body_line, kind, &mut directives);
    } else {
        let sections = Sections::parse(split.body, split.body_line);
        for (heading, section) in sections.iter() {
            let kind = if heading.eq_ignore_ascii_case("Engineering") {
                DirectiveKind::Engineering
            } else if heading.eq_ignore_ascii_case("Design") {
                DirectiveKind::Design
            } else {
                continue;
            };
            collect(&section.text, section.line, kind, &mut directives);
        }
    }
    Ok(DirectiveFile {
        path: path.to_owned(),
        front,
        directives,
    })
}

/// Adds the directives of every top-level ordered list in `text`.
fn collect(text: &str, first_line: u32, kind: DirectiveKind, out: &mut Vec<Directive>) {
    for list in walk(text, first_line)
        .lists
        .into_iter()
        .filter(|l| l.ordered)
    {
        out.extend(list.items.into_iter().map(|item| directive(item, kind)));
    }
}

fn directive(item: Item, kind: DirectiveKind) -> Directive {
    let contradicts = item.text.lines().find_map(|line| {
        let name = line.strip_prefix("Contradicts:")?;
        let name = name.trim().trim_end_matches('.').trim();
        (!name.is_empty()).then(|| name.to_owned())
    });
    Directive {
        name: item.name,
        kind,
        text: item.text,
        contradicts,
        line: item.line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), String>;

    fn read(path: &str, text: &str, kind: Option<DirectiveKind>) -> Result<DirectiveFile, String> {
        parse_directives(path, text, kind).map_err(|f| f.message)
    }

    const ENG: &str = "---\nstatus: draft\n---\n\n# Engineering\n\n1. **Quality over speed.** Take the time.\n2. **Small modules.** One job each.\n   Contradicts: Quality over speed\n3. No name here.\n";

    #[test]
    fn directive_names_drop_the_trailing_stop() -> TestResult {
        let f = read("d.md", ENG, Some(DirectiveKind::Engineering))?;
        assert!(f.front.is_some());
        assert_eq!(f.directives.len(), 3);
        assert_eq!(f.directives[0].name.as_deref(), Some("Quality over speed"));
        assert_eq!(f.directives[0].kind, DirectiveKind::Engineering);
        assert_eq!(f.directives[0].line, 7);
        assert_eq!(f.directives[1].name.as_deref(), Some("Small modules"));
        Ok(())
    }

    #[test]
    fn contradicts_line_is_read() -> TestResult {
        let f = read("d.md", ENG, Some(DirectiveKind::Design))?;
        assert_eq!(f.directives[0].contradicts, None);
        assert_eq!(
            f.directives[1].contradicts.as_deref(),
            Some("Quality over speed")
        );
        assert_eq!(f.directives[1].kind, DirectiveKind::Design);
        Ok(())
    }

    #[test]
    fn project_headings_set_directive_kind() -> TestResult {
        let text = "# Project\n\nIntro:\n\n1. not a directive\n\n## Engineering\n\n### Group\n\n1. **A.** a\n\n## Design\n\n1. **B.** b\n\n## Other\n\n1. **C.** c\n";
        let f = read("project.md", text, None)?;
        let got: Vec<(Option<&str>, DirectiveKind)> = f
            .directives
            .iter()
            .map(|d| (d.name.as_deref(), d.kind))
            .collect();
        assert_eq!(
            got,
            [
                (Some("A"), DirectiveKind::Engineering),
                (Some("B"), DirectiveKind::Design)
            ]
        );
        Ok(())
    }

    #[test]
    fn unnamed_directive_has_no_name() -> TestResult {
        let f = read("d.md", ENG, Some(DirectiveKind::Engineering))?;
        assert_eq!(f.directives[2].name, None);
        assert!(f.directives[2].text.starts_with("No name here"));
        Ok(())
    }

    #[test]
    fn bad_front_matter_is_a_finding() {
        let r = read("d.md", "---\nstatus: final\n---\n1. **A.** a\n", None);
        assert!(r.is_err());
    }

    #[test]
    fn crlf_body_reads_like_lf() -> TestResult {
        let crlf = ENG.replace('\n', "\r\n");
        let a = read("d.md", ENG, Some(DirectiveKind::Engineering))?;
        let b = read("d.md", &crlf, Some(DirectiveKind::Engineering))?;
        assert_eq!(a.directives, b.directives);
        assert_eq!(b.directives.len(), 3);
        Ok(())
    }
}
