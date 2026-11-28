//! Helpers: throwaway git repositories in a `tempfile` directory.
#![cfg(test)]

use std::path::Path;
use std::process::Command;

use loom_run::config::{KitRef, RunConfig, RunSpec};

/// Runs `git` in `dir` with a fixed identity and returns trimmed stdout. Panics on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A repository at `dir` with one commit on `main` that holds `files`.
pub fn make_repo(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).expect("create repo dir");
    git(dir, &["init", "-q", "-b", "main"]);
    commit_files(dir, files, "initial");
}

/// Writes `files` and commits them with `message`.
pub fn commit_files(dir: &Path, files: &[(&str, &str)], message: &str) {
    for (name, body) in files {
        std::fs::write(dir.join(name), body).expect("write file");
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// A config whose data dir is `data_dir`.
pub fn config(data_dir: &Path) -> RunConfig {
    RunConfig::from_toml(&format!(
        "data_dir = '{}'\nimage = 'img'\nproxy_image = 'proxy'\n",
        data_dir.display()
    ))
    .expect("config parses")
}

/// A run spec for the repository at `repo`.
pub fn spec(repo: &Path, story_branch: &str, kit: KitRef) -> RunSpec {
    RunSpec {
        story_key: "ED-12".to_owned(),
        repo_url: repo.display().to_string(),
        story_branch: story_branch.to_owned(),
        default_branch: "main".to_owned(),
        kit,
        prompt: "go".to_owned(),
        model: None,
        extra_args: Vec::new(),
        caps: loom_run::caps::Caps::default(),
    }
}
