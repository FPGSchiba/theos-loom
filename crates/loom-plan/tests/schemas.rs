//! Behaviour tests of the generated JSON schemas.

use std::fs;
use std::path::Path;

use loom_plan::model::{AfterRef, Key, TicketId};
use loom_plan::schema::{front_matter_json, schemas, write_schemas};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const TEMPLATES: &str = "tests/fixtures/po-templates";

fn schema_value(name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let (_, schema) = schemas()
        .into_iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no schema named {name}"))?;
    Ok(serde_json::to_value(&schema)?)
}

fn read_normalised(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(path)?.replace("\r\n", "\n"))
}

// AC: TU-01.4
#[test]
fn committed_schemas_equal_generated_ones() -> TestResult {
    let generated = Path::new(env!("CARGO_TARGET_TMPDIR")).join("schemas");
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas");
    fs::create_dir_all(&generated)?;
    let written = write_schemas(&generated)?;
    assert_eq!(written.len(), schemas().len());
    for path in &written {
        let name = path.file_name().ok_or("no file name")?;
        let committed_file = committed.join(name);
        // Compared as text with CRLF turned into LF, so autocrlf on Windows cannot flake it.
        let same =
            committed_file.exists() && read_normalised(&committed_file)? == read_normalised(path)?;
        assert!(
            same,
            "{} differs from the generated schema; copy the folder {} over {} (or run \
             `cargo run -p loom -- plan schemas crates/loom-plan/schemas` once that command exists)",
            committed_file.display(),
            generated.display(),
            committed.display()
        );
    }
    Ok(())
}

// AC: TU-01.4
#[test]
fn every_front_matter_type_has_a_schema() {
    let names: Vec<&str> = schemas().into_iter().map(|(n, _)| n).collect();
    for want in [
        "initiative",
        "epic",
        "leaf",
        "plan",
        "document-front-matter",
        "decision",
        "loom-toml",
        "component-toml",
        "tickets-toml",
        "state-mirror",
    ] {
        assert!(names.contains(&want), "missing schema {want}");
    }
}

// AC: TU-01.4
#[test]
fn po_templates_validate_against_their_schemas() -> TestResult {
    let md = [
        ("initiative.md", "initiative"),
        ("epic.md", "epic"),
        ("story.md", "leaf"),
        ("spike.md", "leaf"),
        ("bug.md", "leaf"),
        ("plan.md", "plan"),
    ];
    for (file, schema) in md {
        let text = fs::read_to_string(Path::new(TEMPLATES).join(file))?;
        let value =
            front_matter_json(&text)?.ok_or_else(|| format!("{file} has no front matter"))?;
        let validator = jsonschema::validator_for(&schema_value(schema)?)?;
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{file} against {schema}: {errors:?}");
    }
    for file in [
        "tickets-epics.toml",
        "tickets-entries.toml",
        "tickets-initiatives.toml",
    ] {
        let text = fs::read_to_string(Path::new(TEMPLATES).join(file))?;
        let value: Value = toml::from_str(&text)?;
        let validator = jsonschema::validator_for(&schema_value("tickets-toml")?)?;
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{file}: {errors:?}");
    }
    Ok(())
}

// AC: TU-01.4
#[test]
fn unknown_key_fails_schema_validation() -> TestResult {
    let text = fs::read_to_string(Path::new(TEMPLATES).join("epic.md"))?;
    let mut value = front_matter_json(&text)?.ok_or("no front matter")?;
    let validator = jsonschema::validator_for(&schema_value("epic")?)?;
    assert!(validator.is_valid(&value));
    value["surprise"] = json!("x");
    assert!(!validator.is_valid(&value));
    Ok(())
}

// AC: TU-01.4
#[test]
fn deny_unknown_fields_gives_additional_properties_false() -> TestResult {
    for name in [
        "initiative",
        "epic",
        "leaf",
        "plan",
        "document-front-matter",
        "decision",
    ] {
        let value = schema_value(name)?;
        assert_eq!(value["additionalProperties"], json!(false), "{name}");
    }
    Ok(())
}

// AC: TU-01.4
#[test]
fn id_patterns_agree_between_rust_and_schema() -> TestResult {
    let definitions = |name: &str, def: &str| -> Result<Value, Box<dyn std::error::Error>> {
        let v = schema_value(name)?;
        Ok(v["$defs"][def].clone())
    };
    let key = jsonschema::validator_for(&definitions("epic", "Key")?)?;
    let ticket = jsonschema::validator_for(&definitions("tickets-toml", "TicketId")?)?;
    let after = jsonschema::validator_for(&definitions("tickets-toml", "AfterRef")?)?;
    // YAML `1.3` unquoted is a JSON number; the Rust side reads its text, "1.3".
    assert!(after.is_valid(&json!(1.3)), "numeric 1.3");
    assert!("1.3".parse::<AfterRef>().is_ok());
    for s in [
        "ED-12", "A-1", "A1B-007", "ed-12", "ED-", "-12", "ED12", "1A-2", "ED-1x", "", "E D-1",
    ] {
        assert_eq!(
            s.parse::<Key>().is_ok(),
            key.is_valid(&json!(s)),
            "key {s:?}"
        );
    }
    for s in ["6", "2b", "abc", "", "1.3", "a-b", "é"] {
        assert_eq!(
            TicketId::try_from(s.to_owned()).is_ok(),
            ticket.is_valid(&json!(s)),
            "ticket id {s:?}"
        );
    }
    for s in [
        "3",
        "2.3",
        "2b.3c",
        "epic-cart",
        "epic-",
        "1.",
        ".1",
        "1.2.3",
        "a b",
        "",
        "epic",
    ] {
        assert_eq!(
            s.parse::<AfterRef>().is_ok(),
            after.is_valid(&json!(s)),
            "after {s:?}"
        );
    }
    Ok(())
}

// AC: TU-01.4
#[test]
fn unquoted_cross_epic_after_validates_against_leaf_and_epic() -> TestResult {
    for (file, schema, old) in [
        (
            "story.md",
            "leaf",
            r#"after: [2, "1.3", "epic-pricing-rules"]"#,
        ),
        ("epic.md", "epic", r#"after: ["epic-pricing-rules"]"#),
    ] {
        let text = fs::read_to_string(Path::new(TEMPLATES).join(file))?;
        assert!(text.contains(old), "{file} lost its after line");
        let text = text.replace(old, "after: [2, 1.3, epic-x]");
        let value = front_matter_json(&text)?.ok_or("no front matter")?;
        assert!(value["after"][1].is_number(), "1.3 should read as a number");
        let validator = jsonschema::validator_for(&schema_value(schema)?)?;
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{file} against {schema}: {errors:?}");
    }
    Ok(())
}
