//! The shape of the GitHub Actions workflows and of the setup action under `.github/`.
//!
//! CI is thin over xtask (P00 spec D1): a job checks out, sets up and runs `cargo xtask <check>`,
//! and the workflow files hold triggers, runners, caching and permissions and nothing else. These
//! tests parse the real files, so a change that breaks a rule of `docs/gate.md` §1 or of the
//! spec's §5.6 fails here, before it fails on GitHub.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use xtask::github::settings::required_contexts_from_gate_md;
use xtask::verdict::{GATE_ORDER, PR_ONLY};
use yaml_rust2::{Yaml, YamlLoader};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The workflows whose jobs are the required status checks of `main`.
const PR_WORKFLOWS: [&str; 3] = ["ci.yml", "pr.yml", "approval.yml"];

/// Every workflow file.
const ALL_WORKFLOWS: [&str; 5] = [
    "ci.yml",
    "pr.yml",
    "approval.yml",
    "platforms.yml",
    "settings.yml",
];

const SETUP_ACTION: &str = ".github/actions/setup/action.yml";

/// The step that runs the setup action, as a job names it.
const SETUP_USES: &str = "./.github/actions/setup";

/// The expression that is the workflow token.
const TOKEN: &str = "${{ github.token }}";

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

fn index_of(jobs: &[Job], name: &str) -> TestResult<usize> {
    jobs.iter()
        .position(|job| job.name == name)
        .ok_or_else(|| format!("no job named {name}").into())
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

/// The suffixes GitHub adds to the name of a job that runs on one OS of a matrix.
const OS_SUFFIXES: [&str; 3] = [" (windows)", " (macos)", " (macos intel)"];

/// The check a job runs: its name without the OS suffix.
fn check_of(name: &str) -> &str {
    OS_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
}

/// The runner a job of this name needs: its OS suffix says which.
fn runner_of(name: &str) -> &'static str {
    if name.ends_with(" (windows)") {
        "windows-latest"
    } else if name.ends_with(" (macos)") {
        "macos-latest"
    } else if name.ends_with(" (macos intel)") {
        "macos-15-intel"
    } else {
        "ubuntu-latest"
    }
}

/// The command a job runs: `cargo xtask <check>`, and for `settings` the settings check.
fn command_of(name: &str) -> String {
    if name == "settings" {
        "cargo xtask github check".to_owned()
    } else {
        format!("cargo xtask {}", check_of(name))
    }
}

/// What a step is, for the shape of a job: the checkout, the setup action, the cache, or a command.
fn step_kind(step: &Yaml) -> TestResult<&'static str> {
    if let Some(uses) = step["uses"].as_str() {
        return if uses.starts_with("actions/checkout@") {
            Ok("checkout")
        } else if uses == SETUP_USES {
            Ok("setup")
        } else if uses.starts_with("actions/cache@") {
            Ok("cache")
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

// AC: TU-00.1
#[test]
fn ci_builds_and_tests_on_linux() -> TestResult {
    let jobs = jobs(&workflow("ci.yml")?)?;
    for check in ["clippy", "test", "slow-tests"] {
        let job = &jobs[index_of(&jobs, check)?];
        assert_eq!(job.runs_on()?, "ubuntu-latest", "{check}");
        assert_eq!(job.commands()?, [format!("cargo xtask {check}")], "{check}");
    }
    Ok(())
}

// AC: TU-00.1
#[test]
fn platforms_builds_and_tests_on_windows_and_macos() -> TestResult {
    let jobs = jobs(&workflow("platforms.yml")?)?;
    for (os, runner) in [("windows", "windows-latest"), ("macos", "macos-latest")] {
        for check in ["clippy", "test", "slow-tests"] {
            let name = format!("{check} ({os})");
            let job = &jobs[index_of(&jobs, &name)?];
            assert_eq!(job.runs_on()?, runner, "{name}");
            assert_eq!(
                job.commands()?,
                [format!("cargo xtask {}", check_of(&name))],
                "{name}"
            );
        }
    }
    Ok(())
}

#[test]
fn engine_free_runs_on_every_pr() -> TestResult {
    let ci = workflow("ci.yml")?;
    assert!(
        keys(&ci["on"])?.contains("pull_request"),
        "ci.yml does not run on pull requests"
    );
    let jobs = jobs(&ci)?;
    let engine_free = &jobs[index_of(&jobs, "engine-free")?];
    assert_eq!(engine_free.runs_on()?, "ubuntu-latest");
    assert_eq!(engine_free.commands()?, ["cargo xtask engine-free"]);
    Ok(())
}

// AC: TU-00.2
#[test]
fn required_contexts_are_pr_jobs() -> TestResult {
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
fn every_job_only_runs_its_own_check() -> TestResult {
    for (file, job) in all_jobs()? {
        let name = &job.name;
        assert_eq!(
            job.commands()?,
            [command_of(name)],
            "{file}: job {name} runs more or other than its check"
        );
        assert_eq!(job.runs_on()?, runner_of(name), "{file}: job {name}");

        let kinds = job
            .steps()?
            .iter()
            .map(step_kind)
            .collect::<TestResult<Vec<_>>>()?;
        let expected: &[&str] = match name.as_str() {
            "approval" => &["checkout", "checkout", "setup", "run"],
            "coverage" => &["checkout", "setup", "cache", "run"],
            _ => &["checkout", "setup", "run"],
        };
        assert_eq!(kinds, expected, "{file}: the steps of job {name}");
    }

    // The two checkouts of `approval` (D3): the base commit with its history, and, only while
    // the base has no xtask, the head of the pull request.
    let approval = jobs(&workflow("approval.yml")?)?;
    let steps = approval[index_of(&approval, "approval")?].steps()?;
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
/// `defaults` (another shell or folder), `strategy` and `needs` (a job that is skipped or
/// tolerated), `container` and `services`.
const JOB_KEYS: [&str; 6] = [
    "name",
    "runs-on",
    "timeout-minutes",
    "permissions",
    "if",
    "steps",
];

/// What a step of a job may spell. `env` is the token of the two steps that call GitHub, which
/// `token_reaches_only_the_steps_that_call_github` pins. Left out on purpose:
/// `continue-on-error`, `working-directory`, `shell`, `timeout-minutes` and `id`.
const STEP_KEYS: [&str; 5] = ["uses", "with", "run", "if", "env"];

/// What a step of the setup action may spell on top of the keys of a job's step.
const SETUP_STEP_KEYS: [&str; 6] = ["name", "id", "uses", "with", "run", "shell"];

/// The expression of the one step of the workflows that has an `if:`: the checkout of the head of
/// the pull request, which runs only while the base commit has no xtask.
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
            // The only job-level conditions are the label and event conditions of the
            // `platforms` jobs, which are not required checks.
            assert_eq!(
                job.node["if"].is_badvalue(),
                file != "platforms.yml",
                "{file}: job {name}: only the platforms jobs have an `if:`"
            );
            // The token goes to two steps, never to a job: see the token test.
            for (position, step) in job.steps()?.iter().enumerate() {
                assert_eq!(
                    unexpected_keys(step, &STEP_KEYS)?,
                    Vec::<String>::new(),
                    "{file}: job {name}, step {position}: keys that can make the step pass while \
                     xtask fails, or run it somewhere else than the root of the repository"
                );
                let fallback = file == "approval.yml" && name == "approval" && position == 1;
                assert_eq!(
                    step["if"].as_str(),
                    fallback.then_some(APPROVAL_FALLBACK_IF),
                    "{file}: job {name}, step {position}: only the head checkout of approval has \
                     an `if:`"
                );
            }
        }
    }
    Ok(())
}

// AC: TU-00.2, TU-00.3
#[test]
fn no_key_lets_a_step_of_the_setup_action_fail_silently() -> TestResult {
    let action = parse(SETUP_ACTION)?;
    // `inputs` are pinned by `setup_action_passes_the_cache_inputs_to_rust_cache`.
    assert_eq!(
        unexpected_keys(&action, &["name", "description", "inputs", "runs"])?,
        Vec::<String>::new(),
        "the setup action has a key it does not need"
    );
    assert_eq!(
        unexpected_keys(&action["runs"], &["using", "steps"])?,
        Vec::<String>::new(),
        "the runs of the setup action has a key it does not need"
    );
    for (position, step) in action["runs"]["steps"]
        .as_vec()
        .ok_or("the setup action has no steps")?
        .iter()
        .enumerate()
    {
        assert_eq!(
            unexpected_keys(step, &SETUP_STEP_KEYS)?,
            Vec::<String>::new(),
            "setup action, step {position}: keys that can make the step pass when it fails (such \
             as `continue-on-error`), skip it or run it elsewhere than the root of the repository"
        );
    }
    Ok(())
}

/// The minutes a job may run before GitHub cancels it. The default is six hours. The builds and
/// the tests of one platform and the coverage run take the longest, the slow tests longer still:
/// map-scale generator runs of a minute each, on every OS.
fn timeout_of(name: &str) -> i64 {
    match name {
        _ if check_of(name) == "slow-tests" => 120,
        "test" | "coverage" => 60,
        _ if OS_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) => 60,
        _ => 30,
    }
}

// AC: TU-00.2, TU-00.3
#[test]
fn every_job_has_a_timeout() -> TestResult {
    for (file, job) in all_jobs()? {
        assert_eq!(
            job.node["timeout-minutes"].as_i64(),
            Some(timeout_of(&job.name)),
            "{file}: job {} has no `timeout-minutes` of {}, and a hung job runs for six hours",
            job.name,
            timeout_of(&job.name)
        );
    }
    Ok(())
}

// AC: TU-00.3
#[test]
fn gate_list_equals_ci_jobs_minus_pr_only() -> TestResult {
    let mut checks = BTreeSet::new();
    for file in PR_WORKFLOWS {
        for job in jobs(&workflow(file)?)? {
            checks.insert(check_of(&job.name).to_owned());
        }
    }
    for pr_only in PR_ONLY {
        assert!(
            checks.remove(pr_only.name()),
            "{} is a PR-only check without a job",
            pr_only.name()
        );
    }
    let gate: BTreeSet<String> = GATE_ORDER
        .iter()
        .map(|check| check.name())
        .filter(|name| *name != "tools")
        .map(str::to_owned)
        .collect();
    assert_eq!(
        checks, gate,
        "cargo xtask gate and CI run different checks (`tools` is the gate's own version check)"
    );
    Ok(())
}

// AC: TU-08.12
#[test]
fn platforms_runs_weekly_on_demand_and_by_label() -> TestResult {
    let platforms = workflow("platforms.yml")?;
    let on = &platforms["on"];
    assert_eq!(
        keys(on)?,
        set(&["schedule", "workflow_dispatch", "pull_request"])
    );
    let schedule = on["schedule"].as_vec().ok_or("schedule is not a list")?;
    assert_eq!(schedule.len(), 1, "one schedule");
    // Monday, 04:00 UTC.
    assert_eq!(schedule[0]["cron"].as_str(), Some("0 4 * * 1"));
    assert_eq!(strings(&on["pull_request"]["types"])?, ["labeled"]);

    let jobs = jobs(&platforms)?;
    let names: BTreeSet<String> = jobs.iter().map(|job| job.name.clone()).collect();
    assert_eq!(
        names,
        set(&[
            "clippy (windows)",
            "test (windows)",
            "slow-tests (windows)",
            "lints (windows)",
            "ac-trace (windows)",
            "clippy (macos)",
            "test (macos)",
            "test (macos intel)",
            "slow-tests (macos)",
            "slow-tests (macos intel)",
            "lints (macos)",
            "ac-trace (macos)",
        ])
    );
    for job in &jobs {
        let os = if job.name.ends_with(" (windows)") {
            "windows"
        } else {
            "macos"
        };
        assert_eq!(job.runs_on()?, runner_of(&job.name), "{}", job.name);
        // On a pull request only the label of the job's own OS starts it; the schedule and a
        // manual run always do.
        let expected =
            format!("github.event_name != 'pull_request' || github.event.label.name == '{os}'");
        assert_eq!(
            job.node["if"].as_str(),
            Some(expected.as_str()),
            "{}",
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

/// The step of each job that calls GitHub, the only one that gets the token.
const TOKEN_STEPS: [(&str, &str); 2] = [
    ("approval", "cargo xtask approval"),
    ("settings", "cargo xtask github check"),
];

#[test]
fn token_reaches_only_the_steps_that_call_github() -> TestResult {
    for (file, job) in all_jobs()? {
        assert_eq!(
            job.node["env"],
            Yaml::BadValue,
            "{file}: job {} has an env, and the token is given to the steps that need it",
            job.name
        );
        for (position, step) in job.steps()?.iter().enumerate() {
            let calls_github = TOKEN_STEPS
                .iter()
                .any(|(name, run)| job.name == *name && step["run"].as_str() == Some(run));
            if calls_github {
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

/// Every `uses:` of the workflows and of the setup action, as the files spell it.
fn uses_lines() -> TestResult<Vec<(String, String)>> {
    let mut files: Vec<String> = ALL_WORKFLOWS
        .iter()
        .map(|file| workflow_path(file))
        .collect();
    files.push(SETUP_ACTION.to_owned());
    let mut found = Vec::new();
    for file in files {
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
    for step in parse(SETUP_ACTION)?["runs"]["steps"]
        .as_vec()
        .ok_or("the setup action has no steps")?
    {
        parsed += usize::from(step["uses"].as_str().is_some());
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
    let action = parse(SETUP_ACTION)?;
    let steps = action["runs"]["steps"].as_vec().ok_or("no steps")?;
    for command in run_values(steps) {
        assert!(
            !command.contains("${{"),
            "the setup action has an expression in `run: {command}`"
        );
    }
    Ok(())
}

/// The checks that read history: the commits of a pull request, or the merge base with its base.
/// `slow-tests` reads the changes since the merge base to decide whether its tests run.
const READ_HISTORY: [&str; 3] = ["commit-msg", "coverage", "slow-tests"];

#[test]
fn history_is_fetched_where_commits_or_the_merge_base_are_read() -> TestResult {
    for file in ["ci.yml", "platforms.yml"] {
        for job in jobs(&workflow(file)?)? {
            let depth = &job.steps()?[0]["with"]["fetch-depth"];
            if READ_HISTORY.contains(&check_of(&job.name)) {
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
    }
    Ok(())
}

#[test]
fn coverage_restores_and_saves_the_baseline_cache() -> TestResult {
    let ci = jobs(&workflow("ci.yml")?)?;
    let coverage = &ci[index_of(&ci, "coverage")?];
    let cache = coverage
        .steps()?
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/cache@"))
        })
        .ok_or("coverage has no cache step")?;
    assert_eq!(
        cache["with"]["path"].as_str(),
        Some("target/xtask/coverage/cache")
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

/// The action a step uses, without its version.
fn action_name(step: &Yaml) -> Option<&str> {
    step["uses"]
        .as_str()
        .and_then(|uses| uses.split_once('@'))
        .map(|(name, _)| name)
}

#[test]
fn setup_action_installs_the_toolchain_the_cache_and_the_pinned_tools() -> TestResult {
    let action = parse(SETUP_ACTION)?;
    assert_eq!(action["runs"]["using"].as_str(), Some("composite"));
    let steps = action["runs"]["steps"]
        .as_vec()
        .ok_or("the action has no steps")?;
    assert_eq!(steps.len(), 4);

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

    // The spec line of `gate/tools.toml` goes to the install action through a step output.
    assert_eq!(steps[2]["id"].as_str(), Some("tools"));
    let command = steps[2]["run"]
        .as_str()
        .ok_or("the tools step has no command")?;
    assert!(
        command.contains("cargo xtask tools --print-install-spec"),
        "{command}"
    );
    assert!(
        command.contains("spec=") && command.contains("$GITHUB_OUTPUT"),
        "{command}"
    );
    assert_eq!(action_name(&steps[3]), Some("taiki-e/install-action"));
    assert_eq!(
        steps[3]["with"]["tool"].as_str(),
        Some("${{ steps.tools.outputs.spec }}")
    );
    Ok(())
}

/// The two inputs of the setup action reach rust-cache and nothing else: the key of jobs that
/// build the same thing and share one cache, and whether the job saves the cache. Their defaults
/// keep rust-cache's own behaviour, a cache per job that every run saves.
#[test]
fn setup_action_passes_the_cache_inputs_to_rust_cache() -> TestResult {
    let action = parse(SETUP_ACTION)?;
    let inputs = &action["inputs"];
    assert_eq!(keys(inputs)?, set(&["cache-key", "save-cache"]));
    for (input, default) in [("cache-key", ""), ("save-cache", "true")] {
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
            ("shared-key", "${{ inputs.cache-key }}"),
            ("save-if", "${{ inputs.save-cache }}"),
        ]),
        "the rust-cache step"
    );
    Ok(())
}

/// `approval` runs on pull requests only, so a cache of its own is never saved on `main`, the one
/// place every pull request restores from, and each of its runs built xtask from scratch. It
/// restores the cache of `fmt` instead, which builds the same xtask, and saves nothing. `fmt`
/// saves that cache on `main` only: a pull request is searched first, so a cache a pull request
/// saved would win over the one of `main`, in the one job that holds a token that can label.
#[test]
fn approval_restores_the_xtask_cache_that_main_saves() -> TestResult {
    let shared = [
        (
            "ci.yml",
            "fmt",
            map(&[
                ("cache-key", "xtask"),
                ("save-cache", "${{ github.ref == 'refs/heads/main' }}"),
            ]),
        ),
        (
            "approval.yml",
            "approval",
            map(&[("cache-key", "xtask"), ("save-cache", "false")]),
        ),
    ];
    for (file, job) in all_jobs()? {
        let name = &job.name;
        let setup = job
            .steps()?
            .iter()
            .find(|step| step["uses"].as_str() == Some(SETUP_USES))
            .ok_or_else(|| format!("{file}: job {name} has no setup step"))?;
        let expected = shared
            .iter()
            .find(|(in_file, job_name, _)| *in_file == file && job_name == name)
            .map(|(_, _, with)| with);
        match expected {
            Some(with) => assert_eq!(&string_map(&setup["with"])?, with, "{file}: job {name}"),
            None => assert_eq!(
                setup["with"],
                Yaml::BadValue,
                "{file}: job {name} passes inputs to the setup action"
            ),
        }
    }
    Ok(())
}
