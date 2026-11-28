//! Walking the top-level lists and paragraphs of a Markdown text with `pulldown-cmark`.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::front_matter::skip_line_end;
use super::line_at;

/// One item of a top-level list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    /// The number as written in the item's marker; `0` for a bullet list.
    pub number: u32,
    /// The item text, marker removed, every line trimmed.
    pub text: String,
    /// File line of the first line of the item.
    pub line: u32,
    /// The text of a bold span that opens the item.
    pub name: Option<String>,
}

/// A top-level list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct List {
    /// True for an ordered list.
    pub ordered: bool,
    /// The items.
    pub items: Vec<Item>,
}

/// A top-level paragraph: its trimmed text and the file line of its first line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Paragraph {
    /// The text, every line trimmed.
    pub text: String,
    /// File line of the first line.
    pub line: u32,
}

/// What [`walk`] finds.
#[derive(Debug, Clone, Default)]
pub(crate) struct Walk {
    /// Lists that are not nested in another list.
    pub lists: Vec<List>,
    /// Paragraphs that are not inside a list.
    pub paragraphs: Vec<Paragraph>,
}

/// Where the opening bold span of an item stands.
enum NameState {
    Waiting,
    Collecting(String),
    Done(Option<String>),
}

struct Open {
    number: u32,
    first: Option<usize>,
    name: NameState,
}

/// Walks `text`; `first_line` is the file line of its first line.
pub(crate) fn walk(text: &str, first_line: u32) -> Walk {
    let file_line = |offset: usize| line_at(text, offset) + first_line - 1;
    let mut out = Walk::default();
    let mut depth = 0usize;
    let mut next_number = 0u32;
    let mut open: Option<Open> = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        if let Some(item) = open.as_mut() {
            if item.first.is_none() && !matches!(event, Event::End(TagEnd::Item)) {
                item.first = Some(range.start);
            }
            item.name = advance(
                std::mem::replace(&mut item.name, NameState::Waiting),
                &event,
            );
        }
        match event {
            Event::Start(Tag::List(start)) => {
                if depth == 0 {
                    out.lists.push(List {
                        ordered: start.is_some(),
                        items: Vec::new(),
                    });
                    next_number = start.map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
                }
                depth += 1;
            }
            Event::End(TagEnd::List(_)) => depth = depth.saturating_sub(1),
            Event::Start(Tag::Item) if depth == 1 => {
                let number = written_number(&text[range.start..]).unwrap_or(next_number);
                open = Some(Open {
                    number,
                    first: None,
                    name: NameState::Waiting,
                });
                next_number = number.saturating_add(1);
            }
            Event::End(TagEnd::Item) if depth == 1 => {
                if let (Some(item), Some(list)) = (open.take(), out.lists.last_mut()) {
                    let start = item.first.unwrap_or(range.start);
                    list.items.push(Item {
                        number: item.number,
                        text: clean(&text[start..range.end.max(start)]),
                        line: file_line(start),
                        name: match item.name {
                            NameState::Done(name) => name,
                            _ => None,
                        },
                    });
                }
            }
            Event::Start(Tag::Paragraph) if depth == 0 => {
                out.paragraphs.push(Paragraph {
                    text: clean(&text[range.clone()]),
                    line: file_line(range.start),
                });
            }
            _ => {}
        }
    }
    out
}

/// The number of an ordered item's marker as written (`2.` or `2)`), which `pulldown-cmark`
/// does not keep: it numbers items on from the list's start. `None` for a bullet item.
fn written_number(item: &str) -> Option<u32> {
    let digits = item.bytes().take_while(u8::is_ascii_digit).count();
    item.get(..digits)?.parse().ok()
}

/// Moves the opening-bold-span state on by one event.
fn advance(state: NameState, event: &Event<'_>) -> NameState {
    match state {
        NameState::Waiting => match event {
            Event::Start(Tag::Paragraph) => NameState::Waiting,
            Event::Start(Tag::Strong) => NameState::Collecting(String::new()),
            _ => NameState::Done(None),
        },
        NameState::Collecting(mut name) => match event {
            Event::Text(t) | Event::Code(t) => {
                name.push_str(t);
                NameState::Collecting(name)
            }
            Event::End(TagEnd::Strong) => {
                let name = name.trim().trim_end_matches('.').trim_end().to_owned();
                NameState::Done((!name.is_empty()).then_some(name))
            }
            _ => NameState::Collecting(name),
        },
        done @ NameState::Done(_) => done,
    }
}

/// Trims every line and the whole text.
fn clean(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().map(str::trim).collect();
    lines.join("\n").trim().to_owned()
}

/// The byte offset where the text after a heading starts.
pub(crate) fn after_line(text: &str, end: usize) -> usize {
    let end = end.min(text.len());
    if text[..end].ends_with('\n') {
        end
    } else {
        skip_line_end(text, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_finds_top_level_lists_and_paragraphs() {
        let w = walk("Intro\nline\n\n1. **A.** x\n   - nested\n2. y\n\n- b\n", 10);
        assert_eq!(w.paragraphs.len(), 1);
        assert_eq!(w.paragraphs[0].line, 10);
        assert_eq!(w.lists.len(), 2);
        assert!(w.lists[0].ordered && !w.lists[1].ordered);
        assert_eq!(w.lists[0].items.len(), 2);
        assert_eq!(w.lists[0].items[0].name.as_deref(), Some("A"));
        assert_eq!(w.lists[0].items[1].number, 2);
        assert_eq!(w.lists[0].items[0].line, 13);
    }

    #[test]
    fn item_numbers_are_read_as_written_not_renumbered() {
        let w = walk("1. a\n2. b\n2. c\n7) d\n", 1);
        let numbers: Vec<u32> = w
            .lists
            .iter()
            .flat_map(|l| &l.items)
            .map(|i| i.number)
            .collect();
        assert_eq!(numbers, [1, 2, 2, 7]);
    }
}
