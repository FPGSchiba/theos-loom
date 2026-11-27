//! The files that hold the GitHub settings (`gate/ruleset-*.json`, `gate/repo-settings.toml`) and
//! how a live repository is compared with them. The tagged tests keep the files and
//! `docs/gate.md` §1 and §6 saying the same; the others read the comparison on small changes of
//! a live repository that starts out equal to the files.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xtask::github::LabelSpec;
use xtask::github::settings::{
    Desired, Live, compare, differences, required_contexts_from_gate_md,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The id of the GitHub Actions app, which every required check is pinned to.
const ACTIONS_APP: i64 = 15_368;

fn root() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?
        .to_path_buf())
}

fn ruleset<'a>(desired: &'a Desired, name: &str) -> TestResult<&'a Value> {
    desired
        .rulesets
        .iter()
        .find(|ruleset| ruleset["name"] == name)
        .ok_or_else(|| format!("no ruleset called {name}").into())
}

/// The rule of the type `kind` in `ruleset`.
fn rule<'a>(ruleset: &'a Value, kind: &str) -> TestResult<&'a Value> {
    ruleset["rules"]
        .as_array()
        .and_then(|rules| rules.iter().find(|rule| rule["type"] == kind))
        .ok_or_else(|| format!("no rule of type {kind}").into())
}

/// The types of all rules of `ruleset`.
fn rule_types(ruleset: &Value) -> TestResult<BTreeSet<String>> {
    let rules = ruleset["rules"].as_array().ok_or("rules is not a list")?;
    rules
        .iter()
        .map(|rule| {
            let kind = rule["type"].as_str().ok_or("a rule has no type")?;
            Ok(kind.to_owned())
        })
        .collect()
}

// AC: TU-00.2
#[test]
fn ruleset_contexts_equal_gate_md_checks() -> TestResult {
    let root = root()?;
    let desired = Desired::load(&root)?;
    let main = ruleset(&desired, "main")?;
    let rule = rule(main, "required_status_checks")?;
    let checks = rule["parameters"]["required_status_checks"]
        .as_array()
        .ok_or("required_status_checks is not a list")?;
    let mut contexts = BTreeSet::new();
    for check in checks {
        let context = check["context"].as_str().ok_or("a check has no context")?;
        assert_eq!(
            check["integration_id"], ACTIONS_APP,
            "{context} is not pinned to the GitHub Actions app"
        );
        contexts.insert(context.to_owned());
    }
    assert_eq!(contexts.len(), checks.len(), "a context is listed twice");

    let gate_md = fs::read_to_string(root.join("docs").join("gate.md"))?;
    let required = required_contexts_from_gate_md(&gate_md);
    assert_eq!(contexts, required);
    let expected: BTreeSet<String> = [
        "fmt",
        "clippy",
        "test",
        "slow-tests",
        "docs",
        "deny",
        "engine-free",
        "coverage",
        "lints",
        "ac-trace",
        "commit-msg",
        "pr-title",
        "pr-body",
        "approval",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(required, expected, "docs/gate.md §1 lists other checks");
    Ok(())
}

// AC: TU-00.2
#[test]
fn main_ruleset_encodes_section_6() -> TestResult {
    let desired = Desired::load(&root()?)?;
    let main = ruleset(&desired, "main")?;
    assert_eq!(main["target"], "branch");
    assert_eq!(main["enforcement"], "active");
    // No bypass: the rules hold for administrators too.
    assert_eq!(main["bypass_actors"], json!([]));
    let included = &main["conditions"]["ref_name"];
    assert_eq!(included["include"], json!(["~DEFAULT_BRANCH"]));
    assert_eq!(included["exclude"], json!([]));

    // No force push (`non_fast_forward`) and no deletion; linear history; a pull request for
    // every change; the checks.
    let expected: BTreeSet<String> = [
        "deletion",
        "non_fast_forward",
        "required_linear_history",
        "pull_request",
        "required_status_checks",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(rule_types(main)?, expected);

    // Squash merge only; no approval that auto-merge would wait for.
    let pull_request = &rule(main, "pull_request")?["parameters"];
    assert_eq!(pull_request["allowed_merge_methods"], json!(["squash"]));
    assert_eq!(pull_request["required_approving_review_count"], 0);

    // Up to date with `main` before merging.
    let checks = &rule(main, "required_status_checks")?["parameters"];
    assert_eq!(checks["strict_required_status_checks_policy"], true);
    assert_eq!(checks["do_not_enforce_on_create"], false);
    Ok(())
}

// AC: TU-00.2
#[test]
fn tag_ruleset_blocks_update_and_deletion() -> TestResult {
    let desired = Desired::load(&root()?)?;
    let tags = ruleset(&desired, "tags")?;
    assert_eq!(tags["target"], "tag");
    assert_eq!(tags["enforcement"], "active");
    assert_eq!(tags["bypass_actors"], json!([]));
    let names = &tags["conditions"]["ref_name"];
    assert_eq!(names["include"], json!(["refs/tags/v*"]));
    assert_eq!(names["exclude"], json!([]));
    let expected: BTreeSet<String> = ["update", "deletion"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(rule_types(tags)?, expected);
    Ok(())
}

// AC: TU-00.2
#[test]
fn repo_settings_encode_section_6() -> TestResult {
    let desired = Desired::load(&root()?)?;
    let repo = serde_json::to_value(&desired.repo)?;
    assert_eq!(
        repo,
        json!({
            // Squash merge only.
            "allow_squash_merge": true,
            "allow_merge_commit": false,
            "allow_rebase_merge": false,
            // The commit on `main` keeps the trailers of the commits.
            "squash_merge_commit_title": "PR_TITLE",
            "squash_merge_commit_message": "COMMIT_MESSAGES",
            // Auto-merge, branch deleted after merge, branches can be brought up to date.
            "allow_auto_merge": true,
            "delete_branch_on_merge": true,
            "allow_update_branch": true,
        })
    );
    let label = |name: &str, color: &str, description: &str| LabelSpec {
        name: name.to_owned(),
        color: color.to_owned(),
        description: description.to_owned(),
    };
    assert_eq!(
        desired.labels,
        [
            label(
                "needs-jann",
                "d93f0b",
                "Touches a protected path; waits for Jann"
            ),
            label(
                "approved",
                "0e8a16",
                "Reviewed by Jann; only Jann adds this label"
            ),
            label(
                "windows",
                "1d76db",
                "Run the Windows jobs of the platforms workflow on this pull request"
            ),
            label(
                "macos",
                "5319e7",
                "Run the macOS jobs of the platforms workflow on this pull request"
            ),
        ]
    );
    Ok(())
}

#[test]
fn required_contexts_are_the_first_column_of_section_1_without_ui() {
    let text = "# The gate

## 1. Required checks

| Check | What it runs | OS |
|---|---|---|
| `fmt` | `cargo fmt --all --check` | Linux |
| `clippy` | `cargo clippy` | Linux |
| `ui` | from P11: `pnpm install` | Linux |
| `docs` | `cargo doc` with warnings | Linux |

Text after the table, and `not-a-check` in it.

## 2. Lints

| Check | What it runs | OS |
|---|---|---|
| `other` | not in section 1 | Linux |
";
    let expected: BTreeSet<String> = ["fmt", "clippy", "docs"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(required_contexts_from_gate_md(text), expected);
    assert_eq!(
        required_contexts_from_gate_md("no table here"),
        BTreeSet::new()
    );
}

/// `value` with every list reversed and an extra key on every object: what GitHub may answer for
/// the same settings.
fn answered(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().rev().map(answered).collect()),
        Value::Object(map) => {
            let mut answer: serde_json::Map<String, Value> = map
                .iter()
                .rev()
                .map(|(key, value)| (key.clone(), answered(value)))
                .collect();
            answer.insert("zz_extra".to_owned(), json!(["only", "on", "github"]));
            Value::Object(answer)
        }
        other => other.clone(),
    }
}

/// A live repository that has exactly what the files say.
fn live_like(desired: &Desired) -> TestResult<Live> {
    Ok(Live {
        rulesets: desired.rulesets.clone(),
        repo: serde_json::to_value(&desired.repo)?,
        labels: desired.labels.clone(),
    })
}

fn load() -> TestResult<(Desired, Live)> {
    let desired = Desired::load(&root()?)?;
    let live = live_like(&desired)?;
    Ok((desired, live))
}

/// The live ruleset called `name`, for a change.
fn live_ruleset<'a>(live: &'a mut Live, name: &str) -> TestResult<&'a mut Value> {
    live.rulesets
        .iter_mut()
        .find(|ruleset| ruleset["name"] == name)
        .ok_or_else(|| format!("no live ruleset called {name}").into())
}

#[test]
fn differences_ignore_extra_live_keys_and_order() -> TestResult {
    let (desired, mut live) = load()?;
    assert_eq!(differences(&desired, &live), Vec::<String>::new());

    live.rulesets = live.rulesets.iter().rev().map(answered).collect();
    live.repo = answered(&live.repo);
    live.labels.reverse();
    live.labels.push(LabelSpec {
        name: "bug".to_owned(),
        color: "d73a4a".to_owned(),
        description: "Something is not working".to_owned(),
    });
    let comparison = compare(&desired, &live);
    assert_eq!(comparison.differences, Vec::<String>::new());
    assert_eq!(comparison.notes, Vec::<String>::new());
    Ok(())
}

#[test]
fn missing_ruleset_is_a_difference() -> TestResult {
    let (desired, mut live) = load()?;
    live.rulesets.retain(|ruleset| ruleset["name"] != "main");
    assert_eq!(
        differences(&desired, &live),
        ["ruleset main: not on GitHub"]
    );
    live.rulesets.clear();
    assert_eq!(
        differences(&desired, &live),
        ["ruleset main: not on GitHub", "ruleset tags: not on GitHub"]
    );
    Ok(())
}

#[test]
fn a_changed_value_names_both_sides() -> TestResult {
    let (desired, mut live) = load()?;
    live_ruleset(&mut live, "main")?["enforcement"] = json!("disabled");
    live_ruleset(&mut live, "tags")?["conditions"]["ref_name"]["include"] = json!(["refs/tags/*"]);
    assert_eq!(
        differences(&desired, &live),
        [
            r#"ruleset main: enforcement is "disabled", expected "active""#,
            r#"ruleset tags: conditions.ref_name.include is ["refs/tags/*"], expected ["refs/tags/v*"]"#,
        ]
    );
    Ok(())
}

#[test]
fn ruleset_key_that_differs_is_a_difference() -> TestResult {
    let (desired, mut live) = load()?;
    let actor = json!({"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"});
    live_ruleset(&mut live, "main")?["bypass_actors"] = json!([actor.clone()]);
    assert_eq!(
        differences(&desired, &live),
        [format!(
            "ruleset main: bypass_actors is [{actor}], expected []"
        )]
    );
    Ok(())
}

#[test]
fn ruleset_key_hidden_from_the_token_is_not_checked() -> TestResult {
    let (desired, mut live) = load()?;
    // A token without admin rights gets a ruleset without its bypass actors.
    live_ruleset(&mut live, "main")?
        .as_object_mut()
        .ok_or("not an object")?
        .remove("bypass_actors");
    let comparison = compare(&desired, &live);
    assert_eq!(comparison.differences, Vec::<String>::new());
    assert_eq!(
        comparison.notes,
        ["ruleset main: bypass_actors: not checked, GitHub did not return it"]
    );
    Ok(())
}

#[test]
fn a_key_hidden_deeper_in_a_ruleset_is_not_checked_either() -> TestResult {
    let (desired, mut live) = load()?;
    let tags = live_ruleset(&mut live, "tags")?;
    tags["conditions"]["ref_name"]
        .as_object_mut()
        .ok_or("not an object")?
        .remove("exclude");
    let main = live_ruleset(&mut live, "main")?;
    let rules = main["rules"].as_array_mut().ok_or("rules is not a list")?;
    for rule in rules
        .iter_mut()
        .filter(|rule| rule["type"] == "pull_request")
    {
        rule["parameters"]
            .as_object_mut()
            .ok_or("not an object")?
            .remove("allowed_merge_methods");
    }
    let comparison = compare(&desired, &live);
    assert_eq!(comparison.differences, Vec::<String>::new());
    assert_eq!(
        comparison.notes,
        [
            "ruleset main: rule pull_request: parameters.allowed_merge_methods: not checked, \
             GitHub did not return it",
            "ruleset tags: conditions.ref_name.exclude: not checked, GitHub did not return it",
        ]
    );
    Ok(())
}

#[test]
fn rules_are_a_set_keyed_by_type() -> TestResult {
    let (desired, mut live) = load()?;
    let main = live_ruleset(&mut live, "main")?;
    let rules = main["rules"].as_array_mut().ok_or("rules is not a list")?;
    rules.retain(|rule| rule["type"] != "deletion");
    rules.push(json!({"type": "creation"}));
    for rule in rules
        .iter_mut()
        .filter(|rule| rule["type"] == "pull_request")
    {
        rule["parameters"]["allowed_merge_methods"] = json!(["squash", "merge"]);
        rule["parameters"]["required_approving_review_count"] = json!(1);
    }
    assert_eq!(
        differences(&desired, &live),
        [
            "ruleset main: rule deletion is missing",
            "ruleset main: rule pull_request: parameters.allowed_merge_methods is \
             [\"squash\",\"merge\"], expected [\"squash\"]",
            "ruleset main: rule pull_request: parameters.required_approving_review_count is 1, \
             expected 0",
            "ruleset main: rule creation is on GitHub but not in the file",
        ]
    );
    Ok(())
}

#[test]
fn required_checks_are_a_set_of_context_and_app() -> TestResult {
    let (desired, mut live) = load()?;
    let main = live_ruleset(&mut live, "main")?;
    let rules = main["rules"].as_array_mut().ok_or("rules is not a list")?;
    for rule in rules
        .iter_mut()
        .filter(|rule| rule["type"] == "required_status_checks")
    {
        let checks = rule["parameters"]["required_status_checks"]
            .as_array_mut()
            .ok_or("checks is not a list")?;
        // `docs` is gone, `fmt` is pinned to another app, `lint` is new.
        checks.retain(|check| check["context"] != "docs");
        for check in checks.iter_mut().filter(|check| check["context"] == "fmt") {
            check["integration_id"] = json!(1);
        }
        checks.push(json!({"context": "lint"}));
    }
    assert_eq!(
        differences(&desired, &live),
        [
            "ruleset main: rule required_status_checks: required check docs (app 15368) is missing",
            "ruleset main: rule required_status_checks: required check fmt (app 15368) is missing",
            "ruleset main: rule required_status_checks: required check fmt (app 1) is on \
             GitHub but not in the file",
            "ruleset main: rule required_status_checks: required check lint (any app) is on \
             GitHub but not in the file",
        ]
    );
    Ok(())
}

#[test]
fn repo_fields_are_compared_and_the_ones_github_leaves_out_are_notes() -> TestResult {
    let (desired, mut live) = load()?;
    let repo = live.repo.as_object_mut().ok_or("repo is not an object")?;
    repo.insert("allow_merge_commit".to_owned(), json!(true));
    repo.insert("squash_merge_commit_title".to_owned(), json!(null));
    repo.remove("allow_update_branch");
    let comparison = compare(&desired, &live);
    assert_eq!(
        comparison.differences,
        [
            "repo allow_merge_commit is true, expected false",
            "repo squash_merge_commit_title is null, expected \"PR_TITLE\"",
        ]
    );
    assert_eq!(
        comparison.notes,
        ["repo allow_update_branch: not checked, GitHub did not return it"]
    );
    Ok(())
}

#[test]
fn labels_are_found_in_any_case_and_compared_by_name_colour_and_description() -> TestResult {
    let (desired, mut live) = load()?;
    live.labels.retain(|label| label.name != "macos");
    for label in &mut live.labels {
        match label.name.as_str() {
            "needs-jann" => label.color = "FFFFFF".to_owned(),
            "approved" => {
                label.name = "Approved".to_owned();
                label.description = String::new();
            }
            _ => {}
        }
    }
    assert_eq!(
        differences(&desired, &live),
        [
            "label needs-jann: color is \"FFFFFF\", expected \"d93f0b\"",
            "label approved: named \"Approved\", expected \"approved\"",
            "label approved: description is \"\", expected \
             \"Reviewed by Jann; only Jann adds this label\"",
            "label macos: missing",
        ]
    );
    // The colour is compared without regard to case: GitHub writes hex digits in lower case.
    live.labels = desired.labels.clone();
    for label in &mut live.labels {
        label.color = label.color.to_uppercase();
    }
    assert_eq!(differences(&desired, &live), Vec::<String>::new());
    Ok(())
}

#[test]
fn long_values_from_github_are_cut() -> TestResult {
    let (desired, mut live) = load()?;
    live_ruleset(&mut live, "main")?["enforcement"] = json!("x".repeat(500));
    let found = differences(&desired, &live);
    assert_eq!(found.len(), 1);
    assert!(found[0].chars().count() < 200, "{}", found[0]);
    assert!(found[0].contains('…'), "{}", found[0]);
    Ok(())
}
