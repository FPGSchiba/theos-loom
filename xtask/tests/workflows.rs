//! The shape of the GitHub Actions workflows and of the two actions under `.github/`.
//!
//! CI is thin over xtask (`docs/gate.md` §1): a job checks out, sets up and runs one `cargo xtask`
//! command, and the workflow files hold triggers, runners, caching and permissions and nothing
//! else. These tests parse the real files, so a change that breaks a rule of `docs/gate.md` §1
//! or of the spec's §5.7 fails here, before it fails on GitHub.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use xtask::gate::{selected, selection};
use xtask::github::settings::required_contexts_from_gate_md;
use xtask::verdict::{Check, GATE_ORDER};
use yaml_rust2::{Yaml, YamlLoader};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The workflows whose jobs are the required status checks of `main`.
const PR_WORKFLOWS: [&str; 3] = ["ci.yml", "pr.yml", "approval.yml"];

/// Every workflow file.
const ALL_WORKFLOWS: [&str; 4] = ["ci.yml", "pr.yml", "approval.yml", "settings.yml"];

const SETUP_ACTION: &str = ".github/actions/setup/action.yml";
const XTASK_ACTION: &str = ".github/actions/xtask/action.yml";

/// The step that runs the setup action, as a job names it.
const SETUP_USES: &str = "./.github/actions/setup";

/// The step that runs the cached xtask, as a job names it.
const XTASK_USES: &str = "./.github/actions/xtask";

/// The expression that is the workflow token.
const TOKEN: &str = "${{ github.token }}";

/// The expression that is true on `main` only: the jobs save their caches under it.
const ON_MAIN: &str = "${{ github.ref == 'refs/heads/main' }}";

/// The URL of the throwaway database of `test` and `coverage` (umbrella T4b).
const DATABASE_URL: &str = "postgres://postgres@localhost:5432/loom_test";

/// The minutes each job may run before GitHub cancels it, as (file, job id, minutes). The default
/// is six hours. Each value is three times the job's cold run in P00's pull request (7547d98),
/// rounded up to 5 minutes and at least 10; `settings` has not run yet and takes `gate`'s value.
const TIMEOUT_MINUTES: [(&str, &str, i64); 7] = [
    ("ci.yml", "gate", 10),
    ("ci.yml", "test", 10),
    ("ci.yml", "coverage", 15),
    ("ci.yml", "ui", 10),
    ("pr.yml", "pr", 10),
    ("approval.yml", "approval", 10),
    ("settings.yml", "settings", 10),
];

fn root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?
        .to_path_buf())
}

/// The text of a file of the repository, given by its path from the root with `/`.
fn read(relative: &str) -> TestResult<String> {
    Ok(fs::read_to_string(root()?.join(relative))?)
}

/// The one YAML document of a file of the repository.
fn parse(relative: &str) -> TestResult<Yaml> {
    let mut documents = YamlLoader::load_from_str(&read(relative)?)?;
    if documents.len() != 1 {
        return Err(format!("{relative} has {} YAML documents, not one", documents.len()).into());
    }
    Ok(documents.remove(0))
}

fn workflow_path(file: &str) -> String {
    format!(".github/workflows/{file}")
}

fn workflow(file: &str) -> TestResult<Yaml> {
    parse(&workflow_path(file))
}

/// A job of a workflow: its key, its name (which is its status check) and its definition.
struct Job {
    id: String,
    name: String,
    node: Yaml,
}

impl Job {
    fn runs_on(&self) -> TestResult<&str> {
        self.node["runs-on"]
            .as_str()
            .ok_or_else(|| format!("job {} has no runs-on", self.id).into())
    }

    fn steps(&self) -> TestResult<&[Yaml]> {
        self.node["steps"]
            .as_vec()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("job {} has no steps", self.id).into())
    }

    /// The `run:` values of the job, one per `run:` step.
    fn commands(&self) -> TestResult<Vec<String>> {
        Ok(run_values(self.steps()?))
    }

    /// The first step that uses `action` (a local action, as written in `uses:`).
    fn step_using(&self, action: &str) -> TestResult<Option<&Yaml>> {
        Ok(self
            .steps()?
            .iter()
            .find(|step| step["uses"].as_str() == Some(action)))
    }

    /// The command the job's last step runs, as `cargo xtask <args>`: the step's `run:`, or the
    /// arguments of the cached xtask action after `cargo xtask `. Cache saves that follow the
    /// command (they store what it wrote, on `main`) are not counted.
    fn last_command(&self) -> TestResult<String> {
        let last = self
            .steps()?
            .iter()
            .rfind(|step| step_kind(step).is_ok_and(|kind| kind != "cache-save"))
            .ok_or_else(|| format!("job {} has no steps", self.id))?;
        if let Some(command) = last["run"].as_str() {
            return Ok(command.trim().to_owned());
        }
        if last["uses"].as_str() == Some(XTASK_USES) {
            let args = last["with"]["args"]
                .as_str()
                .ok_or_else(|| format!("job {}: the xtask step has no args", self.id))?;
            return Ok(format!("cargo xtask {args}"));
        }
        Err(format!("job {}: the last step runs no command", self.id).into())
    }
}

fn jobs(workflow: &Yaml) -> TestResult<Vec<Job>> {
    let table = workflow["jobs"]
        .as_hash()
        .ok_or("the workflow has no jobs")?;
    table
        .iter()
        .map(|(id, node)| -> TestResult<Job> {
            let id = id.as_str().ok_or("a job key is not a string")?.to_owned();
            let name = node["name"]
                .as_str()
                .ok_or_else(|| format!("job {id} has no name"))?
                .to_owned();
            Ok(Job {
                id,
                name,
                node: node.clone(),
            })
        })
        .collect()
}

/// The job of a workflow with this name.
fn job_named(file: &str, name: &str) -> TestResult<Job> {
    jobs(&workflow(file)?)?
        .into_iter()
        .find(|job| job.name == name)
        .ok_or_else(|| format!("{file} has no job named {name}").into())
}

/// Every job of every workflow, with the file it is in.
fn all_jobs() -> TestResult<Vec<(&'static str, Job)>> {
    let mut all = Vec::new();
    for file in ALL_WORKFLOWS {
        for job in jobs(&workflow(file)?)? {
            all.push((file, job));
        }
    }
    Ok(all)
}

fn run_values(steps: &[Yaml]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|step| step["run"].as_str().map(str::to_owned))
        .collect()
}

/// A string, number or boolean as the text it was written as.
fn scalar(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn strings(node: &Yaml) -> TestResult<Vec<String>> {
    node.as_vec()
        .ok_or("not a list")?
        .iter()
        .map(|item| scalar(item).ok_or_else(|| "a list item is not a scalar".into()))
        .collect()
}

fn keys(node: &Yaml) -> TestResult<BTreeSet<String>> {
    node.as_hash()
        .ok_or("not a mapping")?
        .keys()
        .map(|key| scalar(key).ok_or_else(|| "a key is not a scalar".into()))
        .collect()
}

fn string_map(node: &Yaml) -> TestResult<BTreeMap<String, String>> {
    node.as_hash()
        .ok_or("not a mapping")?
        .iter()
        .map(|(key, value)| {
            let key = scalar(key).ok_or("a key is not a scalar")?;
            let value = scalar(value).ok_or("a value is not a scalar")?;
            Ok((key, value))
        })
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// The `with:` of a step as a map; empty when the step has none.
fn with_of(step: &Yaml) -> TestResult<BTreeMap<String, String>> {
    if step["with"].is_badvalue() {
        Ok(BTreeMap::new())
    } else {
        string_map(&step["with"])
    }
}

/// The action a step uses, without its version.
fn action_name(step: &Yaml) -> Option<&str> {
    step["uses"]
        .as_str()
        .and_then(|uses| uses.split_once('@'))
        .map(|(name, _)| name)
}

/// One `[[check]]` of `.loom/checks.toml`.
#[derive(Deserialize)]
struct CheckEntry {
    name: String,
    command: String,
    parts: Vec<String>,
}

#[derive(Deserialize)]
struct ChecksFile {
    check: Vec<CheckEntry>,
}

/// The check of `.loom/checks.toml` with this name.
fn check_entry(name: &str) -> TestResult<CheckEntry> {
    let file: ChecksFile = toml::from_str(&read(".loom/checks.toml")?)?;
    file.check
        .into_iter()
        .find(|check| check.name == name)
        .ok_or_else(|| format!(".loom/checks.toml has no check {name}").into())
}

/// The four jobs of `ci.yml`.
const CI_JOBS: [&str; 4] = ["gate", "test", "coverage", "ui"];

/// What a step is, for the shape of a job.
fn step_kind(step: &Yaml) -> TestResult<&'static str> {
    if let Some(uses) = step["uses"].as_str() {
        return if uses.starts_with("actions/checkout@") {
            Ok("checkout")
        } else if uses == SETUP_USES {
            Ok("setup")
        } else if uses == XTASK_USES {
            Ok("xtask")
        } else if uses.starts_with("actions/cache/restore@") {
            Ok("cache-restore")
        } else if uses.starts_with("actions/cache/save@") {
            Ok("cache-save")
        } else if uses.starts_with("actions/cache@") {
            // Restores and saves in one step, so it saves on pull requests too.
            Ok("cache")
        } else if uses.starts_with("pnpm/action-setup@") {
            Ok("pnpm")
        } else if uses.starts_with("actions/setup-node@") {
            Ok("node")
        } else {
            Err(format!("a step uses {uses}, which no job needs").into())
        };
    }
    if step["run"].as_str().is_some() {
        Ok("run")
    } else {
        Err("a step neither uses an action nor runs a command".into())
    }
}

fn step_kinds(job: &Job) -> TestResult<Vec<&'static str>> {
    job.steps()?.iter().map(step_kind).collect()
}

// AC: TU-00.1, TU-00.3
#[test]
fn ci_runs_four_jobs_side_by_side_on_linux() -> TestResult {
    let ci = workflow("ci.yml")?;
    let jobs = jobs(&ci)?;
    let ids: Vec<&str> = jobs.iter().map(|job| job.id.as_str()).collect();
    let names: Vec<&str> = jobs.iter().map(|job| job.name.as_str()).collect();
    assert_eq!(ids, CI_JOBS, "the job keys");
    assert_eq!(names, CI_JOBS, "the job names");
    for job in &jobs {
        let name = &job.name;
        assert_eq!(job.runs_on()?, "ubuntu-latest", "{name}");
        assert_eq!(
            job.node["needs"],
            Yaml::BadValue,
            "{name} waits for another job"
        );
        assert_eq!(job.node["if"], Yaml::BadValue, "{name} can be skipped");
    }
    assert_eq!(
        keys(&ci["on"])?,
        set(&["pull_request", "push", "workflow_dispatch"])
    );
    Ok(())
}

// AC: TU-00.3
#[test]
fn each_ci_job_runs_its_check_command() -> TestResult {
    for name in CI_JOBS {
        let job = job_named("ci.yml", name)?;
        assert_eq!(
            job.last_command()?,
            check_entry(name)?.command,
            "the last step of job {name} is not the command of its check in .loom/checks.toml"
        );
    }
    Ok(())
}

/// The parts a `cargo xtask gate` command selects, `tools` left out.
fn parts_of_gate_command(command: &str) -> TestResult<BTreeSet<&'static str>> {
    let arguments = command
        .strip_prefix("cargo xtask gate")
        .ok_or_else(|| format!("{command} is no gate command"))?;
    let words: Vec<&str> = arguments.split_whitespace().collect();
    let list = |value: &str| -> Vec<String> { value.split(',').map(str::to_owned).collect() };
    let (only, skip) = match words.as_slice() {
        [] => (Vec::new(), Vec::new()),
        ["--only", value] => (list(value), Vec::new()),
        ["--skip", value] => (Vec::new(), list(value)),
        other => return Err(format!("the arguments {other:?} are not --only or --skip").into()),
    };
    Ok(selected(&selection(&only, &skip)?)
        .into_iter()
        .filter(|check| *check != Check::Tools)
        .map(Check::name)
        .collect())
}

// AC: TU-00.3
#[test]
fn the_ci_jobs_cover_every_gate_part_once() -> TestResult {
    let mut seen = BTreeSet::new();
    for name in CI_JOBS {
        let command = job_named("ci.yml", name)?.last_command()?;
        for part in parts_of_gate_command(&command)? {
            assert!(seen.insert(part), "{part} runs in two jobs (job {name})");
        }
    }
    let gate: BTreeSet<&str> = GATE_ORDER
        .iter()
        .copied()
        .filter(|check| *check != Check::Tools)
        .map(Check::name)
        .collect();
    assert_eq!(
        seen, gate,
        "cargo xtask gate and CI run different parts (`tools` is the gate's own version check)"
    );
    Ok(())
}

// AC: TU-00.2
#[test]
fn required_contexts_are_the_jobs() -> TestResult {
    let mut contexts = BTreeSet::new();
    for file in PR_WORKFLOWS {
        for job in jobs(&workflow(file)?)? {
            assert!(
                contexts.insert(job.name.clone()),
                "{file}: the job name {} is used twice, and a status check has one job",
                job.name
            );
        }
    }
    assert!(
        !contexts.contains("settings"),
        "settings is not a required check"
    );

    let required = required_contexts_from_gate_md(&read("docs/gate.md")?);
    assert_eq!(
        contexts, required,
        "the jobs are not the checks of docs/gate.md §1"
    );

    let ruleset: serde_json::Value = serde_json::from_str(&read("gate/ruleset-main.json")?)?;
    let rules = ruleset["rules"]
        .as_array()
        .ok_or("the ruleset has no rules")?;
    let status_checks = rules
        .iter()
        .find(|rule| rule["type"] == "required_status_checks")
        .ok_or("the ruleset has no required_status_checks rule")?;
    let checks = status_checks["parameters"]["required_status_checks"]
        .as_array()
        .ok_or("required_status_checks is not a list")?;
    let in_ruleset: BTreeSet<String> = checks
        .iter()
        .map(|check| {
            let context = check["context"].as_str().ok_or("a check has no context")?;
            Ok(context.to_owned())
        })
        .collect::<TestResult<_>>()?;
    assert_eq!(
        contexts, in_ruleset,
        "the jobs are not the contexts of gate/ruleset-main.json"
    );
    Ok(())
}

// AC: TU-00.3
#[test]
fn pr_runs_both_parts_in_one_job() -> TestResult {
    let pr = workflow("pr.yml")?;
    let jobs = jobs(&pr)?;
    assert_eq!(jobs.len(), 1, "pr.yml has one job");
    let job = &jobs[0];
    assert_eq!((job.id.as_str(), job.name.as_str()), ("pr", "pr"));
    assert_eq!(job.last_command()?, "cargo xtask pr");
    let check = check_entry("pr")?;
    assert!(
        check.command.starts_with("cargo xtask pr"),
        "{}",
        check.command
    );
    assert_eq!(check.parts, ["pr-title", "pr-body"]);
    assert_eq!(
        strings(&pr["on"]["pull_request"]["types"])?,
        ["opened", "edited", "synchronize", "reopened"]
    );
    Ok(())
}

// AC: TU-00.3
#[test]
fn approval_runs_the_approval_command() -> TestResult {
    let job = job_named("approval.yml", "approval")?;
    assert_eq!(job.last_command()?, check_entry("approval")?.command);
    assert_eq!(
        step_kinds(&job)?,
        ["checkout", "checkout", "xtask"],
        "the steps of job approval"
    );
    // The two checkouts of `approval` (`docs/gate.md` §5): the base commit with its history, and,
    // only while the base has no xtask, the head of the pull request.
    let steps = job.steps()?;
    let base = &steps[0]["with"];
    assert_eq!(
        base["ref"].as_str(),
        Some("${{ github.event.pull_request.base.sha }}")
    );
    assert_eq!(base["fetch-depth"].as_i64(), Some(0));
    assert_eq!(
        steps[0]["if"],
        Yaml::BadValue,
        "the base checkout always runs"
    );
    let head = &steps[1]["with"];
    assert_eq!(
        head["ref"].as_str(),
        Some("${{ github.event.pull_request.head.sha }}")
    );
    assert_eq!(head["fetch-depth"].as_i64(), Some(0));
    assert_eq!(steps[1]["if"].as_str(), Some(APPROVAL_FALLBACK_IF));
    // `cargo xtask approval` fetches the pull request refs with `git fetch origin`, which needs
    // the credentials the checkout leaves behind.
    for step in &steps[..2] {
        assert_eq!(
            step["with"]["persist-credentials"],
            Yaml::BadValue,
            "a checkout of the approval job changes persist-credentials"
        );
    }
    Ok(())
}

/// What the workflow files may spell at their top level. A `defaults:` or an `env:` there would
/// change every command below it.
const WORKFLOW_KEYS: [&str; 5] = ["name", "on", "permissions", "concurrency", "jobs"];

/// What a job may spell. Left out on purpose: `continue-on-error` (a failed check passes),
/// `defaults` (another shell or folder), `strategy`, `needs` and `if` (a job that is skipped or
/// tolerated) and `container`. `env` and `services` are pinned by the database test.
const JOB_KEYS: [&str; 7] = [
    "name",
    "runs-on",
    "timeout-minutes",
    "permissions",
    "env",
    "services",
    "steps",
];

/// What a step of a job may spell. `env` is the token of the two steps that call GitHub, which
/// `token_reaches_only_the_steps_that_call_github` pins. Left out on purpose:
/// `continue-on-error`, `working-directory`, `shell` and `timeout-minutes`.
const STEP_KEYS: [&str; 7] = ["name", "id", "uses", "with", "run", "if", "env"];

/// What a step of the two actions may spell. `if` and `env` pass a value to a step, and the
/// shape tests below pin where they are used.
const ACTION_STEP_KEYS: [&str; 8] = ["name", "id", "uses", "with", "run", "shell", "if", "env"];

/// The steps of the workflows that have an `if:`: a cache save, on `main` only, and the checkout
/// of the head of the pull request, which runs only while the base commit has no xtask. This is
/// the expression of the latter.
const APPROVAL_FALLBACK_IF: &str = "hashFiles('xtask/Cargo.toml') == ''";

/// The keys of a mapping that are not in `allowed`.
fn unexpected_keys(node: &Yaml, allowed: &[&str]) -> TestResult<Vec<String>> {
    Ok(keys(node)?
        .into_iter()
        .filter(|key| !allowed.contains(&key.as_str()))
        .collect())
}

// AC: TU-00.2, TU-00.3
#[test]
fn no_key_lets_a_job_pass_while_xtask_fails() -> TestResult {
    for file in ALL_WORKFLOWS {
        let workflow = workflow(file)?;
        assert_eq!(
            unexpected_keys(&workflow, &WORKFLOW_KEYS)?,
            Vec::<String>::new(),
            "{file}: a key at the top of the workflow that no check needs"
        );
        for job in jobs(&workflow)? {
            let name = &job.name;
            assert_eq!(
                unexpected_keys(&job.node, &JOB_KEYS)?,
                Vec::<String>::new(),
                "{file}: job {name} has keys that can make it pass while xtask fails, or run \
                 somewhere else than the root of the repository"
            );
            for (position, step) in job.steps()?.iter().enumerate() {
                assert_eq!(
                    unexpected_keys(step, &STEP_KEYS)?,
                    Vec::<String>::new(),
                    "{file}: job {name}, step {position}: keys that can make the step pass while \
                     xtask fails, or run it somewhere else than the root of the repository"
                );
                let fallback = file == "approval.yml" && name == "approval" && position == 1;
                let saves = step_kind(step)? == "cache-save";
                match step["if"].as_str() {
                    None => assert!(!saves && !fallback, "{file}: job {name}, step {position}"),
                    Some(condition) if saves => assert!(
                        condition.starts_with("github.ref == 'refs/heads/main'"),
                        "{file}: job {name}, step {position}: saves a cache outside main"
                    ),
                    Some(condition) => assert!(
                        fallback && condition == APPROVAL_FALLBACK_IF,
                        "{file}: job {name}, step {position}: only the head checkout of approval and a cache save on main have an `if:`"
                    ),
                }
            }
        }
    }
    Ok(())
}

// AC: TU-00.2, TU-00.3
#[test]
fn no_key_lets_a_step_of_the_setup_action_fail_silently() -> TestResult {
    for path in [SETUP_ACTION, XTASK_ACTION] {
        let action = parse(path)?;
        assert_eq!(
            unexpected_keys(&action, &["name", "description", "inputs", "runs"])?,
            Vec::<String>::new(),
            "{path} has a key it does not need"
        );
        assert_eq!(
            unexpected_keys(&action["runs"], &["using", "steps"])?,
            Vec::<String>::new(),
            "the runs of {path} has a key it does not need"
        );
        for (position, step) in action["runs"]["steps"]
            .as_vec()
            .ok_or("the action has no steps")?
            .iter()
            .enumerate()
        {
            assert_eq!(
                unexpected_keys(step, &ACTION_STEP_KEYS)?,
                Vec::<String>::new(),
                "{path}, step {position}: keys that can make the step pass when it fails (such \
                 as `continue-on-error`) or run it elsewhere than the root of the repository"
            );
        }
    }
    Ok(())
}

// AC: TU-00.2, TU-00.3
#[test]
fn every_job_has_a_timeout() -> TestResult {
    let jobs = all_jobs()?;
    if jobs.len() != TIMEOUT_MINUTES.len() {
        return Err(format!(
            "{} jobs exist but TIMEOUT_MINUTES lists {}; every job needs its own entry",
            jobs.len(),
            TIMEOUT_MINUTES.len()
        )
        .into());
    }
    for (file, job) in jobs {
        let Some(&(_, _, minutes)) = TIMEOUT_MINUTES
            .iter()
            .find(|(listed, id, _)| *listed == file && *id == job.id)
        else {
            return Err(format!("{file}: job {} is not listed in TIMEOUT_MINUTES", job.id).into());
        };
        assert_eq!(
            job.node["timeout-minutes"].as_i64(),
            Some(minutes),
            "{file}: job {} has no `timeout-minutes` of {minutes}, and a hung job runs for six hours",
            job.name
        );
    }
    Ok(())
}

#[test]
fn required_jobs_always_report() -> TestResult {
    for file in PR_WORKFLOWS {
        let workflow = workflow(file)?;
        let on = &workflow["on"];
        assert!(
            keys(on)?.contains("pull_request"),
            "{file} has no pull_request trigger"
        );
        if on["pull_request"].is_hash() {
            let found = keys(&on["pull_request"])?;
            for filter in [
                "paths",
                "paths-ignore",
                "branches",
                "branches-ignore",
                "tags",
                "tags-ignore",
            ] {
                assert!(
                    !found.contains(filter),
                    "{file}: pull_request filters on {filter}, so a required check can be missing"
                );
            }
        }
        for job in jobs(&workflow)? {
            assert_eq!(
                job.node["if"],
                Yaml::BadValue,
                "{file}: job {} can be skipped, and a skipped check is not a passed one",
                job.name
            );
        }
    }
    Ok(())
}

#[test]
fn no_pull_request_target_and_least_privilege() -> TestResult {
    for file in ALL_WORKFLOWS {
        let workflow = workflow(file)?;
        let triggers = keys(&workflow["on"])?;
        assert!(
            !triggers.contains("pull_request_target"),
            "{file} runs on pull_request_target, with a token and secrets for fork code"
        );
        assert_eq!(
            string_map(&workflow["permissions"])?,
            map(&[("contents", "read")]),
            "{file}: the top-level permissions"
        );
        for job in jobs(&workflow)? {
            if file == "approval.yml" && job.name == "approval" {
                // It labels and comments on the pull request, and nothing else.
                assert_eq!(
                    string_map(&job.node["permissions"])?,
                    map(&[
                        ("contents", "read"),
                        ("pull-requests", "write"),
                        ("issues", "write"),
                    ]),
                    "the permissions of the approval job"
                );
            } else if file == "settings.yml" && job.name == "settings" {
                // A job-level `permissions:` replaces the top-level one and sets every scope it
                // does not name to none. The drift check lists the labels of the repository,
                // which needs `issues: read` in a private repository. It reads, and writes
                // nothing.
                assert_eq!(
                    string_map(&job.node["permissions"])?,
                    map(&[("contents", "read"), ("issues", "read")]),
                    "the permissions of the settings job"
                );
            } else {
                assert_eq!(
                    job.node["permissions"],
                    Yaml::BadValue,
                    "{file}: job {} sets permissions, and only approval and settings may",
                    job.name
                );
            }
        }
    }
    Ok(())
}

/// Whether a step is the one of its job that calls GitHub, the only one that gets the token:
/// the cached xtask of `approval`, and the settings check.
fn calls_github(job: &Job, step: &Yaml) -> bool {
    (job.name == "approval" && step["uses"].as_str() == Some(XTASK_USES))
        || (job.name == "settings" && step["run"].as_str() == Some("cargo xtask github check"))
}

#[test]
fn token_reaches_only_the_steps_that_call_github() -> TestResult {
    for (file, job) in all_jobs()? {
        // A job's `env` is the database URL of `test` and `coverage`, which the database test
        // pins; the token is given to the steps that need it.
        assert!(
            job.node["env"]
                .as_hash()
                .is_none_or(|env| !env.contains_key(&Yaml::String("GH_TOKEN".to_owned()))),
            "{file}: job {} gives the token to every step",
            job.name
        );
        for (position, step) in job.steps()?.iter().enumerate() {
            if calls_github(&job, step) {
                assert_eq!(
                    string_map(&step["env"])?,
                    map(&[("GH_TOKEN", TOKEN)]),
                    "{file}: job {}, step {position}: the env of a step that calls GitHub",
                    job.name
                );
            } else {
                assert_eq!(
                    step["env"],
                    Yaml::BadValue,
                    "{file}: job {}, step {position} has an env, and only the steps that call \
                     GitHub need the token",
                    job.name
                );
            }
        }
    }
    Ok(())
}

// AC: TU-00.4
#[test]
fn only_the_approval_checkouts_keep_the_git_credentials() -> TestResult {
    // Only `cargo xtask approval` talks to git with credentials (it fetches the pull request
    // refs); every other job leaves them out of `.git/config`, away from the later actions.
    for (file, job) in all_jobs()? {
        for (position, step) in job.steps()?.iter().enumerate() {
            if step_kind(step)? != "checkout" {
                continue;
            }
            let persisted = &step["with"]["persist-credentials"];
            if job.name == "approval" {
                assert_eq!(
                    *persisted,
                    Yaml::BadValue,
                    "{file}: job {}, step {position}: approval needs the default",
                    job.name
                );
            } else {
                assert_eq!(
                    persisted.as_bool(),
                    Some(false),
                    "{file}: job {}, step {position}: the checkout must not persist credentials",
                    job.name
                );
            }
        }
    }
    Ok(())
}

/// The files that hold a `uses:`: the workflows and the two actions.
fn files_with_uses() -> Vec<String> {
    let mut files: Vec<String> = ALL_WORKFLOWS
        .iter()
        .map(|file| workflow_path(file))
        .collect();
    files.push(SETUP_ACTION.to_owned());
    files.push(XTASK_ACTION.to_owned());
    files
}

/// Every `uses:` of the workflows and of the actions, as the files spell it.
fn uses_lines() -> TestResult<Vec<(String, String)>> {
    let mut found = Vec::new();
    for file in files_with_uses() {
        for line in read(&file)?.lines() {
            let line = line.trim_start();
            let line = line.strip_prefix("- ").unwrap_or(line);
            if let Some(value) = line.strip_prefix("uses:") {
                found.push((file.clone(), value.trim().to_owned()));
            }
        }
    }
    Ok(found)
}

/// The steps of an action file.
fn action_steps(path: &str) -> TestResult<Vec<Yaml>> {
    Ok(parse(path)?["runs"]["steps"]
        .as_vec()
        .ok_or_else(|| format!("{path} has no steps"))?
        .clone())
}

#[test]
fn actions_pinned_by_sha() -> TestResult {
    let found = uses_lines()?;

    // The text scan above must see every action the parsed files use.
    let mut parsed = 0;
    for (_, job) in all_jobs()? {
        parsed += job
            .steps()?
            .iter()
            .filter(|step| step["uses"].as_str().is_some())
            .count();
    }
    for path in [SETUP_ACTION, XTASK_ACTION] {
        parsed += action_steps(path)?
            .iter()
            .filter(|step| step["uses"].as_str().is_some())
            .count();
    }
    assert_eq!(
        found.len(),
        parsed,
        "a `uses:` is written in a way the scan misses"
    );

    for (file, value) in found {
        if value.starts_with("./") {
            continue;
        }
        let (action, comment) = value
            .split_once(" # ")
            .ok_or_else(|| format!("{file}: {value} has no version comment"))?;
        let (name, sha) = action
            .split_once('@')
            .ok_or_else(|| format!("{file}: {value} is not pinned"))?;
        assert!(
            name.contains('/') && !name.contains(' '),
            "{file}: {value} is not an owner/repo action"
        );
        assert!(
            sha.len() == 40 && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{file}: {value} is not pinned by a full commit SHA"
        );
        assert!(
            comment.starts_with('v') && comment[1..].starts_with(|c: char| c.is_ascii_digit()),
            "{file}: {value} does not say its version in the comment"
        );
    }
    Ok(())
}

#[test]
fn no_expressions_in_run_steps() -> TestResult {
    for (file, job) in all_jobs()? {
        for command in job.commands()? {
            assert!(
                !command.contains("${{"),
                "{file}: job {} has an expression in `run: {command}`, which a pull request can inject",
                job.name
            );
        }
    }
    for path in [SETUP_ACTION, XTASK_ACTION] {
        for command in run_values(&action_steps(path)?) {
            assert!(
                !command.contains("${{"),
                "{path} has an expression in `run: {command}`"
            );
        }
    }
    Ok(())
}

// AC: TU-00.3
#[test]
fn history_is_fetched_where_changes_or_commits_are_read() -> TestResult {
    let reads_history = [
        ("ci.yml", "gate"),
        ("ci.yml", "test"),
        ("ci.yml", "coverage"),
        ("ci.yml", "ui"),
        ("approval.yml", "approval"),
    ];
    for (file, job) in all_jobs()? {
        let depth = &job.steps()?[0]["with"]["fetch-depth"];
        if reads_history.contains(&(file, job.name.as_str())) {
            assert_eq!(
                depth.as_i64(),
                Some(0),
                "{file}: {} reads history",
                job.name
            );
        } else {
            assert_eq!(
                *depth,
                Yaml::BadValue,
                "{file}: {} fetches more than the commit it checks",
                job.name
            );
        }
    }
    Ok(())
}

#[test]
fn coverage_restores_and_saves_the_baseline_cache() -> TestResult {
    let coverage = job_named("ci.yml", "coverage")?;
    let cache = coverage
        .steps()?
        .iter()
        .find(|step| step_kind(step).is_ok_and(|kind| kind == "cache-restore"))
        .ok_or("coverage has no cache restore step")?;
    let save = coverage
        .steps()?
        .iter()
        .find(|step| step_kind(step).is_ok_and(|kind| kind == "cache-save"))
        .ok_or("coverage has no cache save step")?;
    assert_eq!(
        cache["with"]["path"].as_str(),
        Some("target/xtask/coverage/cache")
    );
    assert_eq!(save["with"]["path"], cache["with"]["path"]);
    assert!(
        save["if"]
            .as_str()
            .is_some_and(|condition| condition.starts_with("github.ref == 'refs/heads/main'")),
        "the baseline is saved on main only"
    );
    assert_eq!(
        cache["with"]["key"].as_str(),
        Some(
            "coverage-${{ github.event.pull_request.base.sha || github.sha }}-${{ \
             hashFiles('rust-toolchain.toml', 'gate/tools.toml') }}"
        )
    );
    assert_eq!(
        cache["with"]["restore-keys"].as_str().map(str::trim),
        Some("coverage-")
    );
    assert!(
        save["with"]["key"]
            .as_str()
            .is_some_and(|key| key.contains("cache-primary-key")),
        "the baseline is saved under the key it was restored by"
    );
    Ok(())
}

#[test]
fn triggers_and_concurrency_follow_the_spec() -> TestResult {
    let ci = workflow("ci.yml")?;
    assert_eq!(
        keys(&ci["on"])?,
        set(&["pull_request", "push", "workflow_dispatch"])
    );
    assert_eq!(
        strings(&ci["on"]["pull_request"]["types"])?,
        ["opened", "synchronize", "reopened"]
    );
    assert_eq!(strings(&ci["on"]["push"]["branches"])?, ["main"]);

    let pr = workflow("pr.yml")?;
    assert_eq!(keys(&pr["on"])?, set(&["pull_request"]));
    assert_eq!(
        strings(&pr["on"]["pull_request"]["types"])?,
        ["opened", "edited", "synchronize", "reopened"]
    );

    let approval = workflow("approval.yml")?;
    assert_eq!(keys(&approval["on"])?, set(&["pull_request"]));
    assert_eq!(
        strings(&approval["on"]["pull_request"]["types"])?,
        [
            "opened",
            "edited",
            "synchronize",
            "reopened",
            "labeled",
            "unlabeled"
        ]
    );

    let settings = workflow("settings.yml")?;
    assert_eq!(
        keys(&settings["on"])?,
        set(&["schedule", "workflow_dispatch"])
    );
    let schedule = settings["on"]["schedule"]
        .as_vec()
        .ok_or("schedule is not a list")?;
    assert_eq!(schedule.len(), 1, "one schedule");
    // Monday, 05:00 UTC.
    assert_eq!(schedule[0]["cron"].as_str(), Some("0 5 * * 1"));

    // Outdated runs of the CI and the PR checks are cancelled; an approval run never is, so a
    // label change is not cut off halfway.
    let group = "${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}";
    for file in ["ci.yml", "pr.yml"] {
        let concurrency = &workflow(file)?["concurrency"];
        assert_eq!(concurrency["group"].as_str(), Some(group), "{file}");
        assert_eq!(
            concurrency["cancel-in-progress"].as_str(),
            Some("${{ github.event_name == 'pull_request' }}"),
            "{file}"
        );
    }
    let concurrency = &approval["concurrency"];
    assert_eq!(
        concurrency["group"].as_str(),
        Some("${{ github.workflow }}-${{ github.event.pull_request.number }}")
    );
    assert_eq!(concurrency["cancel-in-progress"].as_bool(), Some(false));
    Ok(())
}

// AC: TU-00.3
#[test]
fn setup_action_installs_the_toolchain_the_cache_and_the_pinned_tools() -> TestResult {
    let action = parse(SETUP_ACTION)?;
    assert_eq!(action["runs"]["using"].as_str(), Some("composite"));
    let steps = action_steps(SETUP_ACTION)?;
    assert_eq!(steps.len(), 5);

    // Every command step names its shell: a composite action has no default.
    for step in steps.iter().filter(|step| step["run"].as_str().is_some()) {
        assert_eq!(step["shell"].as_str(), Some("bash"));
    }

    // The toolchain comes from `rust-toolchain.toml`. Since rustup 1.28 only `toolchain
    // install` installs it; `rustup show` does not.
    assert_eq!(
        steps[0]["run"].as_str().map(str::trim),
        Some("rustup toolchain install")
    );

    assert_eq!(action_name(&steps[1]), Some("Swatinem/rust-cache"));

    // The spec line of `gate/tools.toml` for the parts of the job goes to the install action
    // through a step output; the parts come in through `env:`, not through the script text.
    assert_eq!(steps[2]["id"].as_str(), Some("tools"));
    assert_eq!(
        string_map(&steps[2]["env"])?,
        map(&[("PARTS", "${{ inputs.parts }}")])
    );
    let command = steps[2]["run"]
        .as_str()
        .ok_or("the tools step has no command")?;
    assert!(
        command.contains("cargo xtask tools --print-install-spec --for \"$PARTS\""),
        "{command}"
    );
    assert!(
        command.contains("spec=") && command.contains("$GITHUB_OUTPUT"),
        "{command}"
    );
    assert_eq!(action_name(&steps[3]), Some("taiki-e/install-action"));
    assert_eq!(
        string_map(&steps[3]["with"])?,
        map(&[("tool", "${{ steps.tools.outputs.spec }}")])
    );
    assert_eq!(
        steps[3]["if"].as_str(),
        Some("steps.tools.outputs.spec != ''"),
        "a job whose parts need no cargo tool installs none"
    );
    Ok(())
}

/// taiki-e/install-action has no manifest for gitleaks (C-10), so the setup action downloads the
/// release archive, checks it against the release's checksums file, and takes the version from
/// `gate/tools.toml` through xtask: no workflow spells it.
#[test]
fn gitleaks_is_downloaded_and_verified_against_the_release_checksums() -> TestResult {
    let steps = action_steps(SETUP_ACTION)?;
    let step = &steps[4];
    assert_eq!(
        step["if"].as_str(),
        Some("contains(inputs.parts, 'secrets')")
    );
    assert_eq!(step["shell"].as_str(), Some("bash"));
    let command = step["run"].as_str().ok_or("no gitleaks command")?;
    for needle in [
        "cargo xtask tools --print-pin gitleaks",
        "github.com/gitleaks/gitleaks/releases/download/v${version}",
        "gitleaks_${version}_linux_x64.tar.gz",
        "gitleaks_${version}_checksums.txt",
        "sha256sum --check --strict",
        "$GITHUB_PATH",
    ] {
        assert!(command.contains(needle), "the gitleaks step lacks {needle}");
    }
    // The check runs before the archive is unpacked.
    let check = command.find("sha256sum").ok_or("no sha256sum")?;
    let unpack = command.find("tar ").ok_or("no tar")?;
    assert!(
        check < unpack,
        "the archive is unpacked before it is checked"
    );

    // The version is in `gate/tools.toml` only.
    let version = xtask::tools::load_pins(&root()?)?
        .into_iter()
        .find(|pin| pin.name == "gitleaks")
        .ok_or("gate/tools.toml pins no gitleaks")?
        .version;
    for file in files_with_uses() {
        assert!(
            !read(&file)?.contains(&version),
            "{file} spells the gitleaks version, which belongs in gate/tools.toml only"
        );
    }
    Ok(())
}

/// The inputs of the setup action reach rust-cache and the tools steps and nothing else: the key
/// of the build cache (the job by default), whether the job saves it, and the parts whose tools
/// are installed.
#[test]
fn setup_action_passes_its_inputs_on() -> TestResult {
    let action = parse(SETUP_ACTION)?;
    let inputs = &action["inputs"];
    assert_eq!(keys(inputs)?, set(&["cache-key", "save-cache", "parts"]));
    for (input, default) in [("cache-key", ""), ("save-cache", "true"), ("parts", "")] {
        assert_eq!(
            scalar(&inputs[input]["default"]).as_deref(),
            Some(default),
            "{input}"
        );
        assert_eq!(inputs[input]["required"].as_bool(), Some(false), "{input}");
    }
    assert_eq!(
        string_map(&action["runs"]["steps"][1]["with"])?,
        map(&[
            ("shared-key", "${{ inputs.cache-key || github.job }}"),
            ("save-if", "${{ inputs.save-cache }}"),
        ]),
        "the rust-cache step"
    );
    Ok(())
}

// AC: TU-00.3
#[test]
fn each_job_installs_the_tools_of_its_parts() -> TestResult {
    for name in ["gate", "test", "coverage"] {
        let job = job_named("ci.yml", name)?;
        let setup = job
            .step_using(SETUP_USES)?
            .ok_or_else(|| format!("job {name} has no setup step"))?;
        assert_eq!(
            with_of(setup)?.get("parts"),
            Some(&check_entry(name)?.parts.join(",")),
            "job {name}: the parts of the setup step are not those of .loom/checks.toml"
        );
    }
    Ok(())
}

/// Only `gate`, `test` and `coverage` build Rust, so only they set up the toolchain and a build
/// cache, and each saves that cache on `main` only: a pull request saves none, so open pull
/// requests cannot evict the caches of `main`.
#[test]
fn build_caches_are_saved_only_on_main() -> TestResult {
    for (file, job) in all_jobs()? {
        let name = job.name.as_str();
        let setup = job.step_using(SETUP_USES)?;
        match (file, name) {
            ("ci.yml", "gate" | "test" | "coverage") => {
                let setup = setup.ok_or_else(|| format!("job {name} has no setup step"))?;
                assert_eq!(
                    with_of(setup)?.get("save-cache").map(String::as_str),
                    Some(ON_MAIN),
                    "job {name}"
                );
                assert_eq!(with_of(setup)?.get("cache-key"), None, "job {name}");
            }
            ("settings.yml", "settings") => {
                let setup = setup.ok_or("settings has no setup step")?;
                assert_eq!(
                    with_of(setup)?.get("save-cache").map(String::as_str),
                    Some(ON_MAIN)
                );
            }
            _ => assert!(
                setup.is_none(),
                "{file}: job {name} uses the setup action, with its build cache"
            ),
        }
    }
    // No job saves a cache outside `main`: every save step is conditioned on it, and no step
    // restores and saves in one (`actions/cache`).
    for (file, job) in all_jobs()? {
        for step in job.steps()? {
            match step_kind(step)? {
                "cache" => panic!("{file}: job {} has a plain cache step", job.name),
                "cache-save" => assert!(
                    step["if"].as_str().is_some_and(|condition| {
                        condition.starts_with("github.ref == 'refs/heads/main'")
                    }),
                    "{file}: job {} saves a cache outside main",
                    job.name
                ),
                "node" => assert_eq!(
                    step["with"]["cache"],
                    Yaml::BadValue,
                    "{file}: job {} saves a cache through setup-node",
                    job.name
                ),
                _ => {}
            }
        }
    }
    // The cached xtask builds without a Rust build cache.
    assert!(!read(XTASK_ACTION)?.contains("rust-cache"));
    Ok(())
}

// AC: TU-00.3
#[test]
fn the_xtask_cache_is_saved_by_gate_on_main_and_restored_by_exact_key() -> TestResult {
    let action = parse(XTASK_ACTION)?;
    assert_eq!(keys(&action["inputs"])?, set(&["args", "save"]));
    assert_eq!(
        scalar(&action["inputs"]["save"]["default"]).as_deref(),
        Some("false")
    );
    let steps = action_steps(XTASK_ACTION)?;
    let uses = |name: &str| -> TestResult<&Yaml> {
        steps
            .iter()
            .find(|step| action_name(step) == Some(name))
            .ok_or_else(|| format!("the xtask action has no {name} step").into())
    };
    let restore = uses("actions/cache/restore")?;
    let save = uses("actions/cache/save")?;
    let key = restore["with"]["key"]
        .as_str()
        .ok_or("the restore step has no key")?;
    for input in [
        "runner.os",
        "'xtask/**'",
        "'Cargo.toml'",
        "'Cargo.lock'",
        "'rust-toolchain.toml'",
        "'.cargo/**'",
    ] {
        assert!(key.contains(input), "the key {key} lacks {input}");
    }
    assert!(key.contains("hashFiles("), "{key}");
    assert_eq!(
        keys(&restore["with"])?,
        set(&["path", "key"]),
        "restore by the exact key: no restore-keys"
    );
    assert_eq!(
        restore["with"]["path"].as_str(),
        Some("target/xtask-bin/debug/xtask")
    );
    assert_eq!(string_map(&save["with"])?, string_map(&restore["with"])?);
    assert_eq!(
        save["if"].as_str(),
        Some("inputs.save == 'true' && steps.restore.outputs.cache-hit != 'true'"),
        "the cache is saved only when asked to and when it was not restored"
    );
    assert_eq!(restore["id"].as_str(), Some("restore"));

    // Only `gate` asks for the save, and only on `main`; the other jobs never do.
    for (file, job) in all_jobs()? {
        let name = job.name.as_str();
        let Some(step) = job.step_using(XTASK_USES)? else {
            continue;
        };
        let with = with_of(step)?;
        if (file, name) == ("ci.yml", "gate") {
            assert_eq!(with, map(&[("save", ON_MAIN)]), "gate saves on main only");
        } else {
            assert_eq!(with.get("save"), None, "{file}: job {name} must not save");
        }
    }
    let gate = job_named("ci.yml", "gate")?;
    assert!(
        gate.step_using(XTASK_USES)?.is_some(),
        "gate does not save the xtask"
    );
    for (file, name) in [
        ("ci.yml", "ui"),
        ("pr.yml", "pr"),
        ("approval.yml", "approval"),
    ] {
        let job = job_named(file, name)?;
        let steps = job.steps()?;
        let step = steps
            .iter()
            .find(|step| step["uses"].as_str() == Some(XTASK_USES))
            .ok_or_else(|| format!("{name} does not use the cached xtask"))?;
        assert!(with_of(step)?.contains_key("args"), "{name}");
    }
    Ok(())
}

// AC: TU-00.3
#[test]
fn the_cached_xtask_is_built_on_a_miss_and_run_with_its_arguments_through_env() -> TestResult {
    let steps = action_steps(XTASK_ACTION)?;
    let commands = run_values(&steps);
    assert!(
        commands
            .iter()
            .any(|command| command.trim() == "rustup toolchain install"),
        "{commands:?}"
    );
    assert!(
        commands.iter().any(|command| command.trim()
            == "cargo build --locked -p xtask --target-dir target/xtask-bin"),
        "{commands:?}"
    );
    let run = steps.last().ok_or("no steps")?;
    assert_eq!(
        run["run"].as_str().map(str::trim),
        Some("target/xtask-bin/debug/xtask --root . $XTASK_ARGS")
    );
    assert_eq!(
        string_map(&run["env"])?,
        map(&[("XTASK_ARGS", "${{ inputs.args }}")])
    );
    // Both the toolchain and the build wait for a miss.
    for step in &steps {
        if step["run"].as_str().is_some_and(|run| {
            run.contains("rustup toolchain install") || run.contains("cargo build")
        }) {
            assert!(
                step["if"].as_str().is_some_and(
                    |condition| condition.contains("steps.restore.outputs.cache-hit != 'true'")
                ),
                "a step builds although the xtask was restored"
            );
        }
    }
    Ok(())
}

// AC: TU-00.1
#[test]
fn test_and_coverage_get_a_throwaway_postgres() -> TestResult {
    for (file, job) in all_jobs()? {
        let name = job.name.as_str();
        if file == "ci.yml" && (name == "test" || name == "coverage") {
            assert_eq!(
                string_map(&job.node["env"])?,
                map(&[("LOOM_TEST_DATABASE_URL", DATABASE_URL)]),
                "job {name}"
            );
            let postgres = &job.node["services"]["postgres"];
            assert_eq!(keys(&job.node["services"])?, set(&["postgres"]), "{name}");
            assert_eq!(postgres["image"].as_str(), Some("postgres:18"), "{name}");
            assert_eq!(
                string_map(&postgres["env"])?,
                map(&[
                    ("POSTGRES_HOST_AUTH_METHOD", "trust"),
                    ("POSTGRES_DB", "loom_test"),
                ]),
                "{name}"
            );
            assert_eq!(strings(&postgres["ports"])?, ["5432:5432"], "{name}");
            let options = postgres["options"]
                .as_str()
                .ok_or("the postgres service has no options")?;
            for needle in ["--health-cmd", "pg_isready", "--health-retries"] {
                assert!(options.contains(needle), "{name}: no {needle} in {options}");
            }
        } else {
            assert_eq!(
                job.node["services"],
                Yaml::BadValue,
                "{file}: job {name} has a service"
            );
            assert_eq!(
                job.node["env"],
                Yaml::BadValue,
                "{file}: job {name} has a job-level env"
            );
        }
    }
    Ok(())
}

// AC: TU-00.1
#[test]
fn only_the_ui_job_sets_up_node() -> TestResult {
    for (file, job) in all_jobs()? {
        let kinds = step_kinds(&job)?;
        if (file, job.name.as_str()) == ("ci.yml", "ui") {
            assert_eq!(
                kinds,
                [
                    "checkout",
                    "pnpm",
                    "node",
                    "run",
                    "cache-restore",
                    "xtask",
                    "cache-save"
                ],
                "the steps of job ui"
            );
            let steps = job.steps()?;
            // pnpm first, from the `packageManager` of the app, so that `pnpm store path` finds it.
            assert_eq!(
                string_map(&steps[1]["with"])?,
                map(&[("package_json_file", "web/package.json")])
            );
            assert_eq!(
                string_map(&steps[2]["with"])?,
                // No `cache: pnpm`: setup-node would save the store on pull requests.
                map(&[("node-version-file", "web/.nvmrc")])
            );
            assert_eq!(
                string_map(&steps[4]["with"])?["key"],
                "pnpm-store-${{ runner.os }}-${{ hashFiles('web/pnpm-lock.yaml') }}"
            );
        } else {
            assert!(
                !kinds.contains(&"pnpm") && !kinds.contains(&"node"),
                "{file}: job {} sets up Node or pnpm",
                job.name
            );
        }
    }
    Ok(())
}

// AC: TU-00.3
#[test]
fn the_jobs_have_the_steps_their_checks_need() -> TestResult {
    let shapes: [(&str, &str, &[&str]); 6] = [
        ("ci.yml", "gate", &["checkout", "setup", "xtask", "run"]),
        ("ci.yml", "test", &["checkout", "setup", "run"]),
        (
            "ci.yml",
            "coverage",
            &["checkout", "setup", "cache-restore", "run", "cache-save"],
        ),
        (
            "ci.yml",
            "ui",
            &[
                "checkout",
                "pnpm",
                "node",
                "run",
                "cache-restore",
                "xtask",
                "cache-save",
            ],
        ),
        ("pr.yml", "pr", &["checkout", "xtask"]),
        ("settings.yml", "settings", &["checkout", "setup", "run"]),
    ];
    for (file, name, expected) in shapes {
        let job = job_named(file, name)?;
        assert_eq!(
            step_kinds(&job)?,
            expected,
            "{file}: the steps of job {name}"
        );
        assert_eq!(job.runs_on()?, "ubuntu-latest", "{file}: job {name}");
    }
    // `settings` runs its own command, with the token.
    let settings = job_named("settings.yml", "settings")?;
    assert_eq!(settings.commands()?, ["cargo xtask github check"]);
    Ok(())
}
