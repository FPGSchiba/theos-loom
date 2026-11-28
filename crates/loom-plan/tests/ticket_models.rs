//! Behaviour tests of the ticket-tree models, against the filled PO templates.

use loom_plan::model::{
    AfterRef, Breakdown, EpicAfter, EpicFront, InitiativeAfter, InitiativeFront, LeafFront,
    LeafKind, LoomMark, Origin, PlanFront, PlanStatus, Severity, TicketId,
};
use loom_plan::{Finding, Rule};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const INITIATIVE: &str = include_str!("fixtures/po-templates/initiative.md");
const EPIC: &str = include_str!("fixtures/po-templates/epic.md");
const STORY: &str = include_str!("fixtures/po-templates/story.md");
const SPIKE: &str = include_str!("fixtures/po-templates/spike.md");
const BUG: &str = include_str!("fixtures/po-templates/bug.md");
const PLAN: &str = include_str!("fixtures/po-templates/plan.md");
const EPICS: &str = include_str!("fixtures/po-templates/tickets-epics.toml");
const ENTRIES: &str = include_str!("fixtures/po-templates/tickets-entries.toml");
const INITIATIVES: &str = include_str!("fixtures/po-templates/tickets-initiatives.toml");

fn ok<T>(result: Result<T, Finding>) -> Result<T, String> {
    result.map_err(|f| f.message)
}

fn some<T>(value: Option<T>) -> Result<T, String> {
    value.ok_or_else(|| "no front matter".to_owned())
}

fn unknown_key_in(finding: &Finding, key: &str, path: &str) {
    assert_eq!(finding.rule, Rule::UnknownKey);
    assert!(finding.message.contains(key), "{}", finding.message);
    assert!(finding.message.contains(path), "{}", finding.message);
}

/// The text with a bogus key added to its front matter.
fn with_front_key(text: &str) -> String {
    text.replacen("---\n", "---\nbogus_key: 1\n", 1)
}

fn tid(s: &str) -> TicketId {
    TicketId::try_from(s.to_owned()).unwrap_or_else(|_| unreachable!())
}

#[test]
fn every_po_template_parses() -> TestResult {
    let initiative = some(ok(InitiativeFront::parse("initiative.md", INITIATIVE))?)?;
    assert_eq!(initiative.title, "Shoppers buy online");
    let epic = some(ok(EpicFront::parse("epic.md", EPIC))?)?;
    assert_eq!(epic.key.map(|k| k.to_string()).as_deref(), Some("SHOP-1"));
    assert_eq!(epic.loom, Some(LoomMark::Ready));
    assert_eq!(epic.needs.len(), 1);
    assert_eq!(
        epic.after,
        [AfterRef::Epic("epic-pricing-rules".to_owned())]
    );
    let story = some(ok(LeafFront::parse("story.md", STORY))?)?;
    assert_eq!(story.kind, LeafKind::Story);
    assert_eq!(story.origin, Some(Origin::Sliced));
    assert_eq!(story.after.len(), 3);
    let sp = some(ok(LeafFront::parse("spike.md", SPIKE))?)?;
    assert_eq!(sp.kind, LeafKind::Spike);
    assert_eq!(sp.loom, Some(LoomMark::Paused));
    let bug = some(ok(LeafFront::parse("bug.md", BUG))?)?;
    assert_eq!(bug.severity, Some(Severity::P1));
    let plan = some(ok(PlanFront::parse("plan.md", PLAN))?)?;
    assert_eq!(plan.status, Some(PlanStatus::Blocked));
    assert_eq!(plan.ticket.to_string(), "1");

    let epics = ok(Breakdown::parse("tickets.toml", EPICS))?;
    assert_eq!(
        (epics.kinds(), epics.epic.len(), epics.next_id),
        (1, 2, Some(3))
    );
    assert!(matches!(epics.epic[1].after[0], EpicAfter::Needs(_)));
    assert!(matches!(
        epics.epic[1].after[1],
        EpicAfter::Ref(AfterRef::Epic(_))
    ));
    assert!(matches!(
        epics.epic[1].after[2],
        EpicAfter::Ref(AfterRef::Sibling(_))
    ));
    let entries = ok(Breakdown::parse("tickets.toml", ENTRIES))?;
    assert_eq!((entries.kinds(), entries.entry.len()), (1, 2));
    assert_eq!(entries.entry[1].id, tid("2"));
    assert_eq!(entries.entry[0].needs.len(), 1);
    let inits = ok(Breakdown::parse("tickets.toml", INITIATIVES))?;
    assert_eq!((inits.kinds(), inits.initiative.len()), (1, 2));
    assert!(matches!(
        inits.initiative[1].after[0],
        InitiativeAfter::Needs(_)
    ));
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_initiative_is_named() -> TestResult {
    let f = InitiativeFront::parse("initiative.md", &with_front_key(INITIATIVE))
        .err()
        .ok_or("expected an error")?;
    unknown_key_in(&f, "bogus_key", "initiative.md");
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_epic_is_named() -> TestResult {
    let f = EpicFront::parse("epic.md", &with_front_key(EPIC))
        .err()
        .ok_or("expected an error")?;
    unknown_key_in(&f, "bogus_key", "epic.md");
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_leaf_is_named() -> TestResult {
    let f = LeafFront::parse("story.md", &with_front_key(STORY))
        .err()
        .ok_or("expected an error")?;
    unknown_key_in(&f, "bogus_key", "story.md");
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_plan_is_named() -> TestResult {
    let f = PlanFront::parse("plan.md", &with_front_key(PLAN))
        .err()
        .ok_or("expected an error")?;
    unknown_key_in(&f, "bogus_key", "plan.md");
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_tickets_toml_is_named() -> TestResult {
    for (text, table) in [
        (ENTRIES, "[[entry]]"),
        (EPICS, "[[epic]]"),
        (INITIATIVES, "[[initiative]]"),
    ] {
        let bad = text.replacen(table, &format!("{table}\nbogus_key = 1"), 1);
        let f = Breakdown::parse("tickets.toml", &bad)
            .err()
            .ok_or("expected an error")?;
        unknown_key_in(&f, "bogus_key", "tickets.toml");
    }
    let f = Breakdown::parse("tickets.toml", "bogus_top = 1\n")
        .err()
        .ok_or("expected an error")?;
    unknown_key_in(&f, "bogus_top", "tickets.toml");
    Ok(())
}

#[test]
fn bad_key_form_is_a_parse_finding() -> TestResult {
    let bad = EPIC.replace("key: SHOP-1", "key: ed-1");
    let f = EpicFront::parse("epic.md", &bad)
        .err()
        .ok_or("expected an error")?;
    assert_eq!(f.rule, Rule::Parse);
    assert!(f.message.contains("ed-1"), "{}", f.message);
    Ok(())
}

#[test]
fn loom_mark_accepts_ready_and_paused_only() -> TestResult {
    let bad = EPIC.replace("loom: ready", "loom: go");
    let f = EpicFront::parse("epic.md", &bad)
        .err()
        .ok_or("expected an error")?;
    assert_eq!(f.rule, Rule::Parse);
    let paused = EPIC.replace("loom: ready", "loom: paused");
    let e = some(ok(EpicFront::parse("epic.md", &paused))?)?;
    assert_eq!(e.loom, Some(LoomMark::Paused));
    Ok(())
}

#[test]
fn yaml_integer_id_reads_like_the_string_id() -> TestResult {
    let int = some(ok(LeafFront::parse("a.md", STORY))?)?;
    let string = some(ok(LeafFront::parse(
        "a.md",
        &STORY.replace("id: 1\n", "id: \"1\"\n"),
    ))?)?;
    assert_eq!(int.id, string.id);
    assert_eq!(int.id, Some(tid("1")));
    Ok(())
}

#[test]
fn two_kinds_of_table_are_counted_not_refused() -> TestResult {
    let text = format!("{ENTRIES}\n{EPICS}").replace("next_id = 3\n", "");
    let b = ok(Breakdown::parse("tickets.toml", &text))?;
    assert_eq!(b.kinds(), 2);
    assert_eq!(Breakdown::default().kinds(), 0);
    Ok(())
}
