//! GitHub, as far as the gate needs it: the labels and comments of a pull request, and the
//! rulesets, settings and labels of the repository.
//!
//! Checks talk to the [`PullRequests`] trait, and [`settings`] to [`RepoAdmin`]. [`gh_cli::GhCli`]
//! is the real one for both, which calls the `gh` command line; `fake::FakeGitHub` is a stand-in
//! that records the calls, for tests.
//!
//! Whatever a pull request wrote (a comment, a label, a title) is data. An [`Error`] is printed
//! raw to stderr, which the runner reads for workflow commands, so an error from GitHub names the
//! request and the status and nothing GitHub sent back.

use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result};

pub mod gh_cli;
pub mod settings;

#[cfg(test)]
pub(crate) mod fake;

/// A comment on a pull request.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Comment {
    /// The login of the account that wrote it, such as `github-actions[bot]`.
    pub author: String,
    /// The text, as written.
    pub body: String,
    /// When it was written, as GitHub gives it: `2026-10-31T12:00:00Z`.
    pub created_at: String,
    /// When it was last edited. The same as `created_at` for a comment that was never edited.
    pub updated_at: String,
}

/// How a label of the repository looks.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelSpec {
    /// The name, such as `needs-jann`.
    pub name: String,
    /// The colour as six hex digits, without `#`.
    pub color: String,
    /// What the label means, in a few words.
    pub description: String,
}

/// The labels and comments of pull requests.
pub trait PullRequests {
    /// The names of the labels on pull request `pr`, as they are now.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn labels(&self, pr: u64) -> Result<Vec<String>>;

    /// The id of the commit at the head of pull request `pr`, as it is now.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached, refuses the request or answers with something else
    /// than a pull request.
    fn head_sha(&self, pr: u64) -> Result<String>;

    /// Puts the label `name` on pull request `pr`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request, for instance when the label
    /// does not exist.
    fn add_label(&self, pr: u64, name: &str) -> Result<()>;

    /// Takes the label `name` off pull request `pr`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request, for instance when the pull
    /// request does not have the label.
    fn remove_label(&self, pr: u64, name: &str) -> Result<()>;

    /// Makes sure the repository has a label called `spec.name`, and creates it as `spec` says
    /// when it does not. An existing label is left as it is.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn ensure_label(&self, spec: &LabelSpec) -> Result<()>;

    /// Every comment on pull request `pr`, from all pages, oldest first.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached, refuses the request or answers with something else
    /// than comments.
    fn comments(&self, pr: u64) -> Result<Vec<Comment>>;

    /// Writes a new comment on pull request `pr`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn comment(&self, pr: u64, body: &str) -> Result<()>;
}

/// The settings of the repository: its rulesets, its merge settings and its labels. The files in
/// `gate/` say how they should be; [`settings`] compares and applies.
pub trait RepoAdmin {
    /// The id and the name of every ruleset of the repository, not those of its organisation.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request, for instance because the plan
    /// has no rulesets.
    fn rulesets(&self) -> Result<Vec<(u64, String)>>;

    /// The ruleset `id` as GitHub describes it.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached, refuses the request or answers with something else
    /// than a ruleset.
    fn ruleset(&self, id: u64) -> Result<Value>;

    /// Creates a ruleset from `body`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn create_ruleset(&self, body: &Value) -> Result<()>;

    /// Replaces the ruleset `id` with `body`.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request, for instance when there is no
    /// such ruleset.
    fn update_ruleset(&self, id: u64, body: &Value) -> Result<()>;

    /// The repository as GitHub describes it, with its merge settings.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached, refuses the request or answers with something else
    /// than a repository.
    fn repo(&self) -> Result<Value>;

    /// Changes the settings of the repository that `body` names and leaves the others.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn patch_repo(&self, body: &Value) -> Result<()>;

    /// Every label of the repository, from all pages. Named so that it does not clash with
    /// [`PullRequests::labels`], which is the labels of one pull request.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached, refuses the request or answers with something else
    /// than labels.
    fn repo_labels(&self) -> Result<Vec<LabelSpec>>;

    /// Makes the repository have a label that looks like `spec`: it creates the label, or changes
    /// the colour, the description and the spelling of the name of the one it has.
    ///
    /// # Errors
    ///
    /// Fails when GitHub cannot be reached or refuses the request.
    fn upsert_label(&self, spec: &LabelSpec) -> Result<()>;
}

/// The error for a request that failed. `request` is `METHOD endpoint`; `what` is one of our own
/// phrases, never text that came back from GitHub.
pub(crate) fn failed(request: &str, what: &str) -> Error {
    Error::GitHub {
        message: format!("{request}: {what}"),
    }
}
