//! The GitHub Actions event a run belongs to.
//!
//! Workflow steps contain no `${{ }}` expressions, so no pull request text reaches a shell.
//! Instead a check reads the event payload from `$GITHUB_EVENT_PATH`: the title, the body, the
//! number, the base and head commits and the head branch of a pull request, or the commits of a
//! push. Outside GitHub Actions there is no event and the checks take their inputs from flags
//! and from git.

use std::path::PathBuf;

use serde::Deserialize;

use crate::{Error, Result};

/// The event that started the workflow run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CiEvent {
    /// A `pull_request` event.
    PullRequest(PullRequestEvent),
    /// A `push` event.
    Push(PushEvent),
    /// Any other event, such as `workflow_dispatch` or `schedule`.
    Other,
}

/// What the checks need from a `pull_request` event.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PullRequestEvent {
    /// What happened to the pull request: `opened`, `edited`, `labeled`, and so on.
    pub action: String,
    /// The pull request number.
    pub number: u64,
    /// The title.
    pub title: String,
    /// The description, `None` when it is empty.
    pub body: Option<String>,
    /// The tip of the target branch the pull request was opened against.
    pub base_sha: String,
    /// The tip of the pull request branch.
    pub head_sha: String,
    /// The name of the pull request branch.
    pub head_ref: String,
    /// The label that was added or removed, for the actions `labeled` and `unlabeled`.
    pub label: Option<String>,
}

/// What the checks need from a `push` event.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PushEvent {
    /// The tip of the branch before the push; all zeros for a new branch.
    pub before: String,
    /// The tip of the branch after the push.
    pub after: String,
}

/// The event of the current run, or `None` when `GITHUB_ACTIONS` is not `true`.
///
/// # Errors
///
/// Fails under GitHub Actions when `GITHUB_EVENT_NAME` or `GITHUB_EVENT_PATH` is not set, the
/// payload file cannot be read, or the payload does not have the expected shape.
pub fn from_env() -> Result<Option<CiEvent>> {
    from_vars(|name| std::env::var(name).ok())
}

fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<CiEvent>> {
    if var("GITHUB_ACTIONS").as_deref() != Some("true") {
        return Ok(None);
    }
    let required = |name: &str| {
        var(name).ok_or_else(|| Error::Missing {
            what: format!("environment variable {name} under GitHub Actions"),
        })
    };
    let name = required("GITHUB_EVENT_NAME")?;
    let path = PathBuf::from(required("GITHUB_EVENT_PATH")?);
    let json = std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
    parse_event(&name, &json).map(Some)
}

/// Reads the payload `json` of the event called `name` (the value of `GITHUB_EVENT_NAME`).
///
/// Events other than `pull_request` and `push` are not read and give [`CiEvent::Other`].
///
/// # Errors
///
/// Fails when the payload of a `pull_request` or `push` event is not JSON or lacks a field the
/// checks need.
pub fn parse_event(name: &str, json: &str) -> Result<CiEvent> {
    match name {
        "pull_request" => {
            let payload: PullRequestPayload = payload(name, json)?;
            let pull_request = payload.pull_request;
            Ok(CiEvent::PullRequest(PullRequestEvent {
                action: payload.action,
                number: pull_request.number,
                title: pull_request.title,
                body: pull_request.body,
                base_sha: pull_request.base.sha,
                head_sha: pull_request.head.sha,
                head_ref: pull_request.head.name,
                label: payload.label.map(|label| label.name),
            }))
        }
        "push" => {
            let payload: PushPayload = payload(name, json)?;
            Ok(CiEvent::Push(PushEvent {
                before: payload.before,
                after: payload.after,
            }))
        }
        _ => Ok(CiEvent::Other),
    }
}

fn payload<'a, T: Deserialize<'a>>(name: &str, json: &'a str) -> Result<T> {
    serde_json::from_str(json).map_err(|error| Error::Parse {
        what: format!("`{name}` event payload"),
        message: error.to_string(),
    })
}

#[derive(Deserialize)]
struct PullRequestPayload {
    action: String,
    label: Option<Label>,
    pull_request: PullRequestBody,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct PullRequestBody {
    number: u64,
    title: String,
    body: Option<String>,
    base: Side,
    head: Side,
}

/// One side of a pull request: the branch and its tip.
#[derive(Deserialize)]
struct Side {
    #[serde(rename = "ref")]
    name: String,
    sha: String,
}

#[derive(Deserialize)]
struct PushPayload {
    before: String,
    after: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::error::Error as _;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    const PULL_REQUEST: &str = include_str!("../tests/fixtures/events/pull_request.json");

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn parses_pull_request_event() -> TestResult {
        let CiEvent::PullRequest(event) = parse_event("pull_request", PULL_REQUEST)? else {
            return Err("not a pull request event".into());
        };
        assert_eq!(event.action, "labeled");
        assert_eq!(event.number, 7);
        assert_eq!(
            event.title,
            "feat(xtask): add the verdict, report, ci, branch and git core (P00)"
        );
        assert_eq!(event.body, None);
        assert_eq!(event.base_sha, "60a2dac1f0b5e3a8c9d47e6b2a1f08d3c5b79e41");
        assert_eq!(event.head_sha, "7d646e6b3c1a90f2e48d5b7a6c0e1f39a2d84b57");
        assert_eq!(event.head_ref, "phase/P00-foundation-and-gate");
        assert_eq!(event.label.as_deref(), Some("approved"));
        Ok(())
    }

    #[test]
    fn pull_request_without_label_or_body_text() -> TestResult {
        let json = r#"{"action":"opened","number":3,"pull_request":{"number":3,"title":"t","body":"b",
            "base":{"ref":"main","sha":"1"},"head":{"ref":"fix/x","sha":"2"}}}"#;
        let CiEvent::PullRequest(event) = parse_event("pull_request", json)? else {
            return Err("not a pull request event".into());
        };
        assert_eq!(event.action, "opened");
        assert_eq!(event.body.as_deref(), Some("b"));
        assert_eq!(event.label, None);
        assert_eq!(event.head_ref, "fix/x");
        Ok(())
    }

    #[test]
    fn parses_push_event() -> TestResult {
        let json = r#"{"ref":"refs/heads/main","before":"aaaa","after":"bbbb","repository":{}}"#;
        let CiEvent::Push(event) = parse_event("push", json)? else {
            return Err("not a push event".into());
        };
        assert_eq!(event.before, "aaaa");
        assert_eq!(event.after, "bbbb");
        Ok(())
    }

    #[test]
    fn other_events_are_other() -> TestResult {
        assert!(matches!(
            parse_event("workflow_dispatch", "{}")?,
            CiEvent::Other
        ));
        assert!(matches!(
            parse_event("schedule", "not json")?,
            CiEvent::Other
        ));
        Ok(())
    }

    #[test]
    fn a_payload_that_does_not_fit_is_a_parse_error() {
        for (name, json) in [
            ("pull_request", "{}"),
            ("pull_request", "not json"),
            ("push", r#"{"before":"a"}"#),
        ] {
            let result = parse_event(name, json);
            assert!(
                matches!(&result, Err(Error::Parse { what, .. }) if what.contains(name)),
                "{name}: {result:?}"
            );
        }
    }

    #[test]
    fn there_is_no_event_outside_github_actions() -> TestResult {
        assert!(from_vars(vars(&[]))?.is_none());
        assert!(from_vars(vars(&[("GITHUB_ACTIONS", "false")]))?.is_none());
        assert!(
            from_vars(vars(&[
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_EVENT_PATH", "/nowhere")
            ]))?
            .is_none()
        );
        Ok(())
    }

    #[test]
    fn the_event_comes_from_the_payload_file() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("event.json");
        std::fs::write(&path, PULL_REQUEST)?;
        let path = path.to_str().ok_or("temp path is not UTF-8")?;
        let event = from_vars(vars(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_EVENT_PATH", path),
        ]))?;
        assert!(matches!(event, Some(CiEvent::PullRequest(ref e)) if e.number == 7));
        Ok(())
    }

    #[test]
    fn github_actions_without_an_event_is_an_error() -> TestResult {
        let no_name = from_vars(vars(&[("GITHUB_ACTIONS", "true")]));
        assert!(
            matches!(&no_name, Err(Error::Missing { what }) if what.contains("GITHUB_EVENT_NAME")),
            "{no_name:?}"
        );
        let no_path = from_vars(vars(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "push"),
        ]));
        assert!(
            matches!(&no_path, Err(Error::Missing { what }) if what.contains("GITHUB_EVENT_PATH")),
            "{no_path:?}"
        );
        let dir = tempfile::tempdir()?;
        let gone = dir.path().join("gone.json");
        let gone = gone.to_str().ok_or("temp path is not UTF-8")?;
        let unreadable = from_vars(vars(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_EVENT_PATH", gone),
        ]));
        let error = unreadable.err().ok_or("expected an error")?;
        assert!(matches!(error, Error::Io { .. }), "{error}");
        assert!(error.source().is_some());
        Ok(())
    }
}
