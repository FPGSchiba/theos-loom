//! The per-run clone and the pinned kit (TU-03.1).

mod support;

use std::path::Path;

use loom_core::run::RunId;
use loom_run::config::KitRef;
use loom_run::kit::Kit;
use loom_run::workspace::Workspace;
use support::{commit_files, config, git, make_repo, spec};

fn kit_ref(repo: &Path, version: &str) -> KitRef {
    KitRef {
        repository: repo.display().to_string(),
        version: version.to_owned(),
    }
}

// AC: TU-03.1
#[tokio::test(flavor = "multi_thread")]
async fn two_runs_of_one_story_get_two_clones() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    git(&repo, &["branch", "story/ED-12"]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/ED-12", kit_ref(&repo, "v1"));

    let (one, two) = tokio::join!(
        Workspace::prepare(&config, RunId::new(), &spec),
        Workspace::prepare(&config, RunId::new(), &spec)
    );
    let (one, two) = (one.expect("first clone"), two.expect("second clone"));

    assert_ne!(one.path(), two.path());
    for ws in [&one, &two] {
        assert_eq!(git(ws.path(), &["branch", "--show-current"]), "story/ED-12");
    }
    commit_files(one.path(), &[("only-one.txt", "x")], "in one");
    assert!(!two.path().join("only-one.txt").exists());
    assert_ne!(
        git(one.path(), &["rev-parse", "HEAD"]),
        git(two.path(), &["rev-parse", "HEAD"])
    );
}

#[tokio::test]
async fn a_missing_story_branch_starts_from_the_default_branch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/new", kit_ref(&repo, "v1"));

    let ws = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect("prepare");

    assert_eq!(git(ws.path(), &["branch", "--show-current"]), "story/new");
    assert_eq!(
        git(ws.path(), &["rev-parse", "HEAD"]),
        git(&repo, &["rev-parse", "main"])
    );
    assert_eq!(
        git(ws.path(), &["remote", "get-url", "origin"]),
        spec.repo_url
    );
}

#[tokio::test]
async fn an_existing_story_branch_is_checked_out_with_its_commits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    git(&repo, &["switch", "-q", "-c", "story/ED-12"]);
    commit_files(&repo, &[("story.txt", "s")], "on story");
    git(&repo, &["switch", "-q", "main"]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/ED-12", kit_ref(&repo, "v1"));

    let ws = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect("prepare");

    assert!(ws.path().join("story.txt").exists());
}

#[tokio::test]
async fn a_later_run_sees_what_was_pushed_since() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/new", kit_ref(&repo, "v1"));
    let first = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect("first");
    drop(first);

    commit_files(&repo, &[("b.txt", "b")], "later");
    let second = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect("second");

    assert!(second.path().join("b.txt").exists());
}

#[tokio::test]
async fn remove_deletes_the_run_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/new", kit_ref(&repo, "v1"));
    let run = RunId::new();
    let ws = Workspace::prepare(&config, run, &spec)
        .await
        .expect("prepare");
    let work = ws.path().to_owned();
    let run_dir = tmp.path().join("data").join("runs").join(run.to_string());
    assert!(run_dir.is_dir());

    ws.remove().await.expect("remove");

    assert!(!work.exists());
    assert!(!run_dir.exists());
}

#[tokio::test]
async fn a_failing_git_command_names_the_command_and_stderr() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config(&tmp.path().join("data"));
    let spec = spec(
        &tmp.path().join("no-such-repo"),
        "story/x",
        kit_ref(tmp.path(), "v1"),
    );

    let err = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect_err("fails");

    let text = err.to_string();
    assert!(text.contains("git clone --mirror"), "{text}");
    assert!(text.contains("fatal"), "{text}");
}

fn make_kit_repo(dir: &Path) {
    make_repo(dir, &[("VERSION", "one")]);
    git(dir, &["tag", "v1"]);
    commit_files(dir, &[("VERSION", "two")], "two");
    git(dir, &["tag", "v2"]);
}

// AC: TU-03.1
#[tokio::test]
async fn the_kit_is_checked_out_at_its_pinned_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kit_src = tmp.path().join("kit");
    make_kit_repo(&kit_src);
    let config = config(&tmp.path().join("data"));

    let v1 = Kit::prepare(&config, &kit_ref(&kit_src, "v1"))
        .await
        .expect("v1");
    let v2 = Kit::prepare(&config, &kit_ref(&kit_src, "v2"))
        .await
        .expect("v2");

    assert_eq!(
        std::fs::read_to_string(v1.path().join("VERSION")).expect("read"),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(v2.path().join("VERSION")).expect("read"),
        "two"
    );
}

#[tokio::test]
async fn a_kit_version_is_cloned_once() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kit_src = tmp.path().join("kit");
    make_kit_repo(&kit_src);
    let config = config(&tmp.path().join("data"));
    let reference = kit_ref(&kit_src, "v1");

    let first = Kit::prepare(&config, &reference).await.expect("first");
    // With the source gone, only an existing checkout can satisfy the next calls.
    std::fs::remove_dir_all(&kit_src).expect("remove source");
    let (a, b) = tokio::join!(
        Kit::prepare(&config, &reference),
        Kit::prepare(&config, &reference)
    );

    assert_eq!(a.expect("a").path(), first.path());
    assert_eq!(b.expect("b").path(), first.path());
}

#[tokio::test]
async fn the_kit_files_are_read_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let kit_src = tmp.path().join("kit");
    make_kit_repo(&kit_src);
    let config = config(&tmp.path().join("data"));

    let kit = Kit::prepare(&config, &kit_ref(&kit_src, "v1"))
        .await
        .expect("prepare");

    let file = kit.path().join("VERSION");
    assert!(
        std::fs::metadata(&file)
            .expect("meta")
            .permissions()
            .readonly()
    );
    assert!(std::fs::write(&file, "changed").is_err());
}

#[tokio::test]
async fn a_kit_version_that_is_a_path_or_an_option_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config(&tmp.path().join("data"));
    for version in ["../x", "a/b", "a\\b", "-x", "", ".", "..", "x.partial"] {
        let reference = KitRef {
            repository: "r".to_owned(),
            version: version.to_owned(),
        };
        assert!(
            Kit::prepare(&config, &reference).await.is_err(),
            "{version:?}"
        );
    }
}

#[tokio::test]
async fn a_stale_partial_mirror_does_not_break_the_first_prepare() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    let config = config(&tmp.path().join("data"));
    let spec = spec(&repo, "story/new", kit_ref(&repo, "v1"));
    let hash = loom_hash(&spec.repo_url);
    let partial = tmp
        .path()
        .join("data")
        .join("mirrors")
        .join(format!("{hash}.git.partial"));
    std::fs::create_dir_all(&partial).expect("partial dir");
    std::fs::write(partial.join("garbage"), "not a repository").expect("garbage");

    let ws = Workspace::prepare(&config, RunId::new(), &spec)
        .await
        .expect("prepare");

    assert!(ws.path().join("a.txt").exists());
    assert!(!partial.exists());
}

#[tokio::test]
async fn a_story_branch_or_url_that_looks_like_an_option_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    make_repo(&repo, &[("a.txt", "a")]);
    let config = config(&tmp.path().join("data"));

    let bad_branch = spec(&repo, "-x", kit_ref(&repo, "v1"));
    assert!(
        Workspace::prepare(&config, RunId::new(), &bad_branch)
            .await
            .is_err()
    );
    let mut bad_url = spec(&repo, "story/x", kit_ref(&repo, "v1"));
    bad_url.repo_url = "--upload-pack=x".to_owned();
    assert!(
        Workspace::prepare(&config, RunId::new(), &bad_url)
            .await
            .is_err()
    );
}

/// The mirror's directory name: the first 16 hex digits of the SHA-256 of the URL.
fn loom_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in Sha256::digest(text.as_bytes()).iter().take(8) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
