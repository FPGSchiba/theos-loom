//! The `gh` command line as the GitHub client: every call is `gh api <endpoint>`.
//!
//! `gh` is started with an argument list, never through a shell, and takes its token from
//! `GH_TOKEN` in the environment it inherits, or else uses the login it has stored. It has no
//! stdin, except for a call with a JSON body, which `gh` reads from there. The adapter is thin on
//! purpose: a call is described as a `Request` (which the tests read without running anything),
//! `gh` runs it, and the answer is read as JSON. What a pull request wrote never becomes part of
//! an error: a failed call is reported as `METHOD endpoint: HTTP <status>`, or the exit code of
//! `gh` when it said no status. The status is read from `gh`'s stderr as at most three digits,
//! and the rest of stderr is dropped, because it can quote the request.

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Comment, LabelSpec, PullRequests, RepoAdmin, failed};
use crate::git::Git;
use crate::{Error, Result};

/// The entries GitHub is asked for on each page of a list; its largest.
const PAGE_SIZE: &str = "per_page=100";

/// GitHub through the `gh` command line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GhCli {
    /// The repository as `owner/name`.
    pub repo: String,
}

/// One call of `gh api`.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Request {
    method: &'static str,
    endpoint: String,
    /// Read every page of the answer.
    paginate: bool,
    /// Parameters as `key=value`, sent as strings: in the query of a GET, in the body otherwise.
    fields: Vec<String>,
    /// A JSON body, which `gh` reads from its stdin. A request with a body has no `fields`.
    body: Option<String>,
}

impl Request {
    fn get(endpoint: String) -> Self {
        Self {
            method: "GET",
            endpoint,
            paginate: false,
            fields: Vec::new(),
            body: None,
        }
    }

    /// A GET of a list: all pages, as many entries per page as GitHub gives.
    fn list(endpoint: String) -> Self {
        Self {
            paginate: true,
            fields: vec![PAGE_SIZE.to_owned()],
            ..Self::get(endpoint)
        }
    }

    fn post(endpoint: String, fields: Vec<String>) -> Self {
        Self {
            method: "POST",
            fields,
            ..Self::get(endpoint)
        }
    }

    fn patch(endpoint: String, fields: Vec<String>) -> Self {
        Self {
            method: "PATCH",
            fields,
            ..Self::get(endpoint)
        }
    }

    fn delete(endpoint: String) -> Self {
        Self {
            method: "DELETE",
            ..Self::get(endpoint)
        }
    }

    /// A call that sends `body` as JSON. `-f` would send every value as a string, and GitHub
    /// wants booleans and lists for settings and rules.
    fn json(method: &'static str, endpoint: String, body: &Value) -> Self {
        Self {
            method,
            body: Some(body.to_string()),
            ..Self::get(endpoint)
        }
    }

    /// The arguments after `gh`. Fields go through `-f`, which sends the value as it is: `-F`
    /// would read a value that starts with `@` as a file name.
    fn args(&self) -> Vec<String> {
        let mut args = vec![
            "api".to_owned(),
            "--method".to_owned(),
            self.method.to_owned(),
        ];
        if self.paginate {
            args.push("--paginate".to_owned());
        }
        for field in &self.fields {
            args.push("-f".to_owned());
            args.push(field.clone());
        }
        if self.body.is_some() {
            // `gh` does not tell GitHub what kind of body stdin is.
            for arg in ["--input", "-", "-H", "Content-Type: application/json"] {
                args.push(arg.to_owned());
            }
        }
        args.push(self.endpoint.clone());
        args
    }

    /// `METHOD endpoint`, which is how errors name the call.
    fn describe(&self) -> String {
        format!("{} {}", self.method, self.endpoint)
    }
}

impl GhCli {
    fn issue(&self, pr: u64, part: &str) -> String {
        format!("repos/{}/issues/{pr}/{part}", self.repo)
    }

    /// The client for the repository `flag` names, else the one in `GITHUB_REPOSITORY`, else the
    /// one the remote `origin` of the checkout at `root` points to.
    ///
    /// # Errors
    ///
    /// [`Error::Parse`] when the flag or the variable is not `owner/name`, and [`Error::Missing`]
    /// when all three say nothing.
    pub fn for_checkout(root: &Path, flag: Option<&str>) -> Result<Self> {
        let var = std::env::var("GITHUB_REPOSITORY").ok();
        let repo = select_repo(flag, var.as_deref(), || {
            Git::new(root).remote_url("origin").ok()
        })?;
        Ok(Self { repo })
    }
}

/// The repository to talk to: `flag`, else `var` (the value of `GITHUB_REPOSITORY`, unless it is
/// empty), else what `origin` makes of the URL of the remote.
fn select_repo(
    flag: Option<&str>,
    var: Option<&str>,
    origin: impl FnOnce() -> Option<String>,
) -> Result<String> {
    let given = |what: &str, repo: &str| {
        if is_repo(repo) {
            Ok(repo.to_owned())
        } else {
            Err(Error::Parse {
                what: what.to_owned(),
                message: "expected `owner/name`".to_owned(),
            })
        }
    };
    if let Some(repo) = flag {
        return given("--repo", repo);
    }
    if let Some(repo) = var.filter(|repo| !repo.is_empty()) {
        return given("GITHUB_REPOSITORY", repo);
    }
    origin()
        .and_then(|url| repo_of_remote(&url))
        .ok_or_else(|| Error::Missing {
            what: "repository: pass `--repo owner/name`, set GITHUB_REPOSITORY, or point the \
                   remote `origin` at a github.com repository"
                .to_owned(),
        })
}

/// True for `owner/name`, each part made of letters, digits and `-`, `_`, `.`, and neither part
/// being `.` or `..`. The text becomes part of an endpoint, so nothing else gets through.
fn is_repo(text: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    text.split_once('/')
        .is_some_and(|(owner, name)| part(owner) && part(name))
}

/// The `owner/name` of the github.com repository a remote URL points to, in the forms git
/// accepts: `https://github.com/owner/name.git`, `ssh://git@github.com/owner/name`,
/// `git@github.com:owner/name.git`. `None` for another host, or when the path is not
/// `owner/name`. Whatever comes before the host (a user, even a token) is dropped.
fn repo_of_remote(url: &str) -> Option<String> {
    let url = url.trim();
    let (authority, path) = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?,
        // `user@host:owner/name`, the form scp uses.
        None => url.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let path = path.trim_end_matches('/');
    let repo = path.strip_suffix(".git").unwrap_or(path);
    is_repo(repo).then(|| repo.to_owned())
}

impl PullRequests for GhCli {
    fn labels(&self, pr: u64) -> Result<Vec<String>> {
        let request = Request::list(self.issue(pr, "labels"));
        label_names(&request.describe(), &call(&request)?)
    }

    fn head_sha(&self, pr: u64) -> Result<String> {
        let request = Request::get(format!("repos/{}/pulls/{pr}", self.repo));
        head_sha_from(&request.describe(), &call(&request)?)
    }

    fn add_label(&self, pr: u64, name: &str) -> Result<()> {
        let request = Request::post(self.issue(pr, "labels"), vec![format!("labels[]={name}")]);
        call(&request).map(drop)
    }

    fn remove_label(&self, pr: u64, name: &str) -> Result<()> {
        let request = Request::delete(self.issue(pr, &format!("labels/{}", encode(name))));
        call(&request).map(drop)
    }

    fn ensure_label(&self, spec: &LabelSpec) -> Result<()> {
        let list = Request::list(format!("repos/{}/labels", self.repo));
        let existing = label_names(&list.describe(), &call(&list)?)?;
        match create_label(&self.repo, &existing, spec) {
            Some(create) => call(&create).map(drop),
            None => Ok(()),
        }
    }

    fn comments(&self, pr: u64) -> Result<Vec<Comment>> {
        let request = Request::list(self.issue(pr, "comments"));
        comments_from(&request.describe(), &call(&request)?)
    }

    fn comment(&self, pr: u64, body: &str) -> Result<()> {
        let request = Request::post(self.issue(pr, "comments"), vec![format!("body={body}")]);
        call(&request).map(drop)
    }
}

impl RepoAdmin for GhCli {
    fn rulesets(&self) -> Result<Vec<(u64, String)>> {
        let request = rulesets_request(&self.repo);
        ruleset_names(&request.describe(), &call(&request)?)
    }

    fn ruleset(&self, id: u64) -> Result<Value> {
        let request = Request::get(format!("repos/{}/rulesets/{id}", self.repo));
        object(&request.describe(), &call(&request)?)
    }

    fn create_ruleset(&self, body: &Value) -> Result<()> {
        let request = Request::json("POST", format!("repos/{}/rulesets", self.repo), body);
        call(&request).map(drop)
    }

    fn update_ruleset(&self, id: u64, body: &Value) -> Result<()> {
        let request = Request::json("PUT", format!("repos/{}/rulesets/{id}", self.repo), body);
        call(&request).map(drop)
    }

    fn repo(&self) -> Result<Value> {
        let request = Request::get(format!("repos/{}", self.repo));
        object(&request.describe(), &call(&request)?)
    }

    fn patch_repo(&self, body: &Value) -> Result<()> {
        let request = Request::json("PATCH", format!("repos/{}", self.repo), body);
        call(&request).map(drop)
    }

    fn repo_labels(&self) -> Result<Vec<LabelSpec>> {
        let request = Request::list(format!("repos/{}/labels", self.repo));
        label_specs(&request.describe(), &call(&request)?)
    }

    fn upsert_label(&self, spec: &LabelSpec) -> Result<()> {
        let list = Request::list(format!("repos/{}/labels", self.repo));
        let existing = label_names(&list.describe(), &call(&list)?)?;
        call(&upsert_label_request(&self.repo, &existing, spec)).map(drop)
    }
}

/// The request for the rulesets of the repository `repo`. Only its own: the ones of an
/// organisation are not ours to set.
fn rulesets_request(repo: &str) -> Request {
    Request {
        fields: vec![PAGE_SIZE.to_owned(), "includes_parents=false".to_owned()],
        ..Request::list(format!("repos/{repo}/rulesets"))
    }
}

/// The fields that describe the label `spec`; `name_key` is `name` for a new label and
/// `new_name` for the change of one.
fn label_fields(name_key: &str, spec: &LabelSpec) -> Vec<String> {
    vec![
        format!("{name_key}={}", spec.name),
        format!("color={}", spec.color),
        format!("description={}", spec.description),
    ]
}

/// The request that creates the label `spec`, or `None` when the repository has it already
/// (`existing` are the names it has). GitHub keeps label names unique whatever their case.
fn create_label(repo: &str, existing: &[String], spec: &LabelSpec) -> Option<Request> {
    if existing
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&spec.name))
    {
        return None;
    }
    Some(Request::post(
        format!("repos/{repo}/labels"),
        label_fields("name", spec),
    ))
}

/// The request that makes the repository have the label `spec`: a change of the label it has
/// under that name in any case (`existing` are the names it has), which also sets the spelling of
/// the name, or else a new label.
fn upsert_label_request(repo: &str, existing: &[String], spec: &LabelSpec) -> Request {
    match existing
        .iter()
        .find(|name| name.eq_ignore_ascii_case(&spec.name))
    {
        Some(current) => Request::patch(
            format!("repos/{repo}/labels/{}", encode(current)),
            label_fields("new_name", spec),
        ),
        None => Request::post(format!("repos/{repo}/labels"), label_fields("name", spec)),
    }
}

/// Runs `gh` with the request and returns what it wrote to stdout.
fn call(request: &Request) -> Result<String> {
    run_program(
        OsStr::new("gh"),
        &request.args(),
        &request.describe(),
        request.body.as_deref(),
    )
}

/// Runs `program` with `args` and returns its stdout. `stdin` is what it reads, if anything.
/// `request` names the call in errors.
fn run_program(
    program: &OsStr,
    args: &[String],
    request: &str,
    stdin: Option<&str>,
) -> Result<String> {
    tracing::debug!(request, "gh");
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GH_PROMPT_DISABLED", "1")
        .spawn()
        .map_err(|source| failed(request, &format!("cannot start `gh` ({})", source.kind())))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A body is a few kilobytes, which the pipe takes whole. If `gh` stopped reading, its
        // exit status says why; dropping the pipe ends the input.
        let _ = pipe.write_all(text.as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|source| failed(request, &format!("cannot run `gh` ({})", source.kind())))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let what = match (http_status(&stderr), output.status.code()) {
        (Some(status), _) => format!("HTTP {status}"),
        (None, Some(code)) => format!("`gh` exited with status {code}"),
        (None, None) => "`gh` was ended by a signal".to_owned(),
    };
    Err(failed(request, &what))
}

/// The HTTP status in the stderr of a failed `gh api`, which ends in `(HTTP 404)` or the like.
/// Only a number from 100 to 599 comes out; nothing else of the text does.
fn http_status(stderr: &str) -> Option<u16> {
    let (_, rest) = stderr.split_once("(HTTP ")?;
    let digits: String = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .take(3)
        .collect();
    digits
        .parse()
        .ok()
        .filter(|status| (100..=599).contains(status))
}

/// `segment` as one part of a URL path: everything but letters, digits and `-._~` is `%XX`.
fn encode(segment: &str) -> String {
    use std::fmt::Write as _;
    segment.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            // Writing to a `String` cannot fail.
            let _ = write!(encoded, "%{byte:02X}");
        }
        encoded
    })
}

#[derive(Deserialize)]
struct LabelItem {
    name: String,
    // The listing gives them all; the names are all that most calls read.
    #[serde(default)]
    color: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RulesetItem {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct CommentItem {
    user: Option<UserItem>,
    body: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct UserItem {
    login: String,
}

/// The names in the labels `json` lists.
fn label_names(request: &str, json: &str) -> Result<Vec<String>> {
    let labels: Vec<LabelItem> = pages(request, json)?;
    Ok(labels.into_iter().map(|label| label.name).collect())
}

/// The labels `json` lists. A label without a description has none: the empty string.
fn label_specs(request: &str, json: &str) -> Result<Vec<LabelSpec>> {
    let labels: Vec<LabelItem> = pages(request, json)?;
    Ok(labels
        .into_iter()
        .map(|label| LabelSpec {
            name: label.name,
            color: label.color,
            description: label.description.unwrap_or_default(),
        })
        .collect())
}

/// The id and the name of every ruleset `json` lists.
fn ruleset_names(request: &str, json: &str) -> Result<Vec<(u64, String)>> {
    let rulesets: Vec<RulesetItem> = pages(request, json)?;
    Ok(rulesets
        .into_iter()
        .map(|ruleset| (ruleset.id, ruleset.name))
        .collect())
}

/// `json` as the object it is, for the answer to a request for one thing.
fn object(request: &str, json: &str) -> Result<Value> {
    match serde_json::from_str(json) {
        Ok(value @ Value::Object(_)) => Ok(value),
        // The parser's message can quote what it read, so it is not passed on.
        _ => Err(failed(request, "the answer is not an object")),
    }
}

/// The id of the head commit in the pull request `json` describes: `head.sha`.
fn head_sha_from(request: &str, json: &str) -> Result<String> {
    let pull = object(request, json)?;
    match pull["head"]["sha"].as_str() {
        Some(sha) if !sha.is_empty() => Ok(sha.to_owned()),
        _ => Err(failed(request, "the answer has no head commit")),
    }
}

/// The comments `json` lists. A comment of a deleted account has no author, which no check
/// trusts: it gets the empty string.
fn comments_from(request: &str, json: &str) -> Result<Vec<Comment>> {
    let comments: Vec<CommentItem> = pages(request, json)?;
    Ok(comments
        .into_iter()
        .map(|comment| Comment {
            author: comment.user.map(|user| user.login).unwrap_or_default(),
            body: comment.body.unwrap_or_default(),
            created_at: comment.created_at,
            updated_at: comment.updated_at,
        })
        .collect())
}

/// The entries of every array in `json`. `gh api --paginate` joins the pages of a list into one
/// array, and older versions wrote one array per page, one after the other; reading arrays until
/// the text ends takes both.
fn pages<T: DeserializeOwned>(request: &str, json: &str) -> Result<Vec<T>> {
    let mut entries = Vec::new();
    for page in serde_json::Deserializer::from_str(json).into_iter::<Vec<T>>() {
        // The parser's message can quote what it read, so it is not passed on.
        let page =
            page.map_err(|_| failed(request, "the answer is not a list of the expected shape"))?;
        entries.extend(page);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    const REPO: &str = "owner/repo";

    fn gh() -> GhCli {
        GhCli {
            repo: REPO.to_owned(),
        }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn spec() -> LabelSpec {
        LabelSpec {
            name: "needs-jann".to_owned(),
            color: "d93f0b".to_owned(),
            description: "Waits for Jann".to_owned(),
        }
    }

    #[test]
    fn a_list_reads_every_page_with_the_largest_page_size() {
        let request = Request::list(gh().issue(7, "labels"));
        assert_eq!(request.describe(), "GET repos/owner/repo/issues/7/labels");
        assert_eq!(
            request.args(),
            strings(&[
                "api",
                "--method",
                "GET",
                "--paginate",
                "-f",
                "per_page=100",
                "repos/owner/repo/issues/7/labels"
            ])
        );
    }

    #[test]
    fn a_post_sends_its_fields_as_strings_and_a_delete_has_none() {
        let post = Request::post(
            gh().issue(7, "comments"),
            vec!["body=@file <!-- x -->".to_owned()],
        );
        assert_eq!(
            post.args(),
            strings(&[
                "api",
                "--method",
                "POST",
                "-f",
                "body=@file <!-- x -->",
                "repos/owner/repo/issues/7/comments"
            ])
        );
        let delete = Request::delete(gh().issue(7, "labels/approved"));
        assert_eq!(
            delete.args(),
            strings(&[
                "api",
                "--method",
                "DELETE",
                "repos/owner/repo/issues/7/labels/approved"
            ])
        );
        assert_eq!(
            delete.describe(),
            "DELETE repos/owner/repo/issues/7/labels/approved"
        );
    }

    #[test]
    fn a_label_is_created_unless_the_repository_has_it_in_any_case() {
        let existing = strings(&["bug", "Needs-Jann"]);
        assert_eq!(create_label(REPO, &existing, &spec()), None);
        let request = create_label(REPO, &strings(&["bug"]), &spec());
        assert_eq!(
            request.map(|request| request.args()),
            Some(strings(&[
                "api",
                "--method",
                "POST",
                "-f",
                "name=needs-jann",
                "-f",
                "color=d93f0b",
                "-f",
                "description=Waits for Jann",
                "repos/owner/repo/labels"
            ]))
        );
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        assert_eq!(encode("approved"), "approved");
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode("needs jann/é?"), "needs%20jann%2F%C3%A9%3F");
    }

    #[test]
    fn labels_are_read_from_one_page_or_many() -> crate::Result<()> {
        let request = "GET labels";
        assert_eq!(
            label_names(request, r#"[{"name":"a","id":1},{"name":"b"}]"#)?,
            ["a", "b"]
        );
        // Older `gh` versions write the pages one after the other.
        assert_eq!(
            label_names(request, "[{\"name\":\"a\"}]\n[{\"name\":\"b\"}][]")?,
            ["a", "b"]
        );
        assert_eq!(label_names(request, "[]")?, Vec::<String>::new());
        assert_eq!(label_names(request, "")?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn the_head_is_read_from_the_pull_request() {
        let json = r#"{"number": 7, "head": {"sha": "abc123", "ref": "x"}}"#;
        assert_eq!(head_sha_from("GET p", json).ok().as_deref(), Some("abc123"));
        for bad in [
            "{}",
            r#"{"head": {}}"#,
            r#"{"head": {"sha": ""}}"#,
            "[]",
            "x",
        ] {
            assert!(
                matches!(head_sha_from("GET p", bad), Err(Error::GitHub { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn comments_carry_the_login_the_text_and_both_times() -> crate::Result<()> {
        let json = r#"[
            {"id":1,"user":{"login":"github-actions[bot]","id":41898282},"body":"marker",
             "created_at":"2026-10-31T10:00:00Z","updated_at":"2026-10-31T10:00:00Z"},
            {"id":2,"user":null,"body":null,
             "created_at":"2026-10-31T11:00:00Z","updated_at":"2026-10-31T12:00:00Z"}
        ][{"id":3,"user":{"login":"jann"},"body":"later",
           "created_at":"2026-11-01T10:00:00Z","updated_at":"2026-11-01T10:00:00Z"}]"#;
        let comments = comments_from("GET comments", json)?;
        assert_eq!(
            comments,
            [
                Comment {
                    author: "github-actions[bot]".to_owned(),
                    body: "marker".to_owned(),
                    created_at: "2026-10-31T10:00:00Z".to_owned(),
                    updated_at: "2026-10-31T10:00:00Z".to_owned(),
                },
                Comment {
                    author: String::new(),
                    body: String::new(),
                    created_at: "2026-10-31T11:00:00Z".to_owned(),
                    updated_at: "2026-10-31T12:00:00Z".to_owned(),
                },
                Comment {
                    author: "jann".to_owned(),
                    body: "later".to_owned(),
                    created_at: "2026-11-01T10:00:00Z".to_owned(),
                    updated_at: "2026-11-01T10:00:00Z".to_owned(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn an_answer_of_the_wrong_shape_is_an_error_that_quotes_nothing() {
        for json in [
            r#"{"message":"SECRET text from a comment"}"#,
            r#"[{"name":"SECRET text"}"#,
            r#"[{"login":"SECRET text"}]"#,
            r#"[{"name":7}]"#,
        ] {
            let result = label_names("GET labels", json);
            let Err(Error::GitHub { message }) = &result else {
                panic!("expected a GitHub error for {json}, got {result:?}");
            };
            assert!(message.starts_with("GET labels: "), "{message}");
            assert!(!message.contains("SECRET"), "{message}");
        }
    }

    #[test]
    fn the_status_is_the_number_after_http_and_nothing_else() {
        assert_eq!(http_status("gh: Not Found (HTTP 404)\n"), Some(404));
        assert_eq!(http_status("gh: Validation Failed (HTTP 422)"), Some(422));
        assert_eq!(http_status("gh: no status here"), None);
        assert_eq!(http_status("(HTTP )"), None);
        assert_eq!(http_status("(HTTP 99)"), None);
        assert_eq!(http_status("(HTTP 600)"), None);
        // Longer numbers are cut at three digits: 4040 reads as 404.
        assert_eq!(http_status("(HTTP 4040)"), Some(404));
    }

    #[test]
    fn what_gh_prints_comes_back_when_it_succeeds() -> crate::Result<()> {
        let args = strings(&["--version"]);
        let output = run_program(OsStr::new("git"), &args, "GET x", None)?;
        assert!(output.starts_with("git version"), "{output}");
        Ok(())
    }

    #[test]
    fn a_body_reaches_the_program_through_stdin() -> crate::Result<()> {
        // `git hash-object --stdin` writes the id of what it reads: that of the text `abc`.
        let args = strings(&["hash-object", "--stdin"]);
        let output = run_program(OsStr::new("git"), &args, "PUT x", Some("abc"))?;
        assert_eq!(output.trim(), "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f");
        Ok(())
    }

    #[test]
    fn a_failed_call_names_the_request_and_the_exit_code_only() {
        // `git check-ref-format` fails with exit code 1 and says nothing a test could rely on.
        let args = strings(&["check-ref-format", "refs/heads/.."]);
        let result = run_program(
            OsStr::new("git"),
            &args,
            "GET repos/o/r/issues/7/labels",
            None,
        );
        let Err(Error::GitHub { message }) = &result else {
            panic!("expected a GitHub error, got {result:?}");
        };
        assert_eq!(
            message,
            "GET repos/o/r/issues/7/labels: `gh` exited with status 1"
        );
    }

    #[test]
    fn a_json_body_is_sent_from_stdin_with_its_content_type() {
        let body = serde_json::json!({"allow_auto_merge": true, "rules": [{"type": "deletion"}]});
        let request = Request::json("PUT", "repos/owner/repo/rulesets/9".to_owned(), &body);
        assert_eq!(request.describe(), "PUT repos/owner/repo/rulesets/9");
        assert_eq!(
            request.body.as_deref(),
            Some(r#"{"allow_auto_merge":true,"rules":[{"type":"deletion"}]}"#)
        );
        assert_eq!(
            request.args(),
            strings(&[
                "api",
                "--method",
                "PUT",
                "--input",
                "-",
                "-H",
                "Content-Type: application/json",
                "repos/owner/repo/rulesets/9"
            ])
        );
    }

    #[test]
    fn the_rulesets_asked_for_are_those_of_the_repository_alone() {
        assert_eq!(
            rulesets_request(REPO).args(),
            strings(&[
                "api",
                "--method",
                "GET",
                "--paginate",
                "-f",
                "per_page=100",
                "-f",
                "includes_parents=false",
                "repos/owner/repo/rulesets"
            ])
        );
    }

    #[test]
    fn a_label_is_changed_under_the_name_it_has_in_any_case_and_else_created() {
        let create = upsert_label_request(REPO, &strings(&["bug"]), &spec());
        assert_eq!(create.describe(), "POST repos/owner/repo/labels");
        assert_eq!(
            create.args(),
            create_label(REPO, &strings(&["bug"]), &spec())
                .map(|request| request.args())
                .unwrap_or_default()
        );
        let change = upsert_label_request(REPO, &strings(&["bug", "Needs-Jann"]), &spec());
        assert_eq!(
            change.args(),
            strings(&[
                "api",
                "--method",
                "PATCH",
                "-f",
                "new_name=needs-jann",
                "-f",
                "color=d93f0b",
                "-f",
                "description=Waits for Jann",
                "repos/owner/repo/labels/Needs-Jann"
            ])
        );
    }

    #[test]
    fn rulesets_and_labels_are_read_with_what_the_checks_compare() -> crate::Result<()> {
        let rulesets =
            r#"[{"id":7,"name":"main","target":"branch","source":"o/r"},{"id":9,"name":"tags"}]"#;
        assert_eq!(
            ruleset_names("GET rulesets", rulesets)?,
            [(7, "main".to_owned()), (9, "tags".to_owned())]
        );
        let labels = r#"[
            {"id":1,"name":"bug","color":"d73a4a","description":"Broken","default":true},
            {"id":2,"name":"plain","color":"ffffff","description":null}
        ]"#;
        assert_eq!(
            label_specs("GET labels", labels)?,
            [
                LabelSpec {
                    name: "bug".to_owned(),
                    color: "d73a4a".to_owned(),
                    description: "Broken".to_owned(),
                },
                LabelSpec {
                    name: "plain".to_owned(),
                    color: "ffffff".to_owned(),
                    description: String::new(),
                },
            ]
        );
        assert_eq!(
            object("GET repos/o/r", r#"{"allow_auto_merge":true}"#)?,
            serde_json::json!({"allow_auto_merge": true})
        );
        Ok(())
    }

    #[test]
    fn an_answer_that_is_not_what_was_asked_for_quotes_nothing() {
        for json in [r#"["SECRET"]"#, r#"{"SECRET":"#, "SECRET", ""] {
            let result = object("GET repos/o/r", json);
            let Err(Error::GitHub { message }) = &result else {
                panic!("expected a GitHub error for {json}, got {result:?}");
            };
            assert_eq!(message, "GET repos/o/r: the answer is not an object");
        }
        for json in [r#"[{"id":"SECRET","name":"x"}]"#, r#"[{"name":"SECRET"}]"#] {
            let result = ruleset_names("GET rulesets", json);
            let Err(Error::GitHub { message }) = &result else {
                panic!("expected a GitHub error for {json}, got {result:?}");
            };
            assert!(!message.contains("SECRET"), "{message}");
        }
    }

    #[test]
    fn remotes_of_github_in_every_form_name_the_repository() {
        for url in [
            "https://github.com/FPGSchiba/theos-editor.git",
            "https://github.com/FPGSchiba/theos-editor",
            "https://github.com/FPGSchiba/theos-editor/",
            "https://user:secret@github.com/FPGSchiba/theos-editor.git\n",
            "http://GitHub.com/FPGSchiba/theos-editor.git",
            "ssh://git@github.com/FPGSchiba/theos-editor.git",
            "ssh://git@github.com:22/FPGSchiba/theos-editor.git",
            "git://github.com/FPGSchiba/theos-editor.git",
            "git@github.com:FPGSchiba/theos-editor.git",
            "github.com:FPGSchiba/theos-editor",
        ] {
            assert_eq!(
                repo_of_remote(url).as_deref(),
                Some("FPGSchiba/theos-editor"),
                "{url}"
            );
        }
    }

    #[test]
    fn remotes_of_another_host_or_shape_name_no_repository() {
        for url in [
            "https://gitlab.com/o/r.git",
            "https://github.com.evil.example/o/r.git",
            "https://example.com/github.com/o/r",
            "git@bitbucket.org:o/r.git",
            "https://github.com/owner",
            "https://github.com/o/r/extra",
            "https://github.com/o/..",
            "../local/path",
            r"C:\work\repo",
            "",
        ] {
            assert_eq!(repo_of_remote(url), None, "{url}");
        }
    }

    #[test]
    fn the_repository_is_the_flag_then_the_variable_then_origin() -> crate::Result<()> {
        let origin = || Some("git@github.com:from/origin.git".to_owned());
        assert_eq!(select_repo(Some("a/b"), Some("c/d"), origin)?, "a/b");
        assert_eq!(select_repo(None, Some("c/d"), origin)?, "c/d");
        assert_eq!(select_repo(None, None, origin)?, "from/origin");
        // The runner sets the variable to the empty string in some contexts.
        assert_eq!(select_repo(None, Some(""), origin)?, "from/origin");
        // Origin is not asked when the flag or the variable answers.
        assert_eq!(select_repo(Some("a/b"), None, || None)?, "a/b");
        Ok(())
    }

    #[test]
    fn a_repository_that_is_not_owner_slash_name_is_an_error() {
        let parse = |result: crate::Result<String>| match result {
            Err(Error::Parse { what, message }) => (what, message),
            other => panic!("expected a parse error, got {other:?}"),
        };
        let flag = parse(select_repo(Some("../x/y"), None, || None));
        assert_eq!(
            flag,
            ("--repo".to_owned(), "expected `owner/name`".to_owned())
        );
        let var = parse(select_repo(None, Some("justone"), || None));
        assert_eq!(var.0, "GITHUB_REPOSITORY");
        assert!(matches!(
            select_repo(None, None, || Some("https://example.com/o/r".to_owned())),
            Err(Error::Missing { .. })
        ));
        assert!(matches!(
            select_repo(None, None, || None),
            Err(Error::Missing { .. })
        ));
    }

    #[test]
    fn a_missing_gh_is_a_github_error() {
        let result = run_program(
            OsStr::new("xtask-no-such-program"),
            &strings(&["api"]),
            "POST repos/o/r/issues/7/comments",
            Some("{}"),
        );
        let Err(Error::GitHub { message }) = &result else {
            panic!("expected a GitHub error, got {result:?}");
        };
        assert!(
            message.starts_with("POST repos/o/r/issues/7/comments: cannot start `gh` ("),
            "{message}"
        );
    }
}
