//! A GitHub that lives in memory and remembers every call, for tests.
//!
//! The labels and comments change as the real ones would: a label that is added is there for the
//! next read, a comment that is written is by `github-actions[bot]` and was never edited. The
//! rulesets, the settings and the labels of the repository change the same way: a ruleset that is
//! created is there for the next read, with the keys GitHub adds to what it is given. The calls
//! are kept in order, so a test can say exactly what a check did.

use std::cell::RefCell;

use serde_json::{Map, Value, json};

use super::{Comment, LabelSpec, PullRequests, RepoAdmin, failed};
use crate::Result;

/// What a check asked of GitHub.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Call {
    /// `labels(pr)`.
    Labels(u64),
    /// `head_sha(pr)`.
    HeadSha(u64),
    /// `add_label(pr, name)`.
    AddLabel(u64, String),
    /// `remove_label(pr, name)`.
    RemoveLabel(u64, String),
    /// `ensure_label(spec)`.
    EnsureLabel(LabelSpec),
    /// `comments(pr)`.
    Comments(u64),
    /// `comment(pr, body)`.
    Comment(u64, String),
    /// `rulesets()`.
    Rulesets,
    /// `ruleset(id)`.
    Ruleset(u64),
    /// `create_ruleset(body)`.
    CreateRuleset(Value),
    /// `update_ruleset(id, body)`.
    UpdateRuleset(u64, Value),
    /// `repo()`.
    Repo,
    /// `patch_repo(body)`.
    PatchRepo(Value),
    /// `repo_labels()`.
    RepoLabels,
    /// `upsert_label(spec)`.
    UpsertLabel(LabelSpec),
}

#[derive(Default)]
struct State {
    labels: Vec<String>,
    comments: Vec<Comment>,
    calls: Vec<Call>,
    comments_fail: bool,
    head: Option<String>,
    rulesets: Vec<(u64, Value)>,
    repo: Map<String, Value>,
    repo_labels: Vec<LabelSpec>,
}

/// A recording stand-in for GitHub, with one pull request's labels and comments, and a repository
/// that has no rulesets, no settings and no labels until a test gives it some.
#[derive(Default)]
pub struct FakeGitHub {
    state: RefCell<State>,
}

impl FakeGitHub {
    /// A pull request with no labels and no comments.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The pull request starts with these labels.
    #[must_use]
    pub fn with_labels(self, names: &[&str]) -> Self {
        self.state.borrow_mut().labels = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// The pull request starts with this comment, after the ones it has.
    #[must_use]
    pub fn with_comment(self, comment: Comment) -> Self {
        self.state.borrow_mut().comments.push(comment);
        self
    }

    /// The pull request has this commit at its head now. Without it, reading the head fails.
    #[must_use]
    pub fn with_head(self, sha: &str) -> Self {
        self.state.borrow_mut().head = Some(sha.to_owned());
        self
    }

    /// The head of the pull request moves to this commit.
    pub fn set_head(&self, sha: &str) {
        self.state.borrow_mut().head = Some(sha.to_owned());
    }

    /// Writing a comment fails, as it does when GitHub is down.
    #[must_use]
    pub fn with_failing_comments(self) -> Self {
        self.state.borrow_mut().comments_fail = true;
        self
    }

    /// The repository starts with these settings, as GitHub describes them.
    #[must_use]
    pub fn with_repo(self, repo: &Value) -> Self {
        self.state.borrow_mut().repo = repo.as_object().cloned().unwrap_or_default();
        self
    }

    /// The repository starts with these labels.
    #[must_use]
    pub fn with_repo_labels(self, labels: &[LabelSpec]) -> Self {
        self.state.borrow_mut().repo_labels = labels.to_vec();
        self
    }

    /// Every call so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }

    /// The calls that changed something: all but the reads.
    #[must_use]
    pub fn writes(&self) -> Vec<Call> {
        let mut calls = self.calls();
        calls.retain(|call| {
            !matches!(
                call,
                Call::Labels(_)
                    | Call::HeadSha(_)
                    | Call::Comments(_)
                    | Call::Rulesets
                    | Call::Ruleset(_)
                    | Call::Repo
                    | Call::RepoLabels
            )
        });
        calls
    }

    /// The labels on the pull request now.
    #[must_use]
    pub fn current_labels(&self) -> Vec<String> {
        self.state.borrow().labels.clone()
    }

    /// The comments on the pull request now.
    #[must_use]
    pub fn current_comments(&self) -> Vec<Comment> {
        self.state.borrow().comments.clone()
    }

    /// The rulesets of the repository now, with their ids.
    #[must_use]
    pub fn current_rulesets(&self) -> Vec<(u64, Value)> {
        self.state.borrow().rulesets.clone()
    }

    /// The labels of the repository now.
    #[must_use]
    pub fn current_repo_labels(&self) -> Vec<LabelSpec> {
        self.state.borrow().repo_labels.clone()
    }
}

/// `body` as GitHub keeps a ruleset: the same, and with keys that nobody asked for.
fn stored(id: u64, body: &Value) -> Value {
    let mut ruleset = body.as_object().cloned().unwrap_or_default();
    ruleset.insert("id".to_owned(), json!(id));
    ruleset.insert("source_type".to_owned(), json!("Repository"));
    ruleset.insert("current_user_can_bypass".to_owned(), json!("never"));
    Value::Object(ruleset)
}

fn name_of(ruleset: &Value) -> &str {
    ruleset["name"].as_str().unwrap_or_default()
}

impl PullRequests for FakeGitHub {
    fn labels(&self, pr: u64) -> Result<Vec<String>> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Labels(pr));
        Ok(state.labels.clone())
    }

    fn head_sha(&self, pr: u64) -> Result<String> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::HeadSha(pr));
        state
            .head
            .clone()
            .ok_or_else(|| failed("GET pull request", "HTTP 404"))
    }

    fn add_label(&self, pr: u64, name: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::AddLabel(pr, name.to_owned()));
        if !state.labels.iter().any(|label| label == name) {
            state.labels.push(name.to_owned());
        }
        Ok(())
    }

    fn remove_label(&self, pr: u64, name: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::RemoveLabel(pr, name.to_owned()));
        // GitHub answers 404 for a label the pull request does not have.
        let Some(index) = state.labels.iter().position(|label| label == name) else {
            return Err(failed("DELETE label", "HTTP 404"));
        };
        state.labels.remove(index);
        Ok(())
    }

    fn ensure_label(&self, spec: &LabelSpec) -> Result<()> {
        self.state
            .borrow_mut()
            .calls
            .push(Call::EnsureLabel(spec.clone()));
        Ok(())
    }

    fn comments(&self, pr: u64) -> Result<Vec<Comment>> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Comments(pr));
        Ok(state.comments.clone())
    }

    fn comment(&self, pr: u64, body: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Comment(pr, body.to_owned()));
        if state.comments_fail {
            return Err(failed("POST comment", "HTTP 503"));
        }
        // One second apart, so that "newest" is well defined.
        let time = format!("2030-01-01T00:00:{:02}Z", state.comments.len());
        state.comments.push(Comment {
            author: "github-actions[bot]".to_owned(),
            body: body.to_owned(),
            created_at: time.clone(),
            updated_at: time,
        });
        Ok(())
    }
}

impl RepoAdmin for FakeGitHub {
    fn rulesets(&self) -> Result<Vec<(u64, String)>> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Rulesets);
        Ok(state
            .rulesets
            .iter()
            .map(|(id, ruleset)| (*id, name_of(ruleset).to_owned()))
            .collect())
    }

    fn ruleset(&self, id: u64) -> Result<Value> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Ruleset(id));
        let found = state.rulesets.iter().find(|(known, _)| *known == id);
        found
            .map(|(_, ruleset)| ruleset.clone())
            .ok_or_else(|| failed("GET ruleset", "HTTP 404"))
    }

    fn create_ruleset(&self, body: &Value) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::CreateRuleset(body.clone()));
        // GitHub answers 422 for a name that is taken.
        if state
            .rulesets
            .iter()
            .any(|(_, ruleset)| name_of(ruleset) == name_of(body))
        {
            return Err(failed("POST ruleset", "HTTP 422"));
        }
        let id = state.rulesets.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
        state.rulesets.push((id, stored(id, body)));
        Ok(())
    }

    fn update_ruleset(&self, id: u64, body: &Value) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::UpdateRuleset(id, body.clone()));
        let Some((_, ruleset)) = state.rulesets.iter_mut().find(|(known, _)| *known == id) else {
            return Err(failed("PUT ruleset", "HTTP 404"));
        };
        *ruleset = stored(id, body);
        Ok(())
    }

    fn repo(&self) -> Result<Value> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::Repo);
        Ok(Value::Object(state.repo.clone()))
    }

    fn patch_repo(&self, body: &Value) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::PatchRepo(body.clone()));
        let Some(changes) = body.as_object() else {
            return Err(failed("PATCH repo", "HTTP 422"));
        };
        for (key, value) in changes {
            state.repo.insert(key.clone(), value.clone());
        }
        Ok(())
    }

    fn repo_labels(&self) -> Result<Vec<LabelSpec>> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::RepoLabels);
        Ok(state.repo_labels.clone())
    }

    fn upsert_label(&self, spec: &LabelSpec) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::UpsertLabel(spec.clone()));
        // Names are unique whatever their case, and a change sets the spelling.
        let known = state
            .repo_labels
            .iter_mut()
            .find(|label| label.name.eq_ignore_ascii_case(&spec.name));
        match known {
            Some(label) => *label = spec.clone(),
            None => state.repo_labels.push(spec.clone()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_keeps_labels_and_comments_and_the_order_of_calls() -> crate::Result<()> {
        let gh = FakeGitHub::new().with_labels(&["needs-jann"]);
        gh.add_label(7, "approved")?;
        gh.add_label(7, "approved")?;
        assert_eq!(gh.labels(7)?, ["needs-jann", "approved"]);
        gh.remove_label(7, "approved")?;
        assert!(gh.remove_label(7, "approved").is_err());
        gh.comment(7, "one")?;
        gh.comment(7, "two")?;
        let comments = gh.comments(7)?;
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].author, "github-actions[bot]");
        assert_eq!(comments[0].created_at, comments[0].updated_at);
        assert!(comments[0].created_at < comments[1].created_at);
        let spec = LabelSpec {
            name: "x".to_owned(),
            color: "000000".to_owned(),
            description: String::new(),
        };
        gh.ensure_label(&spec)?;
        assert_eq!(
            gh.writes(),
            [
                Call::AddLabel(7, "approved".to_owned()),
                Call::AddLabel(7, "approved".to_owned()),
                Call::RemoveLabel(7, "approved".to_owned()),
                Call::RemoveLabel(7, "approved".to_owned()),
                Call::Comment(7, "one".to_owned()),
                Call::Comment(7, "two".to_owned()),
                Call::EnsureLabel(spec),
            ]
        );
        assert_eq!(
            gh.calls().len(),
            9,
            "seven writes, a read of labels and one of comments"
        );
        assert_eq!(gh.current_labels(), ["needs-jann"]);
        Ok(())
    }

    #[test]
    fn the_fake_keeps_rulesets_settings_and_labels_as_github_does() -> crate::Result<()> {
        let gh = FakeGitHub::new().with_repo(&json!({"allow_auto_merge": false, "name": "r"}));
        gh.create_ruleset(&json!({"name": "main", "enforcement": "active"}))?;
        gh.create_ruleset(&json!({"name": "tags"}))?;
        assert!(gh.create_ruleset(&json!({"name": "main"})).is_err());
        assert_eq!(
            gh.rulesets()?,
            [(1, "main".to_owned()), (2, "tags".to_owned())]
        );
        let main = gh.ruleset(1)?;
        assert_eq!(main["enforcement"], "active");
        assert_eq!(main["source_type"], "Repository", "keys nobody asked for");
        gh.update_ruleset(1, &json!({"name": "main", "enforcement": "disabled"}))?;
        assert_eq!(gh.ruleset(1)?["enforcement"], "disabled");
        assert!(gh.update_ruleset(9, &json!({})).is_err());
        assert!(gh.ruleset(9).is_err());

        gh.patch_repo(&json!({"allow_auto_merge": true}))?;
        assert_eq!(gh.repo()?, json!({"allow_auto_merge": true, "name": "r"}));
        assert!(gh.patch_repo(&json!([])).is_err());

        let label = |name: &str, color: &str| LabelSpec {
            name: name.to_owned(),
            color: color.to_owned(),
            description: String::new(),
        };
        gh.upsert_label(&label("bug", "111111"))?;
        gh.upsert_label(&label("Bug", "222222"))?;
        gh.upsert_label(&label("new", "333333"))?;
        assert_eq!(
            gh.repo_labels()?,
            [label("Bug", "222222"), label("new", "333333")]
        );
        assert_eq!(gh.current_repo_labels(), gh.repo_labels()?);
        assert_eq!(gh.current_rulesets().len(), 2);
        // Three creates (one refused), two updates (one refused), two patches (one refused) and
        // three labels; the reads in between are not counted.
        assert_eq!(gh.writes().len(), 3 + 2 + 2 + 3);
        Ok(())
    }

    #[test]
    fn a_failing_fake_refuses_to_write_a_comment() {
        let gh = FakeGitHub::new().with_failing_comments();
        assert!(gh.comment(7, "x").is_err());
        assert_eq!(gh.current_comments(), Vec::<Comment>::new());
        assert_eq!(gh.calls(), [Call::Comment(7, "x".to_owned())]);
    }
}
