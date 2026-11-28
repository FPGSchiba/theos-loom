//! Behaviour tests of the settings, document, decision and state models.

use loom_core::state::{EpicState, StoryState};
use loom_plan::model::{
    ComponentToml, DecisionKind, DecisionRecord, DocFront, DocStatus, Document, Key, LoomToml,
    Slicing, StateMirror,
};
use loom_plan::{Finding, Rule};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LOOM: &str = include_str!("fixtures/models/loom.toml");
const COMPONENT: &str = include_str!("fixtures/models/component.toml");
const VISION: &str = include_str!("fixtures/models/vision.md");
const DECISION: &str = include_str!("fixtures/models/0003-use-postgres.md");
const STATE: &str = include_str!("fixtures/models/state.toml");

fn ok<T>(result: Result<T, Finding>) -> Result<T, String> {
    result.map_err(|f| f.message)
}

fn unknown_key_in(finding: &Finding, key: &str) {
    assert_eq!(finding.rule, Rule::UnknownKey);
    assert!(finding.message.contains(key), "{}", finding.message);
}

#[test]
fn loom_toml_parses() -> TestResult {
    let loom = ok(LoomToml::parse("loom.toml", LOOM))?;
    assert_eq!(loom.project.id, "shop");
    assert_eq!(loom.project.name, "Online Shop");
    assert_eq!(loom.budget.map(|b| b.weight), Some(3));
    assert_eq!(
        loom.notify.map(|n| n.target).as_deref(),
        Some("team@example.com")
    );
    Ok(())
}

#[test]
fn component_toml_parses() -> TestResult {
    let toml = ok(ComponentToml::parse("storefront/component.toml", COMPONENT))?;
    let c = &toml.component;
    assert_eq!(c.id, "storefront");
    assert_eq!(c.default_branch, "main");
    assert_eq!(c.slicing, Slicing::Review);
    assert_eq!(c.forbidden_paths, ["secrets/**", "*.pem"]);
    assert!(c.kit_version.is_none());
    assert_eq!(toml.validate("storefront/component.toml").len(), 0);
    Ok(())
}

#[test]
fn component_slicing_defaults_to_auto() -> TestResult {
    let text = "format = 1\n[component]\nid = \"a\"\nname = \"A\"\nrepo = \"r\"\ndefault_branch = \"main\"\n";
    let toml = ok(ComponentToml::parse("a/component.toml", text))?;
    assert_eq!(toml.component.slicing, Slicing::Auto);
    assert_eq!(toml.component.forbidden_paths.len(), 0);
    Ok(())
}

#[test]
fn doc_front_parses() -> TestResult {
    let front = ok(DocFront::parse("vision.md", VISION))?;
    assert_eq!(front.and_then(|f| f.status), Some(DocStatus::Draft));
    assert!(
        ok(DocFront::parse(
            "vision.md",
            "# No front matter
"
        ))?
        .is_none()
    );
    Ok(())
}

#[test]
fn document_keeps_front_matter_raw() {
    let doc = Document::from_bytes("vision.md", VISION.as_bytes());
    assert_eq!(
        doc.front_matter.as_deref(),
        Some(
            "status: draft
"
        )
    );
    assert!(
        doc.text
            .as_deref()
            .is_some_and(|t| t.starts_with("# Vision"))
    );
    let binary = Document::from_bytes("x.md", &[0xff, 0xfe, 0x00]);
    assert_eq!(binary.text, None);
}

#[test]
fn decision_record_parses() -> TestResult {
    let rec = ok(DecisionRecord::parse(
        "decisions/0003-use-postgres.md",
        DECISION,
    ))?;
    assert_eq!(rec.number, 3);
    assert_eq!(rec.slug, "use-postgres");
    assert_eq!(rec.front.kind, DecisionKind::Decision);
    assert_eq!(rec.front.date, "2026-03-14");
    Ok(())
}

#[test]
fn decision_bad_file_name_is_a_finding() {
    let err = DecisionRecord::parse("decisions/three-use-postgres.md", DECISION).err();
    assert_eq!(err.map(|f| f.rule), Some(Rule::Parse));
}

#[test]
fn state_mirror_parses() -> TestResult {
    let mirror = ok(StateMirror::parse("_state/storefront/SHOP-1.toml", STATE))?;
    assert_eq!(mirror.epic, Some(EpicState::Active));
    assert_eq!(mirror.stories.len(), 2);
    assert_eq!(
        mirror.stories.get(&"SHOP-12".parse::<Key>()?),
        Some(&StoryState::Merged)
    );
    let backlog = ok(StateMirror::parse(
        "_state/storefront/backlog.toml",
        "format = 1\n",
    ))?;
    assert!(backlog.epic.is_none() && backlog.stories.is_empty());
    Ok(())
}

// AC: TU-01.1
#[test]
fn unknown_key_in_loom_toml_is_named() {
    let text = format!("{LOOM}colour = \"red\"\n");
    let err = LoomToml::parse("loom.toml", &text).err();
    assert!(err.is_some());
    if let Some(f) = err {
        unknown_key_in(&f, "colour");
    }
}

// AC: TU-01.1
#[test]
fn unknown_key_in_component_toml_is_named() {
    let text = COMPONENT.replace("slicing", "slicer");
    let err = ComponentToml::parse("c/component.toml", &text).err();
    assert!(err.is_some());
    if let Some(f) = err {
        unknown_key_in(&f, "slicer");
    }
}

// AC: TU-01.1
#[test]
fn unknown_key_in_doc_front_is_named() {
    let err = DocFront::parse("vision.md", "---\nstatus: draft\nowner: me\n---\nbody\n").err();
    assert!(err.is_some());
    if let Some(f) = err {
        unknown_key_in(&f, "owner");
    }
}

// AC: TU-01.1
#[test]
fn unknown_key_in_decision_front_is_named() {
    let text = DECISION.replace("kind:", "mood: calm\nkind:");
    let err = DecisionRecord::parse("d/0003-use-postgres.md", &text).err();
    assert!(err.is_some());
    if let Some(f) = err {
        unknown_key_in(&f, "mood");
    }
}

// AC: TU-01.1
#[test]
fn unknown_key_in_state_mirror_is_named() {
    // At the top: after `[stories]` it would be a story key.
    let text = format!("extra = 1\n{STATE}");
    let err = StateMirror::parse("_state/c/e.toml", &text).err();
    assert!(err.is_some());
    if let Some(f) = err {
        unknown_key_in(&f, "extra");
    }
}

#[test]
fn format_2_is_refused() {
    let err = LoomToml::parse("loom.toml", &LOOM.replace("format = 1", "format = 2")).err();
    assert!(err.is_some());
    if let Some(f) = err {
        assert_eq!(f.rule, Rule::Parse);
        assert!(
            f.message
                .contains("format 2 is not supported; Loom reads format 1"),
            "{}",
            f.message
        );
    }
}

#[test]
fn bad_forbidden_glob_is_a_finding() -> TestResult {
    let text = COMPONENT.replace("\"*.pem\"", "\"[unclosed\"");
    let toml = ok(ComponentToml::parse("c/component.toml", &text))?;
    let findings = toml.validate("c/component.toml");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, Rule::Parse);
    assert!(findings[0].message.contains("[unclosed"));
    Ok(())
}

#[test]
fn decision_date_must_be_iso() {
    for bad in ["14.03.2026", "2026-13-01", "2026-3-4", "tomorrow"] {
        let text = DECISION.replace("2026-03-14", bad);
        let err = DecisionRecord::parse("d/0003-use-postgres.md", &text).err();
        assert_eq!(err.map(|f| f.rule), Some(Rule::Parse), "{bad}");
    }
}
