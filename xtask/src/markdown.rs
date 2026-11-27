//! The one place that reads Markdown (`pulldown-cmark` with tables): headings, sections, list
//! items, tables and code spans, each with its line number.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// A heading: its level (1 to 6), its inline text and the line it starts on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Heading {
    /// 1 for `#` up to 6 for `######`.
    pub level: u8,
    /// The inline text, without the markers.
    pub text: String,
    /// The 1-based line the heading starts on.
    pub line: u32,
}

/// A heading and the text under it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section<'a> {
    /// The heading.
    pub heading: Heading,
    /// From the end of the heading to the next heading of the same or a higher level.
    pub body: &'a str,
}

/// A list item, at any depth.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListItem {
    /// The inline text of the item itself, without its nested items.
    pub text: String,
    /// The 1-based line the item starts on.
    pub line: u32,
}

/// A table with the plain text of its cells.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    /// The 1-based line of the header row.
    pub line: u32,
    /// The header cells.
    pub header: Vec<String>,
    /// The body rows.
    pub rows: Vec<Vec<String>>,
}

/// Removes a leading byte-order mark and reads CRLF as LF.
#[must_use]
pub fn normalise(text: &str) -> Cow<'_, str> {
    let text = strip_bom(text);
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// A parser over `text`, with tables on and byte offsets of every event.
fn parse(text: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
    Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter()
}

/// Adds what `event` says to the inline text in `buf`: text and code as written, line breaks as
/// one space, HTML not at all. Returns whether the event was inline content.
fn push_inline(buf: &mut String, event: &Event<'_>) -> bool {
    match event {
        Event::Text(t) | Event::Code(t) => buf.push_str(t),
        Event::SoftBreak | Event::HardBreak => buf.push(' '),
        _ => return false,
    }
    true
}

/// Every heading with the byte range of its source, in document order.
fn heading_spans(text: &str) -> Vec<(Heading, Range<usize>)> {
    let mut found = Vec::new();
    let mut open: Option<(Heading, Range<usize>)> = None;
    for (event, range) in parse(text) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let heading = Heading {
                    level: level as u8,
                    text: String::new(),
                    line: line_of(text, range.start),
                };
                open = Some((heading, range));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((mut heading, span)) = open.take() {
                    let trimmed = heading.text.trim().to_owned();
                    heading.text = trimmed;
                    found.push((heading, span.start..range.end));
                }
            }
            other => {
                if let Some((heading, _)) = open.as_mut() {
                    push_inline(&mut heading.text, &other);
                }
            }
        }
    }
    found
}

/// Every heading of the text, in document order. A line that looks like a heading inside a code
/// block or an HTML block is none.
#[must_use]
pub fn headings(text: &str) -> Vec<Heading> {
    let text = normalise(text);
    heading_spans(&text)
        .into_iter()
        .map(|(heading, _)| heading)
        .collect()
}

/// One section per heading of exactly `level`: `body` runs from the end of the heading to the
/// start of the next heading whose level is `<= level`, else to the end of the text.
///
/// The body borrows `text`, so only a byte-order mark is removed here; the parser reads CRLF as
/// LF by itself, and the body keeps its line ends.
#[must_use]
pub fn sections(text: &str, level: u8) -> Vec<Section<'_>> {
    let text = strip_bom(text);
    let spans = heading_spans(text);
    let mut found = Vec::new();
    for (index, (heading, span)) in spans.iter().enumerate() {
        if heading.level != level {
            continue;
        }
        let end = spans[index + 1..]
            .iter()
            .find(|(next, _)| next.level <= level)
            .map_or(text.len(), |(_, next_span)| next_span.start);
        found.push(Section {
            heading: heading.clone(),
            body: &text[span.end.min(end)..end],
        });
    }
    found
}

/// Every list item at any depth, in document order, with the inline text of the item itself
/// (the text of its nested items belongs to them).
#[must_use]
pub fn list_items(text: &str) -> Vec<ListItem> {
    let text = normalise(text);
    let mut items: Vec<ListItem> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (event, range) in parse(&text) {
        match event {
            Event::Start(Tag::Item) => {
                open.push(items.len());
                items.push(ListItem {
                    text: String::new(),
                    line: line_of(&text, range.start),
                });
            }
            Event::End(TagEnd::Item) => {
                open.pop();
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(item) = open.last().map(|index| &mut items[*index]) {
                    item.text.push(' ');
                }
            }
            other => {
                if let Some(item) = open.last().map(|index| &mut items[*index]) {
                    push_inline(&mut item.text, &other);
                }
            }
        }
    }
    for item in &mut items {
        item.text = item.text.trim().to_owned();
    }
    items
}

/// Every table, in document order, with its cells as plain text.
#[must_use]
pub fn tables(text: &str) -> Vec<Table> {
    let text = normalise(text);
    let mut found = Vec::new();
    let mut table: Option<Table> = None;
    let mut row: Vec<String> = Vec::new();
    let mut cell: Option<String> = None;
    for (event, range) in parse(&text) {
        match event {
            Event::Start(Tag::Table(_)) => {
                table = Some(Table {
                    line: line_of(&text, range.start),
                    header: Vec::new(),
                    rows: Vec::new(),
                });
            }
            Event::End(TagEnd::Table) => found.extend(table.take()),
            Event::Start(Tag::TableHead | Tag::TableRow) => row.clear(),
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = table.as_mut() {
                    table.header = std::mem::take(&mut row);
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = table.as_mut() {
                    table.rows.push(std::mem::take(&mut row));
                }
            }
            Event::Start(Tag::TableCell) => cell = Some(String::new()),
            Event::End(TagEnd::TableCell) => {
                row.extend(cell.take().map(|text| text.trim().to_owned()));
            }
            other => {
                if let Some(cell) = cell.as_mut() {
                    push_inline(cell, &other);
                }
            }
        }
    }
    found
}

/// The inline code spans, without their backticks, in document order.
#[must_use]
pub fn code_spans(text: &str) -> Vec<String> {
    let text = normalise(text);
    parse(&text)
        .filter_map(|(event, _)| match event {
            Event::Code(code) => Some(code.into_string()),
            _ => None,
        })
        .collect()
}

/// Whether some text or code span has a character that is not whitespace. HTML, comments
/// included, never counts.
#[must_use]
pub fn has_content(text: &str) -> bool {
    let text = normalise(text);
    parse(&text).any(|(event, _)| match event {
        Event::Text(t) | Event::Code(t) => t.chars().any(|c| !c.is_whitespace()),
        _ => false,
    })
}

/// The 1-based line of the byte `offset`: 1 plus the line feeds before it.
#[must_use]
pub fn line_of(text: &str, offset: usize) -> u32 {
    let before = &text.as_bytes()[..offset.min(text.len())];
    let lines = before.split(|byte| *byte == b'\n').count();
    u32::try_from(lines).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(level: u8, text: &str, line: u32) -> Heading {
        Heading {
            level,
            text: text.to_owned(),
            line,
        }
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn headings_come_with_level_text_and_line() {
        let text = "# One\n\nText\n\n## Two *x*\n\n###### Six\n";
        assert_eq!(
            headings(text),
            vec![h(1, "One", 1), h(2, "Two x", 5), h(6, "Six", 7)]
        );
    }

    #[test]
    fn a_heading_in_a_code_block_or_an_html_comment_is_not_one() {
        let text = "# Real\n\n```\n## Gate\n```\n\n<!--\n## Gate\n-->\n\n## After\n";
        assert_eq!(headings(text), vec![h(1, "Real", 1), h(2, "After", 11)]);
    }

    #[test]
    fn a_heading_with_trailing_spaces_or_a_closing_sequence_has_clean_text() {
        assert_eq!(headings("## Gate   \n"), vec![h(2, "Gate", 1)]);
        assert_eq!(headings("## Gate ##\n"), vec![h(2, "Gate", 1)]);
    }

    #[test]
    fn sections_end_at_the_next_heading_of_the_same_or_a_higher_level() {
        let text = "## A\n### B\nb1\n#### C\nc1\n### D\nd1\n## E\ne1\n";
        let found = sections(text, 3);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].heading, h(3, "B", 2));
        assert_eq!(found[0].body, "b1\n#### C\nc1\n");
        assert_eq!(found[1].heading, h(3, "D", 6));
        assert_eq!(found[1].body, "d1\n");
        let last = sections(text, 2);
        assert_eq!(last[1].body, "e1\n");
    }

    #[test]
    fn list_items_join_their_lines() {
        let text = "- TU-90.1 The first\n- TU-90.2 The second,\n  which goes on\n  - nested\n";
        let items = list_items(text);
        let got: Vec<(&str, u32)> = items.iter().map(|i| (i.text.as_str(), i.line)).collect();
        assert_eq!(
            got,
            vec![
                ("TU-90.1 The first", 1),
                ("TU-90.2 The second, which goes on", 2),
                ("nested", 4)
            ]
        );
    }

    #[test]
    fn tables_give_cells_as_plain_text() {
        let text =
            "Intro\n\n| Name | B | OS |\n|---|---|---|\n| `fmt` | x | Linux |\n| **b** | | y |\n";
        let found = tables(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].header, strings(&["Name", "B", "OS"]));
        assert_eq!(
            found[0].rows,
            vec![strings(&["fmt", "x", "Linux"]), strings(&["b", "", "y"])]
        );
    }

    #[test]
    fn code_spans_are_collected() {
        let text = "A `one` and ``two`` here.\n\n```\nnot a span\n```\n\n- `three`\n";
        assert_eq!(code_spans(text), strings(&["one", "two", "three"]));
    }

    #[test]
    fn content_ignores_html_comments_and_whitespace() {
        assert!(!has_content("<!-- x -->\n\n"));
        assert!(!has_content("  \n\n"));
        assert!(has_content("<!-- x -->\n\nText\n"));
        assert!(has_content("```\ncode\n```\n"));
    }

    #[test]
    fn crlf_and_a_byte_order_mark_are_read_as_plain_text() {
        let lf = "# One\n\n- item\n  more\n\n## Two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let crlf = format!("\u{feff}{}", lf.replace('\n', "\r\n"));
        assert_eq!(headings(&crlf), headings(lf));
        assert_eq!(list_items(&crlf), list_items(lf));
        assert_eq!(tables(&crlf), tables(lf));
        let (a, b) = (sections(&crlf, 2), sections(lf, 2));
        assert_eq!(a[0].heading, b[0].heading);
        assert_eq!(a[0].body.replace("\r\n", "\n"), b[0].body);
        assert_eq!(normalise(&crlf), lf);
    }

    #[test]
    fn line_of_counts_line_feeds() {
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 4), 3);
    }
}
