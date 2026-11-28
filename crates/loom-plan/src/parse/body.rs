//! Reading the sections of a Markdown body, and from them criteria and requirement lines.
//!
//! Everything runs on `pulldown-cmark` events with byte offsets; line numbers are file lines.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::front_matter::split_front_matter;
use super::items::{after_line, walk};
use super::line_at;

/// One `##` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Raw text after the heading line, up to the next `##` heading.
    pub text: String,
    /// File line of the first line of `text`.
    pub line: u32,
}

/// The `##` sections of a body, in file order.
#[derive(Debug, Clone, Default)]
pub struct Sections {
    list: Vec<(String, Section)>,
}

impl Sections {
    /// Cuts `body` into sections; `body_line` is the file line of its first line.
    #[must_use]
    pub fn parse(body: &str, body_line: u32) -> Sections {
        // (heading text, start of the heading, end of the heading)
        let mut heads: Vec<(String, usize, usize)> = Vec::new();
        let mut current: Option<(String, usize)> = None;
        for (event, range) in Parser::new(body).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading {
                    level: HeadingLevel::H2,
                    ..
                }) => current = Some((String::new(), range.start)),
                Event::Text(t) | Event::Code(t) => {
                    if let Some((title, _)) = current.as_mut() {
                        title.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                    if let Some((title, start)) = current.take() {
                        heads.push((title.trim().to_owned(), start, range.end));
                    }
                }
                _ => {}
            }
        }
        let mut list = Vec::new();
        for (n, (title, _, head_end)) in heads.iter().enumerate() {
            let start = after_line(body, *head_end);
            let end = heads
                .get(n + 1)
                .map_or(body.len(), |next| next.1)
                .max(start);
            list.push((
                title.clone(),
                Section {
                    text: body[start..end].to_owned(),
                    line: line_at(body, start) + body_line - 1,
                },
            ));
        }
        Sections { list }
    }

    /// Cuts the body of a whole file (front matter skipped) into sections.
    #[must_use]
    pub fn from_file_text(text: &str) -> Sections {
        let split = split_front_matter(text);
        Sections::parse(split.body, split.body_line)
    }

    /// The first section with this heading, case-insensitive.
    #[must_use]
    pub fn get(&self, heading: &str) -> Option<&Section> {
        self.list
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(heading))
            .map(|(_, s)| s)
    }

    /// Sections in file order with their headings.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Section)> {
        self.list.iter().map(|(h, s)| (h.as_str(), s))
    }
}

impl Section {
    /// True when the text is only whitespace and HTML comments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        Parser::new(&self.text).all(|event| match event {
            Event::Text(t) => t.trim().is_empty(),
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Start(_)
            | Event::End(_)
            | Event::SoftBreak
            | Event::HardBreak => true,
            _ => false,
        })
    }
}

/// One acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// The number as written in the list.
    pub number: u32,
    /// The bold span that opens the item.
    pub name: Option<String>,
    /// The text of the item, marker removed.
    pub text: String,
    /// File line of the item's first line.
    pub line: u32,
}

/// The criteria under `heading`: the items of the first ordered list there, numbered from the
/// list's start number. An item that is only a `Verify:` line is no criterion.
#[must_use]
pub fn criteria(sections: &Sections, heading: &str) -> Vec<Criterion> {
    let Some(section) = sections.get(heading) else {
        return Vec::new();
    };
    let found = walk(&section.text, section.line);
    let Some(list) = found.lists.into_iter().find(|l| l.ordered) else {
        return Vec::new();
    };
    list.items
        .into_iter()
        .filter(|item| !item.text.starts_with("Verify:"))
        .map(|item| Criterion {
            number: item.number,
            name: item.name,
            text: item.text,
            line: item.line,
        })
        .collect()
}

/// One `R<n>` line of an epic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementLine {
    /// The n of `R<n>`.
    pub number: u32,
    /// The line, trimmed.
    pub text: String,
    /// File line.
    pub line: u32,
}

/// The requirement lines under `Requirements`: lines of list items or paragraphs that start
/// with `R<n>`.
#[must_use]
pub fn requirement_lines(sections: &Sections) -> Vec<RequirementLine> {
    let Some(section) = sections.get("Requirements") else {
        return Vec::new();
    };
    let found = walk(&section.text, section.line);
    let mut blocks: Vec<(&str, u32)> = found
        .paragraphs
        .iter()
        .map(|p| (p.text.as_str(), p.line))
        .collect();
    for list in &found.lists {
        blocks.extend(list.items.iter().map(|i| (i.text.as_str(), i.line)));
    }
    blocks.sort_by_key(|(_, line)| *line);
    let mut out = Vec::new();
    for (text, first) in blocks {
        for (offset, line) in text.lines().enumerate() {
            if let Some(number) = requirement_number(line) {
                out.push(RequirementLine {
                    number,
                    text: line.to_owned(),
                    line: first + u32::try_from(offset).unwrap_or(0),
                });
            }
        }
    }
    out
}

/// The n of a line that starts `R<n>` followed by a non-alphanumeric character or the end.
fn requirement_number(line: &str) -> Option<u32> {
    let rest = line.strip_prefix('R')?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    if rest[digits..]
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    rest[..digits].parse().ok()
}

/// What the epic checks need of a body.
#[derive(Debug, Clone)]
pub struct EpicBody {
    /// Description and Outcome both non-empty.
    pub goal_present: bool,
    /// Boundaries non-empty.
    pub scope_present: bool,
    /// The requirement lines, in file order.
    pub requirements: Vec<RequirementLine>,
}

impl EpicBody {
    /// Reads the body of the epic file text (front matter included in `text`).
    #[must_use]
    pub fn parse(text: &str) -> EpicBody {
        let sections = Sections::from_file_text(text);
        let filled = |name: &str| sections.get(name).is_some_and(|s| !s.is_empty());
        EpicBody {
            goal_present: filled("Description") && filled("Outcome"),
            scope_present: filled("Boundaries"),
            requirements: requirement_lines(&sections),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORY: &str = "---\nid: 1\n---\n\n# T\n\n## Description\n\nText.\n\n## Requirements\n\n1. **Item is added** (R1)\n   **Given** an empty cart\n   **When** a product is added\n   **Then** it shows once\n2. **Item is removed** (R2)\n   **Given** a cart\n   **When** removed\n   **Then** gone\n3. No bold here\n   **Given** x\n   **When** y\n   **Then** z\n\n## Notes\n\n- n\n";

    #[test]
    fn story_criteria_are_the_numbered_items_under_requirements() {
        let s = Sections::from_file_text(STORY);
        let c = criteria(&s, "Requirements");
        let numbers: Vec<u32> = c.iter().map(|c| c.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert_eq!(c[0].name.as_deref(), Some("Item is added"));
        assert_eq!(c[2].name, None);
        assert!(
            c[0].text.contains("**Then** it shows once"),
            "{:?}",
            c[0].text
        );
        assert_eq!(c[0].line, 13);
        assert_eq!(c[1].line, 17);
    }

    #[test]
    fn criteria_number_from_the_list_start() {
        let s = Sections::from_file_text("## Requirements\n\n4. **A**\n   x\n5. **B**\n   y\n");
        let numbers: Vec<u32> = criteria(&s, "requirements")
            .iter()
            .map(|c| c.number)
            .collect();
        assert_eq!(numbers, [4, 5]);
    }

    #[test]
    fn a_verify_line_alone_is_no_criterion() {
        let s = Sections::from_file_text("## Requirements\n\nVerify: the cart shows the item.\n");
        assert_eq!(criteria(&s, "Requirements"), Vec::new());
        let s = Sections::from_file_text("## Requirements\n\n1. Verify: by test\n");
        assert_eq!(criteria(&s, "Requirements"), Vec::new());
    }

    #[test]
    fn bug_criteria_come_from_acceptance_criteria() {
        let s = Sections::from_file_text(
            "## Description\n\nx\n\n## Acceptance Criteria\n\n1. **Holds**\n   **Given** a\n   **Then** b\n",
        );
        assert_eq!(criteria(&s, "Acceptance Criteria").len(), 1);
        assert_eq!(criteria(&s, "Requirements"), Vec::new());
    }

    #[test]
    fn epic_requirement_lines_are_read() {
        let s = Sections::from_file_text(
            "# E\n\n## Requirements\n\nR1 (parent R2): A shopper can add.\nR2: The total follows.\n\n## Notes\n\nR9: not here\n",
        );
        let r = requirement_lines(&s);
        let numbers: Vec<u32> = r.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [1, 2]);
        assert_eq!(r[0].line, 5);
        assert_eq!(r[1].line, 6);
        let s = Sections::from_file_text(
            "## Requirements\n\n- R1: a\n- R3 (parent R1): b\n- Rx: c\n- Reading\n",
        );
        let numbers: Vec<u32> = requirement_lines(&s).iter().map(|r| r.number).collect();
        assert_eq!(numbers, [1, 3]);
    }

    #[test]
    fn epic_goal_and_scope_presence() {
        let full = "## Description\n\nA goal.\n\n## Outcome\n\nDone.\n\n## Boundaries\n\n- Must not change: x\n";
        let b = EpicBody::parse(full);
        assert!(b.goal_present && b.scope_present);
        let empty =
            "## Description\n\nA goal.\n\n## Outcome\n\n<!-- fill -->\n\n## Boundaries\n\n\n";
        let b = EpicBody::parse(empty);
        assert!(!b.goal_present && !b.scope_present);
        let b = EpicBody::parse("# E\n");
        assert!(!b.goal_present && !b.scope_present);
    }

    #[test]
    fn crlf_body_reads_like_lf() {
        let crlf = STORY.replace('\n', "\r\n");
        let a = criteria(&Sections::from_file_text(STORY), "Requirements");
        let b = criteria(&Sections::from_file_text(&crlf), "Requirements");
        assert_eq!(a, b);
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn po_templates_read_as_expected() {
        let story = include_str!("../../tests/fixtures/po-templates/story.md");
        let c = criteria(&Sections::from_file_text(story), "Requirements");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name.as_deref(), Some("Item is added"));
        let bug = include_str!("../../tests/fixtures/po-templates/bug.md");
        let c = criteria(&Sections::from_file_text(bug), "Acceptance Criteria");
        assert_eq!(c.len(), 1);
        let epic = EpicBody::parse(include_str!("../../tests/fixtures/po-templates/epic.md"));
        assert!(epic.goal_present && epic.scope_present);
        let numbers: Vec<u32> = epic.requirements.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [1, 2]);
    }
}
