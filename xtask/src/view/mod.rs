//! The workspace view: what cargo compiles, and what `syn` reads in it.
//!
//! The checks that read code (`lints`, `ac-trace`) share one view, so they agree on which files
//! count and read every file once:
//!
//! - [`cargo_check`] runs `cargo check --workspace --all-targets` and lists the artifacts of the
//!   members, each with the files rustc says it read ([`dep_info`]): `#[path]` modules,
//!   `include!` files and `include_str!` data included.
//! - [`source`] parses every `.rs` file among them with `syn` and `proc-macro2`.
//! - [`comments`] finds the `//` comments in a parsed file.
//!
//! [`View::build`] makes one view; [`Shared`] builds it on the first request in a run and hands
//! the same one to every check after.

pub mod cargo_check;
pub mod comments;
pub mod dep_info;
pub mod source;

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use comments::{LineComment, line_comments};
pub use source::{Source, has_ident, line_of, parse_source};

use crate::verdict::Finding;
use crate::{Error, Result};

/// How the view's `cargo` commands run.
#[derive(Clone, Copy, Debug)]
pub struct ViewOptions {
    /// Pass `--locked`: the lock file must be up to date and stays as it is.
    pub locked: bool,
    /// Pass `--offline`: cargo does not use the network.
    pub offline: bool,
    /// Build into `<root>/target` (`--target-dir`), whatever `CARGO_TARGET_DIR` or
    /// `CARGO_BUILD_TARGET_DIR` say, so workspaces in different folders never share builds.
    pub own_target_dir: bool,
    /// Variables to set on the `cargo check` child. Production code leaves this empty; tests use
    /// it to force an environment, such as a shared `CARGO_TARGET_DIR`, on one view only.
    pub extra_env: &'static [(&'static str, &'static str)],
}

impl ViewOptions {
    /// What the gate runs in the repository.
    pub const REPOSITORY: Self = Self {
        locked: true,
        offline: false,
        own_target_dir: false,
        extra_env: &[],
    };
    /// What tests run over fixture workspaces, which have no network and may gain a lock file.
    /// Every fixture builds into its own `target` folder.
    pub const FIXTURE: Self = Self {
        locked: false,
        offline: true,
        own_target_dir: true,
        extra_env: &[],
    };
}

/// What kind of target an artifact is built from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TargetKind {
    /// A library of any crate type, a procedural macro included.
    Lib,
    /// A binary.
    Bin,
    /// An integration test.
    Test,
    /// A benchmark.
    Bench,
    /// An example.
    Example,
    /// A build script.
    BuildScript,
}

/// One thing `cargo check` built for a workspace member.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Artifact {
    /// The package name.
    pub package: String,
    /// The folder of the package's manifest, repo-relative with `/` (`""` at the root).
    pub manifest_dir: String,
    /// The kind of target.
    pub kind: TargetKind,
    /// The target's name.
    pub name: String,
    /// The target's root file, repo-relative with `/`.
    pub src_path: String,
    /// Whether this is a test build (`profile.test`): the `--test` build of a library or binary,
    /// and every test and benchmark target.
    pub test: bool,
    /// The files rustc read for it, repo-relative with `/`, sorted, without duplicates.
    pub files: Vec<String>,
}

/// The files of a workspace, read once.
pub struct View {
    /// The workspace root the view was built for.
    pub root: PathBuf,
    /// The artifacts of the members, as cargo built them.
    pub artifacts: Vec<Artifact>,
    /// Files that do not parse as Rust.
    pub findings: Vec<Finding>,
    sources: BTreeMap<String, Source>,
}

impl View {
    /// Checks the workspace at `root` with cargo and reads every Rust file of every artifact.
    ///
    /// # Errors
    ///
    /// Fails when cargo fails ([`cargo_check::artifacts`]) or a file of an artifact cannot be
    /// read. A file that does not parse is a finding of the view, not an error.
    pub fn build(root: &Path, options: ViewOptions) -> Result<Self> {
        // Cargo's messages carry absolute paths in cargo's own spelling of the root, so every path
        // is made repo-relative against that spelling and the files are read from it.
        let (root, artifacts) = cargo_check::artifacts(root, options)?;
        let root = root.as_path();
        let rust_files: BTreeSet<&str> = artifacts
            .iter()
            .flat_map(|artifact| &artifact.files)
            .map(String::as_str)
            .filter(|file| Path::new(file).extension().is_some_and(|ext| ext == "rs"))
            .collect();
        let mut sources = BTreeMap::new();
        let mut findings = Vec::new();
        for path in rust_files {
            let location = root.join(path);
            let bytes = fs::read(&location).map_err(|source| Error::Io {
                path: location,
                source,
            })?;
            let (source, finding) = parse_source(path, &bytes);
            findings.extend(finding);
            sources.insert(path.to_owned(), source);
        }
        Ok(Self {
            root: root.to_path_buf(),
            artifacts,
            findings,
            sources,
        })
    }

    /// The Rust file at the repo-relative `path`, when an artifact compiles it.
    #[must_use]
    pub fn source(&self, path: &str) -> Option<&Source> {
        self.sources.get(path)
    }

    /// Every Rust file of every artifact, once, in path order.
    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.sources.values()
    }

    /// The files the test builds compile: those of the artifacts with `test == true`.
    #[must_use]
    pub fn test_files(&self) -> BTreeSet<&str> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.test)
            .flat_map(|artifact| &artifact.files)
            .map(String::as_str)
            .collect()
    }
}

/// One view per run: built on the first request and the same after.
pub struct Shared {
    options: ViewOptions,
    cell: OnceCell<View>,
    /// Why the build failed, once it has: cargo is not run a second time in the same run.
    failure: OnceCell<String>,
}

impl Shared {
    /// A holder that builds the view with [`ViewOptions::REPOSITORY`].
    #[must_use]
    pub const fn new() -> Self {
        Self::with(ViewOptions::REPOSITORY)
    }

    /// A holder that builds the view with `options`.
    #[must_use]
    pub const fn with(options: ViewOptions) -> Self {
        Self {
            options,
            cell: OnceCell::new(),
            failure: OnceCell::new(),
        }
    }

    /// The view of the workspace at `root`; the first call builds it and later calls return the
    /// same one, whatever `root` they pass.
    ///
    /// # Errors
    ///
    /// Fails like [`View::build`]. A failed build is not tried again: later calls fail with an
    /// error that says so and repeats the first one's text.
    pub fn view(&self, root: &Path) -> Result<&View> {
        if let Some(view) = self.cell.get() {
            return Ok(view);
        }
        if let Some(why) = self.failure.get() {
            return Err(Error::Missing {
                what: format!("the workspace view, whose build failed earlier in this run: {why}"),
            });
        }
        match View::build(root, self.options) {
            Ok(view) => Ok(self.cell.get_or_init(|| view)),
            Err(error) => {
                self.failure.get_or_init(|| error.to_string());
                Err(error)
            }
        }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
