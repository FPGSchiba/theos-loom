//! The GitHub settings as files, and the comparison with the live repository (`docs/gate.md` §1
//! and §6).
//!
//! Two files in `gate/` say how the repository is set up: `ruleset-main.json` (the rules of the
//! default branch) and `repo-settings.toml` (the merge settings and the labels). [`Desired`] reads
//! them, [`Live`] reads the same things from GitHub through [`RepoAdmin`], and [`compare`] says
//! where they differ:
//!
//! - Rulesets are matched by `name`. Only the keys the file has are compared, so what GitHub adds
//!   (ids, timestamps, links) does not matter. Lists are compared without regard to order, and an
//!   item on either side only is a difference, so a bypass actor that GitHub has and the file
//!   does not is found. `rules` is a set keyed by `type`, and the required status checks are a set
//!   of `(context, integration_id)`. A key the file has and GitHub does not return, as with a
//!   token that cannot read it, is a note ("not checked"); a ruleset that is not there at all is
//!   a difference.
//! - Settings of the repository are compared one by one. A setting that GitHub does not return
//!   is a note too, not a difference.
//! - Labels are matched by name in any case, and compared by name, colour and description. Labels
//!   that only GitHub has, such as its defaults, are left alone.
//!
//! [`check`] is the comparison as a verdict, and [`apply`] makes the repository like the files:
//! it creates or replaces the ruleset, patches the settings and sets the labels. Applying again
//! changes nothing that is already right. It changes the live repository, so it is only run
//! after Jann has said so (`docs/gate.md` §6).
//!
//! Text from GitHub ends up in the differences, and it is data: a value is shown as JSON, cut to
//! [`MAX_SHOWN`] characters, and [`report`](crate::report) marks every line of what is written.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use super::gh_cli::GhCli;
use super::{LabelSpec, RepoAdmin};
use crate::checks::approval::decide::{APPROVED, NEEDS_JANN};
use crate::checks::lints;
use crate::markdown;
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The file with the merge settings and the labels, relative to the repository root.
pub const FILE: &str = "gate/repo-settings.toml";

/// The files with the rulesets, relative to the repository root.
const RULESET_FILES: [&str; 1] = ["gate/ruleset-main.json"];

/// The most characters of a value from either side that a difference shows.
pub const MAX_SHOWN: usize = 80;

/// The longest description GitHub allows for a label.
const MAX_DESCRIPTION: usize = 100;

/// How the repository should be set up: the files of `gate/`.
#[derive(Clone, PartialEq, Debug)]
pub struct Desired {
    /// The rulesets: `ruleset-main.json`. Each is the body that GitHub takes to create one,
    /// and has a `name`.
    pub rulesets: Vec<Value>,
    /// The settings of the repository, as GitHub names them.
    pub repo: toml::Table,
    /// The labels the repository has, among others.
    pub labels: Vec<LabelSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    repo: toml::Table,
    labels: Vec<LabelSpec>,
}

impl Desired {
    /// Reads the files of the checkout at `root`.
    ///
    /// # Errors
    ///
    /// Fails when a file cannot be read, is not JSON or TOML of the expected shape, or has a
    /// ruleset without a name, a name twice, a label with a colour that is not six hex digits, a
    /// description that is too long, or no label for the gate (`needs-jann`, `approved`). The errors never quote the files.
    pub fn load(root: &Path) -> Result<Self> {
        let mut rulesets: Vec<Value> = Vec::new();
        for file in RULESET_FILES {
            let ruleset = read_ruleset(root, file)?;
            if rulesets
                .iter()
                .any(|known| known["name"] == ruleset["name"])
            {
                return Err(config_error(file, "has the name of another ruleset"));
            }
            rulesets.push(ruleset);
        }
        let SettingsFile { repo, labels } = read_settings(root)?;
        Ok(Self {
            rulesets,
            repo,
            labels,
        })
    }
}

/// The labels of `gate/repo-settings.toml` of the checkout at `root`, checked as [`Desired::load`]
/// checks them. `approval` creates its labels from these.
///
/// # Errors
///
/// The errors of [`Desired::load`] that concern the settings file.
pub fn labels(root: &Path) -> Result<Vec<LabelSpec>> {
    read_settings(root).map(|settings| settings.labels)
}

fn config_error(file: &str, message: impl Into<String>) -> Error {
    Error::Config {
        file: file.to_owned(),
        message: message.into(),
    }
}

/// The text of `file` under `root`.
fn read(root: &Path, file: &str) -> Result<String> {
    let path = root.join(file);
    fs::read_to_string(&path).map_err(|source| Error::Io { path, source })
}

fn read_ruleset(root: &Path, file: &str) -> Result<Value> {
    let text = read(root, file)?;
    // The parser's message can quote what it read, so only its place is passed on.
    let ruleset: Value = serde_json::from_str(&text).map_err(|error| {
        config_error(
            file,
            format!("is not valid JSON, near line {}", error.line()),
        )
    })?;
    let named = ruleset["name"]
        .as_str()
        .is_some_and(|name| !name.is_empty());
    if !(ruleset.is_object() && named && ruleset["rules"].is_array()) {
        return Err(config_error(
            file,
            "is not a ruleset: it is an object with a `name` and a list of `rules`",
        ));
    }
    Ok(ruleset)
}

fn read_settings(root: &Path) -> Result<SettingsFile> {
    let text = read(root, FILE)?;
    // The TOML error would quote the line, so only its place is passed on.
    let table = lints::parse(FILE, &text).map_err(|error| match error {
        Error::Parse { message, .. } => config_error(FILE, message),
        other => other,
    })?;
    let settings: SettingsFile = table.try_into().map_err(|_| {
        config_error(
            FILE,
            "has an unexpected shape: `[repo]` holds settings, and every `[[labels]]` has a \
             `name`, a `color` and a `description`",
        )
    })?;
    if settings.repo.is_empty() {
        return Err(config_error(FILE, "has no setting under `[repo]`"));
    }
    check_labels(&settings.labels)?;
    Ok(settings)
}

/// Whether `labels` can be sent to GitHub and have the labels the gate works with.
fn check_labels(labels: &[LabelSpec]) -> Result<()> {
    let bad =
        |index: usize, message: &str| config_error(FILE, format!("label {} {message}", index + 1));
    for (index, label) in labels.iter().enumerate() {
        if label.name.trim().is_empty() || label.name.chars().any(char::is_control) {
            return Err(bad(index, "has no name, or one with control characters"));
        }
        let hex = label.color.len() == 6 && label.color.chars().all(|c| c.is_ascii_hexdigit());
        if !hex {
            return Err(bad(
                index,
                "has a colour that is not six hex digits, without `#`",
            ));
        }
        if label.description.chars().count() > MAX_DESCRIPTION {
            return Err(bad(index, "has a description of more than 100 characters"));
        }
        if labels[..index]
            .iter()
            .any(|other| other.name.eq_ignore_ascii_case(&label.name))
        {
            return Err(bad(
                index,
                "has the name of an earlier label, whatever the case",
            ));
        }
    }
    for name in [NEEDS_JANN, APPROVED] {
        if !labels.iter().any(|label| label.name == name) {
            return Err(config_error(
                FILE,
                format!("has no label `{name}`, which the gate works with"),
            ));
        }
    }
    Ok(())
}

/// The repository as GitHub has it now, for the things the files describe.
#[derive(Clone, PartialEq, Debug)]
pub struct Live {
    /// The rulesets of the repository that [`Desired`] has a ruleset of the same name for, each
    /// as GitHub describes it. Others are not read.
    pub rulesets: Vec<Value>,
    /// The repository as GitHub describes it, with its merge settings.
    pub repo: Value,
    /// Every label of the repository.
    pub labels: Vec<LabelSpec>,
}

impl Live {
    /// Reads what `desired` describes from GitHub through `gh`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses a request.
    pub fn read(gh: &dyn RepoAdmin, desired: &Desired) -> Result<Self> {
        let wanted: Vec<&str> = desired
            .rulesets
            .iter()
            .filter_map(|ruleset| ruleset["name"].as_str())
            .collect();
        let mut rulesets = Vec::new();
        for (id, name) in gh.rulesets()? {
            if wanted.contains(&name.as_str()) {
                rulesets.push(gh.ruleset(id)?);
            }
        }
        Ok(Self {
            rulesets,
            repo: gh.repo()?,
            labels: gh.repo_labels()?,
        })
    }
}

/// What a comparison found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Comparison {
    /// Where the repository is not as the files say, one line each.
    pub differences: Vec<String>,
    /// What could not be compared, one line each.
    pub notes: Vec<String>,
}

/// Compares the live repository with the files.
#[must_use]
pub fn compare(desired: &Desired, live: &Live) -> Comparison {
    let mut found = Comparison::default();
    for want in &desired.rulesets {
        compare_ruleset(want, &live.rulesets, &mut found);
    }
    compare_repo(&desired.repo, &live.repo, &mut found);
    compare_labels(&desired.labels, &live.labels, &mut found.differences);
    found
}

/// The places where the live repository is not as the files say, one line each.
#[must_use]
pub fn differences(desired: &Desired, live: &Live) -> Vec<String> {
    compare(desired, live).differences
}

/// `text`, or its first characters and `…` when it is longer than [`MAX_SHOWN`].
fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_SHOWN {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_SHOWN - 1).collect();
    cut.push('…');
    cut
}

/// `value` as compact JSON, clipped.
fn show(value: &Value) -> String {
    clip(&value.to_string())
}

fn show_text(text: &str) -> String {
    show(&Value::String(text.to_owned()))
}

fn compare_ruleset(want: &Value, live: &[Value], found: &mut Comparison) {
    let name = want["name"].as_str().unwrap_or_default();
    let prefix = format!("ruleset {name}");
    let have = live
        .iter()
        .find(|ruleset| ruleset["name"].as_str() == Some(name));
    match have {
        Some(have) => compare_value(&prefix, "", want, have, found),
        None => found.differences.push(format!("{prefix}: not on GitHub")),
    }
}

/// Every difference between `want` and `have`, each starting with `prefix` and saying where in
/// the value it is by `path`, such as `conditions.ref_name.include`. A key of `want` that `have`
/// lacks is a note, not a difference: GitHub leaves out what the token may not read, such as the
/// bypass actors for a token without admin rights. A key that both have is compared.
fn compare_value(prefix: &str, path: &str, want: &Value, have: &Value, found: &mut Comparison) {
    match (want, have) {
        (Value::Object(want), Value::Object(have)) => {
            for (key, want) in want {
                let path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let Some(have) = have.get(key) else {
                    found.notes.push(format!(
                        "{prefix}: {path}: not checked, GitHub did not return it"
                    ));
                    continue;
                };
                match (key.as_str(), want, have) {
                    ("rules", Value::Array(want), Value::Array(have)) => {
                        compare_rules(prefix, want, have, found);
                    }
                    ("required_status_checks", Value::Array(want), Value::Array(have)) => {
                        compare_checks(prefix, want, have, &mut found.differences);
                    }
                    _ => compare_value(prefix, &path, want, have, found),
                }
            }
        }
        (Value::Array(want_items), Value::Array(have_items)) => {
            if !same_items(want_items, have_items) {
                found.differences.push(format!(
                    "{prefix}: {path} is {}, expected {}",
                    show(have),
                    show(want)
                ));
            }
        }
        _ => {
            if want != have {
                found.differences.push(format!(
                    "{prefix}: {path} is {}, expected {}",
                    show(have),
                    show(want)
                ));
            }
        }
    }
}

/// Whether the two lists have the same items, in any order.
fn same_items(want: &[Value], have: &[Value]) -> bool {
    let sorted = |items: &[Value]| {
        let mut texts: Vec<String> = items.iter().map(Value::to_string).collect();
        texts.sort();
        texts
    };
    sorted(want) == sorted(have)
}

/// The `type` of a rule.
fn kind(rule: &Value) -> &str {
    rule["type"].as_str().unwrap_or_default()
}

/// Rules are a set keyed by `type`: a type on one side only is a difference, and the parameters
/// of a type that the file gives are compared like any other value.
fn compare_rules(prefix: &str, want: &[Value], have: &[Value], found: &mut Comparison) {
    for rule in want {
        let name = kind(rule);
        match have.iter().find(|other| kind(other) == name) {
            Some(live) => {
                let prefix = format!("{prefix}: rule {}", clip(name));
                compare_value(&prefix, "", rule, live, found);
            }
            None => found
                .differences
                .push(format!("{prefix}: rule {} is missing", clip(name))),
        }
    }
    for rule in have {
        let name = kind(rule);
        if !want.iter().any(|other| kind(other) == name) {
            found.differences.push(format!(
                "{prefix}: rule {} is on GitHub but not in the file",
                clip(name)
            ));
        }
    }
}

/// Required status checks are a set of `(context, integration_id)`. A check without an
/// `integration_id` is one any app may report.
fn compare_checks(prefix: &str, want: &[Value], have: &[Value], out: &mut Vec<String>) {
    let pairs = |items: &[Value]| -> BTreeSet<(String, Option<i64>)> {
        items
            .iter()
            .map(|item| {
                let context = item["context"].as_str().unwrap_or_default();
                (context.to_owned(), item["integration_id"].as_i64())
            })
            .collect()
    };
    let app = |id: Option<i64>| id.map_or_else(|| "any app".to_owned(), |id| format!("app {id}"));
    let (want, have) = (pairs(want), pairs(have));
    for (context, id) in want.difference(&have) {
        out.push(format!(
            "{prefix}: required check {} ({}) is missing",
            clip(context),
            app(*id)
        ));
    }
    for (context, id) in have.difference(&want) {
        out.push(format!(
            "{prefix}: required check {} ({}) is on GitHub but not in the file",
            clip(context),
            app(*id)
        ));
    }
}

/// `value` as JSON.
fn to_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn compare_repo(want: &toml::Table, have: &Value, found: &mut Comparison) {
    for (key, want) in want {
        let want = to_json(want);
        match have.get(key) {
            None => found
                .notes
                .push(format!("repo {key}: not checked, GitHub did not return it")),
            Some(have) if *have == want => {}
            Some(have) => found.differences.push(format!(
                "repo {key} is {}, expected {}",
                show(have),
                show(&want)
            )),
        }
    }
}

fn compare_labels(want: &[LabelSpec], live: &[LabelSpec], out: &mut Vec<String>) {
    for want in want {
        let name = &want.name;
        let Some(have) = live
            .iter()
            .find(|label| label.name.eq_ignore_ascii_case(name))
        else {
            out.push(format!("label {name}: missing"));
            continue;
        };
        if have.name != *name {
            out.push(format!(
                "label {name}: named {}, expected {}",
                show_text(&have.name),
                show_text(name)
            ));
        }
        // GitHub writes the hex digits of a colour in lower case.
        if !have.color.eq_ignore_ascii_case(&want.color) {
            out.push(format!(
                "label {name}: color is {}, expected {}",
                show_text(&have.color),
                show_text(&want.color)
            ));
        }
        if have.description != want.description {
            out.push(format!(
                "label {name}: description is {}, expected {}",
                show_text(&have.description),
                show_text(&want.description)
            ));
        }
    }
}

/// Compares the live repository `gh` reaches with the files at `root`: a failing verdict with a
/// finding for every difference, and a note for everything that could not be compared.
///
/// # Errors
///
/// Fails when the files cannot be read or GitHub cannot be reached.
pub fn check(gh: &dyn RepoAdmin, root: &Path) -> Result<Verdict> {
    let desired = Desired::load(root)?;
    let live = Live::read(gh, &desired)?;
    let found = compare(&desired, &live);
    Ok(Verdict {
        check: Check::Github,
        findings: found.differences.into_iter().map(Finding::new).collect(),
        notes: found.notes,
        not_needed: None,
    })
}

/// Makes the live repository `gh` reaches like the files at `root`, and returns what it did, one
/// line for each change. A ruleset that exists is replaced and one that does not is created; the
/// settings are patched; every label is created or set. Other rulesets and labels are left alone.
/// The rulesets come first: they are the change GitHub is most likely to refuse, for instance
/// when the plan has no rulesets, and then nothing else has been changed.
///
/// This changes the live repository: it is only run after Jann has agreed.
///
/// # Errors
///
/// Fails when the files cannot be read or GitHub cannot be reached or refuses a request. The
/// changes made until then stay.
pub fn apply(gh: &dyn RepoAdmin, root: &Path) -> Result<Vec<String>> {
    let desired = Desired::load(root)?;
    let mut actions = Vec::new();
    let live = gh.rulesets()?;
    for ruleset in &desired.rulesets {
        let name = ruleset["name"].as_str().unwrap_or_default();
        if let Some((id, _)) = live.iter().find(|(_, live_name)| live_name == name) {
            gh.update_ruleset(*id, ruleset)?;
            actions.push(format!("updated ruleset {name} (id {id})"));
        } else {
            gh.create_ruleset(ruleset)?;
            actions.push(format!("created ruleset {name}"));
        }
    }
    let settings = desired
        .repo
        .iter()
        .map(|(key, value)| (key.clone(), to_json(value)))
        .collect();
    gh.patch_repo(&Value::Object(settings))?;
    actions.push(format!("patched {} repo settings", desired.repo.len()));
    for label in &desired.labels {
        gh.upsert_label(label)?;
        actions.push(format!("set label {}", label.name));
    }
    Ok(actions)
}

/// The verdict that reports what [`apply`] did: it passes, and has a note for each action.
fn applied(actions: Vec<String>) -> Verdict {
    Verdict {
        check: Check::Github,
        findings: Vec::new(),
        notes: actions,
        not_needed: None,
    }
}

/// `cargo xtask github check`: [`check`] on the repository `repo` names, else the one of
/// `GITHUB_REPOSITORY`, else the one `origin` points to. `gh` uses `GH_TOKEN`, or else the login
/// it has stored.
///
/// # Errors
///
/// The errors of [`GhCli::for_checkout`] and of [`check`].
pub fn run_check(root: &Path, repo: Option<&str>) -> Result<Verdict> {
    check(&GhCli::for_checkout(root, repo)?, root)
}

/// `cargo xtask github apply`: [`apply`] on the repository that [`run_check`] would read, and a
/// verdict with a note for every change made. It changes the live repository.
///
/// # Errors
///
/// The errors of [`GhCli::for_checkout`] and of [`apply`].
pub fn run_apply(root: &Path, repo: Option<&str>) -> Result<Verdict> {
    let actions = apply(&GhCli::for_checkout(root, repo)?, root)?;
    Ok(applied(actions))
}

/// The checks of `docs/gate.md` §1 as GitHub names them: the first cell of every row of the first
/// table under the level-two section whose heading starts with `1.`. A cell that is not a bare job
/// name (`[a-z0-9-]`) is skipped.
#[must_use]
pub fn required_contexts_from_gate_md(text: &str) -> BTreeSet<String> {
    let sections = markdown::sections(text, 2);
    let Some(section) = sections
        .iter()
        .find(|section| section.heading.text.starts_with("1."))
    else {
        return BTreeSet::new();
    };
    let tables = markdown::tables(section.body);
    let Some(table) = tables.first() else {
        return BTreeSet::new();
    };
    table
        .rows
        .iter()
        .filter_map(|row| row.first())
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::github::fake::{Call, FakeGitHub};
    use crate::report::write_table;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn root() -> TestResult<&'static Path> {
        Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("xtask has no parent folder")?)
    }

    /// A checkout with copies of the settings files, to break one at a time.
    fn copy_of_settings() -> TestResult<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("gate"))?;
        for file in RULESET_FILES.into_iter().chain([FILE]) {
            fs::copy(root()?.join(file), dir.path().join(file))?;
        }
        Ok(dir)
    }

    fn rewrite(dir: &Path, file: &str, change: impl Fn(&str) -> String) -> TestResult {
        let path = dir.join(file);
        let text = fs::read_to_string(&path)?;
        fs::write(path, change(&text))?;
        Ok(())
    }

    fn message_of<T: std::fmt::Debug>(result: Result<T>) -> TestResult<String> {
        match result {
            Err(Error::Config { message, .. }) => Ok(message),
            other => Err(format!("expected a config error, got {other:?}").into()),
        }
    }

    #[test]
    fn apply_creates_then_updates() -> TestResult {
        let gh = FakeGitHub::new();
        let desired = Desired::load(root()?)?;

        let first = apply(&gh, root()?)?;
        assert_eq!(
            first,
            [
                "created ruleset main",
                "patched 8 repo settings",
                "set label needs-jann",
                "set label approved",
            ]
        );
        let kinds: Vec<&str> = gh
            .writes()
            .iter()
            .map(|call| match call {
                Call::CreateRuleset(_) => "create",
                Call::UpdateRuleset(..) => "update",
                Call::PatchRepo(_) => "patch",
                Call::UpsertLabel(_) => "label",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["create", "patch", "label", "label"]);
        assert_eq!(gh.current_rulesets().len(), 1);

        let second = apply(&gh, root()?)?;
        assert_eq!(
            second,
            [
                "updated ruleset main (id 1)",
                "patched 8 repo settings",
                "set label needs-jann",
                "set label approved",
            ]
        );
        assert_eq!(gh.current_rulesets().len(), 1, "nothing was created again");
        assert_eq!(gh.current_repo_labels(), desired.labels);

        // The repository is now as the files say, whatever else GitHub added.
        let live = Live::read(&gh, &desired)?;
        assert_eq!(compare(&desired, &live), Comparison::default());
        Ok(())
    }

    #[test]
    fn apply_sends_the_files_as_they_are() -> TestResult {
        let gh = FakeGitHub::new();
        apply(&gh, root()?)?;
        let desired = Desired::load(root()?)?;
        let sent = gh.writes();
        assert_eq!(sent[0], Call::CreateRuleset(desired.rulesets[0].clone()));
        // Booleans and strings, as JSON: a form field would send them all as strings.
        let Call::PatchRepo(body) = &sent[1] else {
            return Err("the second change is not a patch of the settings".into());
        };
        assert_eq!(body["allow_squash_merge"], json!(true));
        assert_eq!(body["allow_merge_commit"], json!(false));
        assert_eq!(body["squash_merge_commit_title"], json!("PR_TITLE"));
        assert_eq!(body.as_object().map(serde_json::Map::len), Some(8));
        Ok(())
    }

    #[test]
    fn apply_leaves_other_rulesets_and_labels_alone() -> TestResult {
        let other = LabelSpec {
            name: "bug".to_owned(),
            color: "d73a4a".to_owned(),
            description: "Something is not working".to_owned(),
        };
        let gh = FakeGitHub::new().with_repo_labels(std::slice::from_ref(&other));
        gh.create_ruleset(&json!({"name": "other"}))?;
        apply(&gh, root()?)?;
        let names: Vec<String> = gh.rulesets()?.into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, ["other", "main"]);
        assert!(gh.current_repo_labels().contains(&other));
        Ok(())
    }

    #[test]
    fn apply_sets_a_label_that_looks_wrong() -> TestResult {
        let desired = Desired::load(root()?)?;
        let mut wrong = desired.labels[0].clone();
        wrong.name = wrong.name.to_uppercase();
        wrong.color = "ffffff".to_owned();
        let gh = FakeGitHub::new().with_repo_labels(&[wrong]);
        let live = Live::read(&gh, &desired)?;
        // The ruleset, the spelling and the colour of the one label, and the one it lacks.
        assert_eq!(differences(&desired, &live).len(), 4, "{live:?}");
        apply(&gh, root()?)?;
        let live = Live::read(&gh, &desired)?;
        assert_eq!(differences(&desired, &live), Vec::<String>::new());
        assert_eq!(gh.current_repo_labels(), desired.labels);
        Ok(())
    }

    #[test]
    fn check_fails_before_apply_and_passes_after() -> TestResult {
        let gh = FakeGitHub::new();
        let before = check(&gh, root()?)?;
        assert_eq!(before.check, Check::Github);
        assert!(!before.passed());
        let messages: Vec<&str> = before
            .findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect();
        assert_eq!(
            messages,
            [
                "ruleset main: not on GitHub",
                "label needs-jann: missing",
                "label approved: missing",
            ]
        );
        // GitHub returned no settings at all, which is what a token without access does.
        assert_eq!(before.notes.len(), 8);
        assert!(
            before.notes[0].ends_with("not checked, GitHub did not return it"),
            "{:?}",
            before.notes
        );

        apply(&gh, root()?)?;
        let after = check(&gh, root()?)?;
        assert!(after.passed(), "{after:?}");
        assert_eq!(after.notes, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn check_reads_only_the_rulesets_it_compares_and_changes_nothing() -> TestResult {
        let gh = FakeGitHub::new();
        gh.create_ruleset(&json!({"name": "other"}))?;
        gh.create_ruleset(&json!({"name": "main"}))?;
        let writes = gh.writes().len();
        check(&gh, root()?)?;
        let calls = gh.calls();
        assert_eq!(gh.writes().len(), writes, "a check changes nothing");
        // `other` is id 1 and `main` is id 2.
        assert!(calls.contains(&Call::Ruleset(2)));
        assert!(!calls.contains(&Call::Ruleset(1)));
        Ok(())
    }

    #[test]
    fn what_apply_did_is_written_as_marked_lines_of_a_passing_verdict() -> TestResult {
        let actions = apply(&FakeGitHub::new(), root()?)?;
        let verdict = applied(actions);
        assert!(verdict.passed());
        let mut out = Vec::new();
        write_table(&[verdict], &mut out)?;
        let text = String::from_utf8(out)?;
        assert_eq!(
            text.lines().take(3).collect::<Vec<_>>(),
            [
                "pass  github",
                "  note: created ruleset main",
                "  note: patched 8 repo settings"
            ]
        );
        assert!(
            text.lines()
                .skip(1)
                .all(|line| line.starts_with("  note: "))
        );
        Ok(())
    }

    #[test]
    fn a_failing_check_writes_every_difference_as_a_marked_line() -> TestResult {
        let verdict = check(&FakeGitHub::new(), root()?)?;
        let mut out = Vec::new();
        write_table(&[verdict], &mut out)?;
        let text = String::from_utf8(out)?;
        assert!(text.starts_with("FAIL  github\n  - ruleset main: not on GitHub\n"));
        assert!(
            text.lines()
                .skip(1)
                .all(|line| line.starts_with("  - ") || line.starts_with("  note: ")),
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn the_labels_alone_are_read_from_the_settings_file() -> TestResult {
        let dir = copy_of_settings()?;
        fs::remove_file(dir.path().join(RULESET_FILES[0]))?;
        let names: Vec<String> = labels(dir.path())?
            .into_iter()
            .map(|label| label.name)
            .collect();
        assert_eq!(names, ["needs-jann", "approved"]);
        assert!(Desired::load(dir.path()).is_err(), "the ruleset is gone");
        Ok(())
    }

    #[test]
    fn a_settings_file_that_is_missing_is_an_io_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        assert!(matches!(Desired::load(dir.path()), Err(Error::Io { .. })));
        assert!(matches!(labels(dir.path()), Err(Error::Io { .. })));
        Ok(())
    }

    #[test]
    fn a_ruleset_that_is_not_json_or_not_a_ruleset_is_refused_without_quoting_it() -> TestResult {
        for (text, expected) in [
            ("{\"name\": SECRET", "is not valid JSON, near line 1"),
            ("[\"SECRET\"]", "is not a ruleset"),
            ("{\"name\": \"\", \"rules\": []}", "is not a ruleset"),
            ("{\"name\": \"SECRET\"}", "is not a ruleset"),
        ] {
            let dir = copy_of_settings()?;
            rewrite(dir.path(), RULESET_FILES[0], |_| text.to_owned())?;
            let message = message_of(Desired::load(dir.path()))?;
            assert!(message.starts_with(expected), "{message}");
            assert!(!message.contains("SECRET"), "{message}");
        }
        Ok(())
    }

    /// Loading the settings after `change` of their file fails with exactly `expected`, which
    /// does not quote the file.
    fn refused(change: impl Fn(&str) -> String, expected: &str) -> TestResult {
        let dir = copy_of_settings()?;
        rewrite(dir.path(), FILE, change)?;
        let message = message_of(Desired::load(dir.path()))?;
        assert_eq!(message, expected);
        assert!(!message.contains("SECRET"), "{message}");
        Ok(())
    }

    #[test]
    fn settings_that_are_not_valid_are_refused_without_quoting_them() -> TestResult {
        refused(|_| "SECRET = [".to_owned(), "not valid TOML, near line 1")?;
        let shape = "has an unexpected shape: `[repo]` holds settings, and every `[[labels]]` has a \
                     `name`, a `color` and a `description`";
        refused(|text| format!("{text}\n[unknown]\nSECRET = 1\n"), shape)?;
        refused(|text| text.replace("[repo]", "[SECRET]"), shape)?;
        let colour = "label 1 has a colour that is not six hex digits, without `#`";
        refused(|text| text.replace("d93f0b", "#d93f0b"), colour)?;
        refused(|text| text.replace("d93f0b", "d93f0"), colour)?;
        refused(
            |text| text.replace("Touches a protected path; waits for Jann", &"x".repeat(101)),
            "label 1 has a description of more than 100 characters",
        )?;
        refused(
            |text| text.replace("name = \"approved\"", "name = \"Needs-Jann\""),
            "label 2 has the name of an earlier label, whatever the case",
        )?;
        refused(
            |text| text.replace("name = \"approved\"", "name = \" \""),
            "label 2 has no name, or one with control characters",
        )?;
        refused(
            |text| text.replace("name = \"approved\"", "name = \"ok\""),
            "has no label `approved`, which the gate works with",
        )
    }

    #[test]
    fn settings_need_at_least_one_repo_setting() -> TestResult {
        let dir = copy_of_settings()?;
        rewrite(dir.path(), FILE, |text| {
            let labels = text.find("[[labels]]").unwrap_or(0);
            format!("[repo]\n\n{}", &text[labels..])
        })?;
        let message = message_of(Desired::load(dir.path()))?;
        assert_eq!(message, "has no setting under `[repo]`");
        Ok(())
    }
}
