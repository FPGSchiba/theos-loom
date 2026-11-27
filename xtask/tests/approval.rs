//! What `approval` reads from git, over temporary repositories made with the `git` command line:
//! the protected paths and tables a pull request touches, and the fingerprint of its changes.
//! The last test keeps `.loom/protected.toml` and `docs/gate.md` §5 saying the same.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use xtask::checks::approval::fingerprint::{self, Fingerprint};
use xtask::checks::approval::protected::{self, ProtectedPaths};
use xtask::git::Git;
use xtask::markdown;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// A repository on branch `main`, with a helper for each thing the tests do to it.
struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    /// Line endings are left as written (`core.autocrlf` off) and every call carries an
    /// identity, because CI runners have none.
    ///
    /// `core.filemode` is off in the repository, as `git init` sets it on Windows: the
    /// executable bit of a file on disk is then not read, and `git add` keeps the mode that is
    /// in the index. So a mode set with `git update-index --chmod` is the mode that `commit`
    /// stores on Linux, macOS and Windows alike. With `core.filemode` on (what `git init` does
    /// on Linux and macOS) `git add -A` would put the on-disk mode, 0644, back.
    fn new() -> TestResult<Self> {
        let repo = Self {
            dir: tempfile::tempdir()?,
        };
        repo.git(&["init", "-q", "-b", "main"])?;
        repo.git(&["config", "core.filemode", "false"])?;
        Ok(repo)
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> TestResult<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?.trim_end().to_owned())
    }

    fn write(&self, path: &str, text: &str) -> TestResult {
        let path = self.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        Ok(())
    }

    /// Commits everything in the work tree and returns the new commit.
    fn commit(&self, message: &str) -> TestResult<String> {
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-q", "--allow-empty", "-m", message])?;
        self.git(&["rev-parse", "HEAD"])
    }

    /// The mode and the blob id `path` has in the commit `rev`, as the commit stores them.
    fn entry(&self, rev: &str, path: &str) -> TestResult<(String, String)> {
        let line = self.git(&["ls-tree", rev, "--", path])?;
        // `<mode> blob <id>	<path>`
        let mut words = line.split_whitespace();
        let mode = words.next().ok_or("ls-tree printed nothing")?;
        let id = words.nth(1).ok_or("ls-tree has no blob id")?;
        Ok((mode.to_owned(), id.to_owned()))
    }

    fn switch_new(&self, branch: &str) -> TestResult {
        self.git(&["switch", "-q", "-c", branch]).map(drop)
    }

    fn switch(&self, branch: &str) -> TestResult {
        self.git(&["switch", "-q", branch]).map(drop)
    }

    fn repository(&self) -> Git {
        Git::new(self.path())
    }

    /// The protected things `head` touches since `base`, sorted.
    fn touched(&self, base: &str, head: &str) -> TestResult<Vec<String>> {
        let git = self.repository();
        let paths = git.diff_name_only_no_renames(base, head)?;
        let mut touched = protected::touched_in(&git, &rules()?, &paths, base, head)?;
        touched.sort();
        Ok(touched)
    }

    /// The fingerprint of the changes of `head` since the merge base of `onto` and `head`.
    fn fingerprint(&self, onto: &str, head: &str) -> TestResult<Fingerprint> {
        let git = self.repository();
        let merge_base = git.merge_base(onto, head)?;
        Ok(fingerprint::compute(&git, &merge_base, head)?)
    }
}

/// The root of this repository, whose `.loom/protected.toml` is the real rule list.
fn repo_root() -> TestResult<&'static Path> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?)
}

fn rules() -> TestResult<ProtectedPaths> {
    Ok(ProtectedPaths::load(repo_root()?)?)
}

const LINTS: &str = "\
[workspace]
members = [\"crates/*\"]

[workspace.package]
version = \"0.0.0\"

[workspace.lints.rust]
unsafe_code = \"forbid\"

[workspace.lints.clippy]
all = { level = \"warn\", priority = -1 }
unwrap_used = \"deny\"
";

/// `main` with a protected file, an unprotected one and the root manifest.
fn base_repo() -> TestResult<(Repo, String)> {
    let repo = Repo::new()?;
    repo.write(
        "docs/a.md",
        "Some text that is long enough to be seen as the same file.\n",
    )?;
    repo.write("notes/keep.md", "kept\n")?;
    repo.write("src/lib.rs", "// one\n")?;
    repo.write("Cargo.toml", LINTS)?;
    let base = repo.commit("feat: base")?;
    repo.switch_new("topic")?;
    Ok((repo, base))
}

// AC: TU-00.4
#[test]
fn rename_out_of_docs_is_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.git(&["mv", "docs/a.md", "notes/a.md"])?;
    let head = repo.commit("docs: move a out of docs")?;
    // Git reports this as a rename; the old path is what makes it protected.
    assert!(
        repo.git(&["diff", "--name-status", &base, &head])?
            .starts_with('R'),
        "the fixture must be a rename"
    );
    assert_eq!(repo.touched(&base, &head)?, ["docs/a.md"]);
    Ok(())
}

#[test]
fn rename_into_docs_and_other_changes_of_docs_are_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.git(&["mv", "notes/keep.md", "docs/keep.md"])?;
    let head = repo.commit("docs: move keep into docs")?;
    assert_eq!(repo.touched(&base, &head)?, ["docs/keep.md"]);

    // A deletion counts too.
    repo.git(&["rm", "-q", "docs/a.md"])?;
    let head = repo.commit("docs: delete a")?;
    assert_eq!(repo.touched(&base, &head)?, ["docs/a.md", "docs/keep.md"]);
    Ok(())
}

#[test]
fn a_mode_change_of_a_protected_file_is_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.git(&["update-index", "--chmod=+x", "docs/a.md"])?;
    let head = repo.commit("docs: make a executable")?;
    // The commit really holds a mode change and nothing else, whatever the platform does with
    // the executable bit of files on disk.
    let (before, blob_before) = repo.entry(&base, "docs/a.md")?;
    let (after, blob_after) = repo.entry(&head, "docs/a.md")?;
    assert_eq!((before.as_str(), after.as_str()), ("100644", "100755"));
    assert_eq!(blob_before, blob_after);
    assert_eq!(repo.touched(&base, &head)?, ["docs/a.md"]);
    Ok(())
}

#[test]
fn changes_outside_the_protected_paths_are_not_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.git(&["mv", "notes/keep.md", "notes/kept.md"])?;
    repo.write("src/lib.rs", "// two\n")?;
    repo.write("docs/plans/P00-x.md", "a plan\n")?;
    repo.write("docs/trial/P00-y.md", "the status\n")?;
    repo.write("crates/a/src/lib.rs", "// new\n")?;
    let head = repo.commit("feat: change code")?;
    assert_eq!(repo.touched(&base, &head)?, Vec::<String>::new());
    assert_eq!(repo.touched(&head, &head)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn paths_are_matched_in_every_case() -> TestResult {
    let (repo, base) = base_repo()?;
    // The names go straight into the index: on a file system that ignores case, `DOCS/x.md`
    // would be written into the folder `docs` and be recorded as `docs/x.md`.
    let blob = repo.git(&["rev-parse", "HEAD:src/lib.rs"])?;
    for path in ["DOCS/x.md", "Claude.md", "DOCS/plans/P00-x.md"] {
        let entry = format!("100644,{blob},{path}");
        repo.git(&[
            "-c",
            "core.ignorecase=false",
            "update-index",
            "--add",
            "--cacheinfo",
            &entry,
        ])?;
    }
    repo.git(&["commit", "-q", "-m", "docs: case variants"])?;
    let head = repo.git(&["rev-parse", "HEAD"])?;
    assert_eq!(
        repo.touched(&base, &head)?,
        ["Claude.md", "DOCS/plans/P00-x.md", "DOCS/x.md"]
    );
    Ok(())
}

// AC: TU-00.4
#[test]
fn lints_table_change_is_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.write(
        "Cargo.toml",
        &LINTS.replace("unwrap_used = \"deny\"", "unwrap_used = \"allow\""),
    )?;
    let head = repo.commit("chore: weaken a lint")?;
    assert_eq!(
        repo.touched(&base, &head)?,
        ["Cargo.toml [workspace.lints]"]
    );

    // A lint added, one removed, the whole table removed, and a table that comes new.
    for (name, text) in [
        ("added", format!("{LINTS}todo = \"deny\"\n")),
        ("removed", LINTS.replace("unwrap_used = \"deny\"\n", "")),
        (
            "table removed",
            LINTS
                .split("[workspace.lints.rust]")
                .next()
                .unwrap_or("")
                .to_owned(),
        ),
        (
            "level changed",
            LINTS.replace("level = \"warn\"", "level = \"allow\""),
        ),
        (
            "priority changed",
            LINTS.replace("priority = -1", "priority = 0"),
        ),
    ] {
        repo.write("Cargo.toml", &text)?;
        let head = repo.commit(&format!("chore: {name}"))?;
        assert_eq!(
            repo.touched(&base, &head)?,
            ["Cargo.toml [workspace.lints]"],
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_manifest_that_comes_goes_or_breaks_is_protected() -> TestResult {
    // No manifest on the base: one with lints is new.
    let repo = Repo::new()?;
    repo.write("README.md", "readme\n")?;
    let base = repo.commit("feat: base")?;
    repo.switch_new("topic")?;
    repo.write("Cargo.toml", LINTS)?;
    let with_lints = repo.commit("feat: manifest")?;
    assert_eq!(
        repo.touched(&base, &with_lints)?,
        ["Cargo.toml [workspace.lints]"]
    );

    // A manifest with no lints table is no change from none at all.
    repo.write("Cargo.toml", "[workspace]\nmembers = []\n")?;
    let without = repo.commit("feat: manifest without lints")?;
    assert_eq!(repo.touched(&base, &without)?, Vec::<String>::new());

    // Deleted, and not TOML any more: the table is gone or unreadable.
    repo.git(&["rm", "-q", "Cargo.toml"])?;
    let deleted = repo.commit("chore: delete the manifest")?;
    assert_eq!(
        repo.touched(&with_lints, &deleted)?,
        ["Cargo.toml [workspace.lints]"]
    );
    repo.write("Cargo.toml", "this is [not toml")?;
    let broken = repo.commit("chore: break the manifest")?;
    assert_eq!(
        repo.touched(&with_lints, &broken)?,
        ["Cargo.toml [workspace.lints]"]
    );
    assert_eq!(
        repo.touched(&broken, &with_lints)?,
        ["Cargo.toml [workspace.lints]"]
    );
    // Two different broken manifests are a change too: nothing can be said about the table.
    repo.write("Cargo.toml", "this is [also not toml")?;
    let broken_too = repo.commit("chore: break the manifest differently")?;
    assert_eq!(
        repo.touched(&broken, &broken_too)?,
        ["Cargo.toml [workspace.lints]"]
    );
    assert_eq!(
        repo.touched(&broken_too, &broken_too)?,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn a_manifest_change_outside_the_lints_table_is_not_protected() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.write(
        "Cargo.toml",
        &LINTS
            .replace("version = \"0.0.0\"", "version = \"0.1.0\"")
            .replace(
                "members = [\"crates/*\"]",
                "members = [\"crates/*\", \"xtask\"]",
            ),
    )?;
    let head = repo.commit("chore: bump the version")?;
    assert_eq!(repo.touched(&base, &head)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn reformatted_lints_table_is_not_a_change() -> TestResult {
    let (repo, base) = base_repo()?;
    // The same lints: reordered, the inline table spelled out, comments and spacing added, and
    // the table moved below the others.
    repo.write(
        "Cargo.toml",
        "\
# The workspace.
[workspace.package]
version   =   \"0.0.0\"

[workspace]
members = [ \"crates/*\" ]

[workspace.lints.clippy]
unwrap_used = \"deny\"   # still denied

[workspace.lints.clippy.all]
priority = -1
level = \"warn\"

[workspace.lints.rust]
unsafe_code = 'forbid'
",
    )?;
    let head = repo.commit("style: reformat the manifest")?;
    assert!(
        !repo.git(&["diff", "--name-only", &base, &head])?.is_empty(),
        "the manifest must have changed as text"
    );
    assert_eq!(repo.touched(&base, &head)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn fingerprint_changes_on_whitespace_only_edit() -> TestResult {
    let (repo, _) = base_repo()?;
    repo.write("docs/a.md", "text\n")?;
    let first = repo.commit("docs: change a")?;
    repo.write("docs/a.md", "text \n")?;
    let trailing_space = repo.commit("docs: add a trailing space")?;
    repo.write("docs/a.md", "text\r\n")?;
    let crlf = repo.commit("docs: use CRLF")?;

    let fingerprints = [
        repo.fingerprint("main", &first)?,
        repo.fingerprint("main", &trailing_space)?,
        repo.fingerprint("main", &crlf)?,
    ];
    assert_ne!(fingerprints[0], fingerprints[1]);
    assert_ne!(fingerprints[0], fingerprints[2]);
    assert_ne!(fingerprints[1], fingerprints[2]);
    Ok(())
}

#[test]
fn fingerprint_changes_on_mode_change() -> TestResult {
    let (repo, _) = base_repo()?;
    repo.write("docs/a.md", "text\n")?;
    let first = repo.commit("docs: change a")?;
    repo.git(&["update-index", "--chmod=+x", "docs/a.md"])?;
    let executable = repo.commit("docs: make a executable")?;
    // The commit really holds a mode change and nothing else, whatever the platform does with
    // the executable bit of files on disk.
    let (before, blob_before) = repo.entry(&first, "docs/a.md")?;
    let (after, blob_after) = repo.entry(&executable, "docs/a.md")?;
    assert_eq!((before.as_str(), after.as_str()), ("100644", "100755"));
    assert_eq!(blob_before, blob_after);
    assert_eq!(
        repo.git(&["diff", "--name-only", &first, &executable])?,
        "docs/a.md"
    );
    assert_ne!(
        repo.fingerprint("main", &first)?,
        repo.fingerprint("main", &executable)?
    );
    Ok(())
}

#[test]
fn fingerprint_changes_when_a_binary_changes() -> TestResult {
    let (repo, _) = base_repo()?;
    fs::write(repo.path().join("docs.bin"), [0_u8, 1, 2, 3])?;
    let first = repo.commit("feat: binary")?;
    fs::write(repo.path().join("docs.bin"), [0_u8, 1, 2, 4])?;
    let second = repo.commit("feat: binary again")?;
    assert_ne!(
        repo.fingerprint("main", &first)?,
        repo.fingerprint("main", &second)?
    );
    Ok(())
}

#[test]
fn fingerprint_is_the_sha256_of_the_raw_diff() -> TestResult {
    let (repo, base) = base_repo()?;
    repo.write("docs/a.md", "text\n")?;
    let head = repo.commit("docs: change a")?;
    let fingerprint = fingerprint::compute(&repo.repository(), &base, &head)?;
    assert_eq!(fingerprint.0.len(), 64);
    assert!(
        Fingerprint::parse(&fingerprint.0).is_some(),
        "{fingerprint:?}"
    );
    // The same changes at the same base give the same fingerprint, whenever it is asked.
    assert_eq!(
        fingerprint,
        fingerprint::compute(&repo.repository(), &base, &head)?
    );
    // No changes is a fingerprint too: the hash of nothing.
    let none = fingerprint::compute(&repo.repository(), &head, &head)?;
    assert_eq!(
        none.0,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    Ok(())
}

#[test]
fn fingerprint_stable_across_rebase_onto_unrelated_main_commit() -> TestResult {
    let (repo, _) = base_repo()?;
    repo.write("docs/a.md", "changed by the pull request\n")?;
    repo.write("src/new.rs", "// new\n")?;
    let before = repo.commit("docs: change a")?;
    let first = repo.fingerprint("main", &before)?;

    // `main` moves on in a file the pull request does not change; the branch is rebased.
    repo.switch("main")?;
    repo.write("src/lib.rs", "// main moved\n")?;
    repo.commit("feat: main moves on")?;
    repo.switch("topic")?;
    repo.git(&["rebase", "-q", "main"])?;
    let after = repo.git(&["rev-parse", "HEAD"])?;
    assert_ne!(before, after, "the rebase must have made new commits");

    assert_eq!(repo.fingerprint("main", &after)?, first);
    Ok(())
}

#[test]
fn fingerprint_stable_across_a_merge_from_main() -> TestResult {
    let (repo, _) = base_repo()?;
    repo.write("docs/a.md", "changed by the pull request\n")?;
    let before = repo.commit("docs: change a")?;
    let first = repo.fingerprint("main", &before)?;

    repo.switch("main")?;
    repo.write("src/lib.rs", "// main moved\n")?;
    repo.commit("feat: main moves on")?;
    repo.switch("topic")?;
    repo.git(&["merge", "-q", "--no-ff", "-m", "chore: merge main", "main"])?;
    let merged = repo.git(&["rev-parse", "HEAD"])?;
    assert_ne!(before, merged);

    assert_eq!(repo.fingerprint("main", &merged)?, first);
    // And a further edit after the merge moves it.
    repo.write("docs/a.md", "changed again\n")?;
    let edited = repo.commit("docs: change a again")?;
    assert_ne!(repo.fingerprint("main", &edited)?, first);
    Ok(())
}

#[test]
fn fingerprint_changes_when_main_changes_a_file_the_pr_changes() -> TestResult {
    let (repo, _) = base_repo()?;
    repo.write("docs/a.md", "changed by the pull request\n")?;
    let before = repo.commit("docs: change a")?;
    let first = repo.fingerprint("main", &before)?;

    repo.switch("main")?;
    repo.write("docs/a.md", "changed on main\n")?;
    repo.commit("docs: main changes a")?;
    repo.switch("topic")?;
    repo.git(&[
        "merge",
        "-q",
        "-s",
        "ours",
        "-m",
        "chore: merge main",
        "main",
    ])?;
    let merged = repo.git(&["rev-parse", "HEAD"])?;

    // The old side of the file is another blob now, so the fingerprint is another one.
    assert_ne!(repo.fingerprint("main", &merged)?, first);
    Ok(())
}

/// The text of the level-two section of `text` whose heading starts with `prefix`.
fn section<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    markdown::sections(text, 2)
        .into_iter()
        .find(|section| section.heading.text.starts_with(prefix))
        .map(|section| section.body)
}

/// Every inline code span in `text`.
fn backticked(text: &str) -> BTreeSet<String> {
    markdown::code_spans(text).into_iter().collect()
}

fn strings(table: &toml::Table, key: &str) -> TestResult<Vec<String>> {
    let Some(list) = table.get(key) else {
        return Ok(Vec::new());
    };
    let list = list.as_array().ok_or("not a list")?;
    list.iter()
        .map(|item| Ok(item.as_str().ok_or("not a string")?.to_owned()))
        .collect()
}

// AC: TU-00.4
#[test]
fn protected_paths_toml_matches_gate_md_section_5() -> TestResult {
    let root = repo_root()?;
    let gate_md = fs::read_to_string(root.join("docs").join("gate.md"))?;
    let section_5 = section(&gate_md, "5.").ok_or("docs/gate.md has no §5")?;
    let in_gate_md = backticked(section_5);
    assert!(
        in_gate_md.contains("CLAUDE.md"),
        "the section was not found: {section_5}"
    );

    let config: toml::Table =
        fs::read_to_string(root.join(".loom").join("protected.toml"))?.parse()?;
    let protected = strings(&config, "protected")?;
    let except = strings(&config, "except")?;
    assert!(
        protected.len() > 1,
        "the protected list is empty or was not read"
    );
    for pattern in protected.iter().chain(&except) {
        assert!(
            in_gate_md.contains(pattern),
            "`{pattern}` is in .loom/protected.toml but not backticked in docs/gate.md §5"
        );
    }

    // The tables are named as `[workspace.lints]` of the root `Cargo.toml`.
    let rules = rules()?;
    assert_ne!(rules.tables(), [], "no table is protected");
    for rule in rules.tables() {
        let table = format!("[{}]", rule.table);
        assert!(
            in_gate_md.contains(table.as_str()),
            "{table} is not in docs/gate.md §5"
        );
        assert!(
            in_gate_md.contains(rule.file.as_str()),
            "{} is not in docs/gate.md §5",
            rule.file
        );
    }
    Ok(())
}
