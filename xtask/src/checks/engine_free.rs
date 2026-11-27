//! The `engine-free` check: no crate of the workspace can pull in a Theos-Engine crate unless the
//! cargo feature `engine` is on (umbrella G1, `docs/gate.md` §1).
//!
//! The check [`check`] asks cargo twice and fails when either level finds an engine crate:
//!
//! - **Declared** ([`declared_findings`], from `cargo metadata --no-deps`): every dependency of
//!   every workspace member, of any kind and for any target, also one renamed through `package`,
//!   that is an engine crate must be `optional`, and only the feature `engine` may enable it:
//!   `dep:x`, `x`, `x/f` and `x?/f`, where `x` is the rename or else the name. That includes the
//!   feature that cargo makes of an optional dependency when no feature has a `dep:` for it. The
//!   features that `default` reaches must not include `engine`. A workspace member that a crate
//!   depends on with `features = ["engine"]` is an engine dependency of that crate, and a feature
//!   that switches on `x/engine` for such a member `x` is a reference to it.
//! - **Resolved** ([`resolved_findings`], from `cargo metadata` with default features and for all
//!   platforms): no package of the resolve is an engine crate. This catches an engine crate that
//!   a dependency brings in, or that a `[patch]` puts in.
//!
//! An engine crate is a package from a source that matches `gate/engine-crates.toml`, a path
//! package outside the workspace root, or a package whose name is in that file and that is not a
//! member of the workspace. A member is a workspace member whose folder is inside the root. So
//! our member `theos-editor` is ours while the engine's crate of that name is not, and a member
//! that lives outside the root is an engine crate like any other path package there.
//!
//! Findings name the crate, the dependency and the feature, and never quote a manifest: the
//! repository is the pull request's, and what a check prints is read by the runner. For the same
//! reason, what cargo says when it fails is not shown.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use cargo_metadata::{Dependency, DependencyKind, Metadata, Package, PackageId};
use serde::Deserialize;

use crate::ci::CiEvent;
use crate::process::cargo_metadata;
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The file that says what an engine crate is, relative to the repository root.
const ENGINE_FILE: &str = "gate/engine-crates.toml";

/// The one feature that may enable an engine crate.
const ENGINE_FEATURE: &str = "engine";

/// What an engine crate is, besides coming from outside the workspace
/// (`gate/engine-crates.toml`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineCrates {
    /// Parts of source URLs of the engine, such as `github.com/fpgschiba/theos-engine`. A
    /// package whose source contains one of them, without regard for case, is an engine crate.
    pub sources: Vec<String>,
    /// Names of the engine's crates. A package with one of these names is an engine crate
    /// unless it is a member of this workspace.
    pub names: Vec<String>,
}

impl EngineCrates {
    /// Reads `gate/engine-crates.toml` of the repository at `root`.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read, is not the expected TOML (two lists of strings,
    /// `sources` and `names`, and nothing else), or has an empty list or an empty entry. The
    /// message gives the line and nothing of the text.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(ENGINE_FILE);
        let text = fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self> {
        let engine: Self = toml::from_str(text).map_err(|error| {
            let line = error.span().map(|span| {
                let before = text.get(..span.start).unwrap_or(text);
                before.matches('\n').count() + 1
            });
            config_error(match line {
                Some(line) => format!(
                    "not the expected TOML, near line {line}: it has the lists `sources` and \
                     `names` of strings, and nothing else"
                ),
                None => "not the expected TOML: it has the lists `sources` and `names` of \
                         strings, and nothing else"
                    .to_owned(),
            })
        })?;
        for (list, entries) in [("sources", &engine.sources), ("names", &engine.names)] {
            if entries.is_empty() {
                return Err(config_error(format!(
                    "`{list}` is empty, so the check would know less than it must"
                )));
            }
            if entries.iter().any(|entry| entry.trim().is_empty()) {
                return Err(config_error(format!(
                    "`{list}` has an empty entry, which would match every crate"
                )));
            }
        }
        Ok(engine)
    }

    /// Whether `source` (the source of a package or dependency, such as
    /// `git+https://github.com/…`) contains one of the sources, without regard for case.
    #[must_use]
    pub fn matches_source(&self, source: &str) -> bool {
        let source = source.to_lowercase();
        self.sources
            .iter()
            .any(|known| source.contains(&known.to_lowercase()))
    }

    /// Whether `name` is the name of an engine crate, without regard for case and with `_` as
    /// `-`: cargo and crates.io take these names for one.
    #[must_use]
    pub fn lists_name(&self, name: &str) -> bool {
        let name = normal_name(name);
        self.names.iter().any(|known| normal_name(known) == name)
    }
}

fn config_error(message: String) -> Error {
    Error::Config {
        file: ENGINE_FILE.to_owned(),
        message,
    }
}

fn normal_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

/// Runs the check on the repository at `root` with `gate/engine-crates.toml`, `--locked` and
/// online, as CI does.
///
/// # Errors
///
/// Fails when the file cannot be read or cargo cannot give the metadata of the workspace.
pub fn run(root: &Path, _ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    let engine = EngineCrates::load(root)?;
    check(root, &engine, true, false)
}

/// Checks the workspace at `root`: the declared level on `cargo metadata --no-deps`, then the
/// resolved one on `cargo metadata` with default features and all platforms. `locked` and
/// `offline` are passed on to cargo as `--locked` and `--offline`.
///
/// When the declared level has found something and cargo cannot resolve the workspace (an
/// engine crate that is not there to be read, as in CI), the verdict has the findings and a
/// note that the second level did not run.
///
/// # Errors
///
/// Fails when cargo cannot be started, fails, or writes something else than the metadata.
pub fn check(root: &Path, engine: &EngineCrates, locked: bool, offline: bool) -> Result<Verdict> {
    let declared = cargo_metadata(root, true, locked, offline)?;
    let mut findings = declared_findings(&declared, engine);
    let mut notes = Vec::new();
    match cargo_metadata(root, false, locked, offline) {
        Ok(resolved) => findings.extend(resolved_findings(&resolved, engine)),
        Err(error) if !findings.is_empty() => notes.push(format!(
            "the resolved level did not run, because the dependency graph could not be read: \
             {error}"
        )),
        Err(error) => return Err(error),
    }
    Ok(Verdict {
        check: Check::EngineFree,
        findings,
        notes,
    })
}

/// Why a package or dependency is an engine crate, for the message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Why {
    /// Its source matches `gate/engine-crates.toml`.
    Source,
    /// It is a path package outside the workspace root.
    OutsideRoot,
    /// Its name is in `gate/engine-crates.toml` and it is not a member.
    ListedName,
    /// A member of the workspace, with its feature `engine` switched on.
    EnginePart,
}

impl Why {
    const fn text(self) -> &'static str {
        match self {
            Self::Source => "from a source in `gate/engine-crates.toml`",
            Self::OutsideRoot => "a path outside the workspace",
            Self::ListedName => {
                "named in `gate/engine-crates.toml` and not a member of the workspace"
            }
            Self::EnginePart => "a member of the workspace with its feature `engine` switched on",
        }
    }
}

/// `path` with `.` and `..` resolved, without asking the file system. Paths in metadata are
/// absolute, and a `..` at the root stays at the root.
fn normalised(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

/// The workspace of some metadata: its root and the folders of the members that are ours.
struct Workspace {
    root: PathBuf,
    ours: BTreeSet<PathBuf>,
}

impl Workspace {
    fn new(meta: &Metadata) -> Self {
        let root = normalised(meta.workspace_root.as_std_path());
        let ours = meta
            .workspace_packages()
            .into_iter()
            .map(package_dir)
            .filter(|dir| dir.starts_with(&root))
            .collect();
        Self { root, ours }
    }

    /// The manifest of `package`, relative to the root and joined with `/`; `None` outside it.
    fn manifest_file(&self, package: &Package) -> Option<String> {
        let manifest = normalised(package.manifest_path.as_std_path());
        let relative = manifest.strip_prefix(&self.root).ok()?;
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|part| part.as_os_str().to_str())
            .collect();
        Some(parts?.join("/"))
    }

    /// Why `package` is an engine crate, if it is.
    fn why_package(&self, package: &Package, engine: &EngineCrates) -> Option<Why> {
        let dir = package_dir(package);
        match &package.source {
            Some(source) if engine.matches_source(&source.repr) => Some(Why::Source),
            None if !dir.starts_with(&self.root) => Some(Why::OutsideRoot),
            _ => (engine.lists_name(package.name.as_str()) && !self.ours.contains(&dir))
                .then_some(Why::ListedName),
        }
    }

    /// Why `dependency` is an engine crate, if it is.
    fn why_dependency(&self, dependency: &Dependency, engine: &EngineCrates) -> Option<Why> {
        if dependency
            .source
            .as_ref()
            .is_some_and(|source| engine.matches_source(&source.repr))
        {
            return Some(Why::Source);
        }
        let dir = dependency
            .path
            .as_ref()
            .map(|path| normalised(path.as_std_path()));
        if dir.as_ref().is_some_and(|dir| !dir.starts_with(&self.root)) {
            return Some(Why::OutsideRoot);
        }
        if self.is_member(dependency) {
            return dependency
                .features
                .iter()
                .any(|feature| feature == ENGINE_FEATURE)
                .then_some(Why::EnginePart);
        }
        engine
            .lists_name(&dependency.name)
            .then_some(Why::ListedName)
    }

    /// Whether `dependency` is a path dependency on one of our members.
    fn is_member(&self, dependency: &Dependency) -> bool {
        dependency
            .path
            .as_ref()
            .is_some_and(|path| self.ours.contains(&normalised(path.as_std_path())))
    }
}

/// The folder of the manifest of `package`.
fn package_dir(package: &Package) -> PathBuf {
    let manifest = normalised(package.manifest_path.as_std_path());
    manifest
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

/// The dependency that `value`, one entry of a feature, names, as its key (the rename, else the
/// name) and the feature of it when the value is `x/f` or `x?/f`. `None` when the value names a
/// feature of the crate itself. A plain name that is no feature of the crate can only be a
/// dependency.
fn dependency_reference<'a>(
    value: &'a str,
    features: &BTreeMap<String, Vec<String>>,
) -> Option<(&'a str, Option<&'a str>)> {
    if let Some(key) = value.strip_prefix("dep:") {
        return Some((key, None));
    }
    if let Some((key, feature)) = value.split_once('/') {
        return Some((key.strip_suffix('?').unwrap_or(key), Some(feature)));
    }
    (!features.contains_key(value)).then_some((value, None))
}

/// Whether the features that `default` reaches, through features that name features, include
/// `engine`.
fn default_reaches_engine(features: &BTreeMap<String, Vec<String>>) -> bool {
    let mut reached = BTreeSet::new();
    let mut pending = vec!["default"];
    while let Some(feature) = pending.pop() {
        if !reached.insert(feature) {
            continue;
        }
        for value in features.get(feature).into_iter().flatten() {
            if features.contains_key(value) {
                pending.push(value);
            }
        }
    }
    reached.contains(ENGINE_FEATURE)
}

fn kind_name(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Development => "dev-dependency",
        DependencyKind::Build => "build-dependency",
        _ => "dependency",
    }
}

/// A dependency for a message: its package name, and its key when it is renamed.
fn named(name: &str, key: &str) -> String {
    if key == name {
        format!("`{name}`")
    } else {
        format!("`{name}` as `{key}`")
    }
}

fn push_new(messages: &mut Vec<String>, message: String) {
    if !messages.contains(&message) {
        messages.push(message);
    }
}

/// What is wrong with the dependencies and features of `package`, as messages.
fn package_messages(
    workspace: &Workspace,
    package: &Package,
    engine: &EngineCrates,
) -> Vec<String> {
    let krate = package.name.as_str();
    let mut messages = Vec::new();
    // The engine dependencies by key, each with the name of the package and why it is one, and
    // the keys of the members.
    let mut engine_deps: BTreeMap<&str, (&str, Why)> = BTreeMap::new();
    let mut members = BTreeSet::new();
    for dependency in &package.dependencies {
        let key = dependency.rename.as_deref().unwrap_or(&dependency.name);
        if workspace.is_member(dependency) {
            members.insert(key);
        }
        let Some(why) = workspace.why_dependency(dependency, engine) else {
            continue;
        };
        engine_deps
            .entry(key)
            .or_insert((dependency.name.as_str(), why));
        if !dependency.optional {
            push_new(
                &mut messages,
                format!(
                    "`{krate}` has a {} on the engine crate {} ({}) that is not optional: an \
                     engine crate may only be an optional dependency that the feature \
                     `{ENGINE_FEATURE}` alone enables",
                    kind_name(dependency.kind),
                    named(&dependency.name, key),
                    why.text(),
                ),
            );
        }
    }
    for (feature, values) in &package.features {
        if feature == ENGINE_FEATURE {
            continue;
        }
        for value in values {
            let Some((key, of)) = dependency_reference(value, &package.features) else {
                continue;
            };
            if let Some((name, _)) = engine_deps.get(key) {
                push_new(
                    &mut messages,
                    format!(
                        "the feature `{feature}` of `{krate}` enables the engine crate {}: only \
                         the feature `{ENGINE_FEATURE}` may",
                        named(name, key),
                    ),
                );
            } else if members.contains(key) && of == Some(ENGINE_FEATURE) {
                push_new(
                    &mut messages,
                    format!(
                        "the feature `{feature}` of `{krate}` switches on the feature \
                         `{ENGINE_FEATURE}` of `{key}`: only the feature `{ENGINE_FEATURE}` may"
                    ),
                );
            }
        }
    }
    if default_reaches_engine(&package.features) {
        push_new(
            &mut messages,
            format!(
                "the feature `default` of `{krate}` reaches the feature `{ENGINE_FEATURE}`: \
                 engine crates must stay off unless it is asked for"
            ),
        );
    }
    messages
}

/// The declared level for the workspace of `meta`, which `cargo metadata --no-deps` gives: what
/// is wrong with the dependencies and features of the members. A finding names the manifest of
/// the crate.
#[must_use]
pub fn declared_findings(meta: &Metadata, engine: &EngineCrates) -> Vec<Finding> {
    let workspace = Workspace::new(meta);
    meta.workspace_packages()
        .into_iter()
        .flat_map(|package| {
            let file = workspace.manifest_file(package);
            package_messages(&workspace, package, engine)
                .into_iter()
                .map(move |message| Finding {
                    file: file.clone(),
                    line: None,
                    message,
                })
        })
        .collect()
}

/// The resolved level for `meta`, which `cargo metadata` gives for default features and all
/// platforms: one finding for every engine crate among the packages of the resolve, with the
/// crates that need it. Without a resolve, every package counts.
#[must_use]
pub fn resolved_findings(meta: &Metadata, engine: &EngineCrates) -> Vec<Finding> {
    let workspace = Workspace::new(meta);
    let nodes = meta.resolve.as_ref().map(|resolve| &resolve.nodes);
    let used: Option<BTreeSet<&PackageId>> =
        nodes.map(|nodes| nodes.iter().map(|node| &node.id).collect());
    let names: BTreeMap<&PackageId, &str> = meta
        .packages
        .iter()
        .map(|package| (&package.id, package.name.as_str()))
        .collect();
    let mut findings = Vec::new();
    for package in &meta.packages {
        if used
            .as_ref()
            .is_some_and(|used| !used.contains(&package.id))
        {
            continue;
        }
        let Some(why) = workspace.why_package(package, engine) else {
            continue;
        };
        let needed_by: BTreeSet<&str> = nodes
            .into_iter()
            .flatten()
            .filter(|node| node.dependencies.contains(&package.id))
            .filter_map(|node| names.get(&node.id).copied())
            .collect();
        let needed_by = if needed_by.is_empty() {
            String::new()
        } else {
            let list: Vec<String> = needed_by.iter().map(|name| format!("`{name}`")).collect();
            format!(", needed by {}", list.join(", "))
        };
        findings.push(Finding::new(format!(
            "the dependency graph with default features has the engine crate `{}` ({}){needed_by}: \
             an engine crate may only be in it with the feature `{ENGINE_FEATURE}`",
            package.name,
            why.text(),
        )));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    const ROOT: &str = "/ws";
    const GIT: &str = "git+https://github.com/FPGSchiba/Theos-Engine?branch=main#0123456789ab";
    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

    fn engine() -> EngineCrates {
        EngineCrates {
            sources: vec!["github.com/fpgschiba/theos-engine".to_owned()],
            names: vec!["theos-core".to_owned(), "theos-editor".to_owned()],
        }
    }

    /// A dependency as `cargo metadata` writes it.
    fn dep(name: &str) -> Value {
        json!({
            "name": name, "source": null, "req": "*", "kind": null, "rename": null,
            "optional": false, "uses_default_features": true, "features": [], "target": null,
            "registry": null, "path": null
        })
    }

    /// `value` with its field `key` set.
    fn with(mut value: Value, key: &str, to: Value) -> Value {
        value[key] = to;
        value
    }

    /// A path dependency.
    fn path_dep(name: &str, path: &str) -> Value {
        with(dep(name), "path", json!(path))
    }

    /// A package at `dir` (its id too) with these dependencies and features.
    fn package(
        name: &str,
        dir: &str,
        source: Option<&str>,
        deps: &[Value],
        features: &Value,
    ) -> Value {
        json!({
            "name": name, "version": "0.0.0", "id": dir, "source": source,
            "dependencies": deps, "targets": [], "features": features,
            "manifest_path": format!("{dir}/Cargo.toml")
        })
    }

    /// A workspace at `ROOT` with these packages, whose members are the packages in `members`
    /// (by directory) and whose resolve has the nodes `nodes`, each a package with the
    /// directories of what it needs. `None` is `--no-deps`.
    fn metadata(
        packages: &[Value],
        members: &[&str],
        nodes: Option<&[(&str, &[&str])]>,
    ) -> TestResult<Metadata> {
        let resolve = nodes.map(|nodes| {
            let nodes: Vec<Value> = nodes
                .iter()
                .map(|(id, needs)| json!({"id": id, "dependencies": needs, "deps": [], "features": []}))
                .collect();
            json!({"nodes": nodes, "root": null})
        });
        Ok(serde_json::from_value(json!({
            "packages": packages, "workspace_members": members, "resolve": resolve,
            "workspace_root": ROOT, "target_directory": "/ws/target", "version": 1
        }))?)
    }

    /// The declared findings for a workspace of the one crate `a` with `deps` and `features`, and
    /// the member `theos-editor` next to it.
    fn declared(deps: &[Value], features: &Value) -> TestResult<Vec<Finding>> {
        let a = package("a", "/ws/crates/a", None, deps, features);
        let editor = package(
            "theos-editor",
            "/ws/crates/theos-editor",
            None,
            &[],
            &json!({"engine": []}),
        );
        let meta = metadata(
            &[a, editor],
            &["/ws/crates/a", "/ws/crates/theos-editor"],
            None,
        )?;
        Ok(declared_findings(&meta, &engine()))
    }

    fn messages(findings: &[Finding]) -> Vec<&str> {
        findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect()
    }

    #[test]
    fn source_match_is_case_insensitive() {
        let engine = engine();
        assert!(
            engine.matches_source("git+https://GitHub.com/FPGSchiba/Theos-Engine?branch=main#ab")
        );
        assert!(engine.matches_source("git+ssh://git@github.com/fpgschiba/theos-engine.git"));
        assert!(!engine.matches_source(REGISTRY));
        assert!(!engine.matches_source("git+https://github.com/fpgschiba/theos-editor"));
        // The text in the file is compared without case too.
        let upper = EngineCrates {
            sources: vec!["GitHub.com/FPGSchiba/Theos-Engine".to_owned()],
            names: Vec::new(),
        };
        assert!(upper.matches_source("git+https://github.com/fpgschiba/theos-engine"));
    }

    #[test]
    fn names_are_compared_without_case_and_with_underscore_as_hyphen() {
        let engine = engine();
        assert!(engine.lists_name("theos-core"));
        assert!(engine.lists_name("Theos_Core"));
        assert!(!engine.lists_name("theos-content"));
        assert!(!engine.lists_name("theos"));
    }

    #[test]
    fn gate_file_text_is_read_and_checked() -> TestResult {
        let ok = EngineCrates::parse("sources = [\"a/b\"]\nnames = [\"theos-core\"]\n")?;
        assert_eq!(ok.sources, ["a/b"]);
        assert_eq!(ok.names, ["theos-core"]);
        for bad in [
            "names = [\"theos-core\"]\n",
            "sources = [\"a/b\"]\n",
            "sources = []\nnames = [\"theos-core\"]\n",
            "sources = [\"a/b\"]\nnames = []\n",
            // An empty source would match every source.
            "sources = [\"\"]\nnames = [\"theos-core\"]\n",
            "sources = [\" \"]\nnames = [\"theos-core\"]\n",
            "sources = [\"a/b\"]\nnames = [\"\"]\n",
            "sources = [\"a/b\"]\nnames = [\"theos-core\"]\nextra = 1\n",
            "sources = [\"a/b\"\nnames = [\"theos-core\"]\n",
        ] {
            let error = EngineCrates::parse(bad).err().ok_or(bad)?;
            assert!(matches!(error, Error::Config { .. }), "{bad}: {error:?}");
        }
        Ok(())
    }

    #[test]
    fn a_dependency_on_a_crate_from_the_engine_repo_is_an_engine_dependency() -> TestResult {
        // The name is not in the list: the source alone makes it an engine crate.
        let git = with(dep("anything"), "source", json!(GIT));
        let findings = declared(std::slice::from_ref(&git), &json!({}))?;
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("`anything`"), "{findings:?}");
        assert!(findings[0].message.contains("not optional"), "{findings:?}");
        assert_eq!(findings[0].file.as_deref(), Some("crates/a/Cargo.toml"));

        let optional = with(git, "optional", json!(true));
        let features = json!({"engine": ["dep:anything"]});
        assert_eq!(declared(&[optional], &features)?, []);
        Ok(())
    }

    #[test]
    fn a_registry_crate_with_a_listed_name_is_an_engine_crate() -> TestResult {
        for name in ["theos-core", "theos_core", "Theos-Core"] {
            let findings = declared(&[with(dep(name), "source", json!(REGISTRY))], &json!({}))?;
            assert_eq!(findings.len(), 1, "{name}: {findings:?}");
        }
        let serde = with(dep("serde"), "source", json!(REGISTRY));
        assert_eq!(declared(&[serde], &json!({}))?, []);
        Ok(())
    }

    #[test]
    fn a_path_dependency_outside_the_root_is_an_engine_crate_whatever_its_name() -> TestResult {
        let outside = path_dep("anything", "/elsewhere/anything");
        let findings = declared(&[outside], &json!({}))?;
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("outside the workspace"),
            "{findings:?}"
        );

        // `..` that leaves the root is outside, however the path is written.
        let climbing = path_dep("anything", "/ws/crates/../../elsewhere/anything");
        assert_eq!(declared(&[climbing], &json!({}))?.len(), 1);
        // A folder that only starts like the root is not inside it.
        let sibling = path_dep("anything", "/ws-other/anything");
        assert_eq!(declared(&[sibling], &json!({}))?.len(), 1);

        let inside = path_dep("anything", "/ws/crates/anything");
        assert_eq!(declared(&[inside], &json!({}))?, []);
        let inside_climbing = path_dep("anything", "/ws/crates/a/../anything");
        assert_eq!(declared(&[inside_climbing], &json!({}))?, []);
        Ok(())
    }

    #[test]
    fn our_theos_editor_is_ours_and_any_other_is_the_engines() -> TestResult {
        let ours = path_dep("theos-editor", "/ws/crates/theos-editor");
        assert_eq!(declared(&[ours], &json!({}))?, []);

        // From a registry, a git repo elsewhere or a folder of the workspace that is no member.
        let registry = with(dep("theos-editor"), "source", json!(REGISTRY));
        assert_eq!(declared(&[registry], &json!({}))?.len(), 1);
        let git = with(
            dep("theos-editor"),
            "source",
            json!("git+https://example.org/x"),
        );
        assert_eq!(declared(&[git], &json!({}))?.len(), 1);
        let stray = path_dep("theos-editor", "/ws/vendor/theos-editor");
        assert_eq!(declared(&[stray], &json!({}))?.len(), 1);
        Ok(())
    }

    #[test]
    fn every_kind_and_target_of_dependency_counts() -> TestResult {
        let core = path_dep("theos-core", "/elsewhere/theos-core");
        let kinds = [("dev", "dev-dependency"), ("build", "build-dependency")];
        for (kind, label) in kinds {
            let findings = declared(&[with(core.clone(), "kind", json!(kind))], &json!({}))?;
            assert_eq!(findings.len(), 1, "{kind}: {findings:?}");
            assert!(findings[0].message.contains(label), "{findings:?}");
        }
        let target = with(core, "target", json!("cfg(windows)"));
        assert_eq!(declared(&[target], &json!({}))?.len(), 1);
        Ok(())
    }

    #[test]
    fn features_name_a_dependency_by_its_key() -> TestResult {
        // `core` is the key of `theos-core`, which is how a feature writes it.
        let renamed = with(
            with(
                path_dep("theos-core", "/elsewhere/theos-core"),
                "rename",
                json!("core"),
            ),
            "optional",
            json!(true),
        );
        let ok = json!({"engine": ["dep:core"]});
        assert_eq!(declared(std::slice::from_ref(&renamed), &ok)?, []);

        for value in ["dep:core", "core/extra", "core?/extra", "core"] {
            // A plain name that is no feature but the key of a dependency is that dependency.
            let features = json!({"engine": ["dep:core"], "render": [value]});
            let findings = declared(std::slice::from_ref(&renamed), &features)?;
            assert_eq!(findings.len(), 1, "{value}: {findings:?}");
            let message = &findings[0].message;
            assert!(
                message.contains("feature `render`") && message.contains("`core`"),
                "{message}"
            );
        }
        // The implicit feature of an optional dependency is a feature that is not `engine`.
        let implicit = json!({"core": ["dep:core"]});
        let findings = declared(&[renamed], &implicit)?;
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("feature `core`"),
            "{findings:?}"
        );
        Ok(())
    }

    #[test]
    fn a_feature_that_is_no_reference_to_the_dependency_is_left_alone() -> TestResult {
        let optional = with(
            path_dep("theos-core", "/elsewhere/theos-core"),
            "optional",
            json!(true),
        );
        let features = json!({
            "engine": ["dep:theos-core"],
            "render": ["other"],
            "other": ["serde/derive", "dep:serde"],
        });
        assert_eq!(declared(&[optional], &features)?, []);
        Ok(())
    }

    #[test]
    fn default_must_not_reach_engine_through_any_chain() -> TestResult {
        for features in [
            json!({"default": ["engine"], "engine": []}),
            json!({"default": ["a1"], "a1": ["a2"], "a2": ["engine"], "engine": []}),
            // A cycle ends.
            json!({"default": ["x"], "x": ["y"], "y": ["x", "engine"], "engine": []}),
        ] {
            let findings = declared(&[], &features)?;
            assert_eq!(findings.len(), 1, "{features}: {findings:?}");
            let message = &findings[0].message;
            assert!(
                message.contains("feature `default`") && message.contains("feature `engine`"),
                "{message}"
            );
        }
        for features in [
            json!({"default": ["x"], "x": ["y"], "y": ["x"], "engine": []}),
            json!({"engine": [], "default": []}),
            json!({"engine": []}),
        ] {
            assert_eq!(declared(&[], &features)?, [], "{features}");
        }
        Ok(())
    }

    #[test]
    fn a_member_with_its_engine_feature_on_is_an_engine_dependency() -> TestResult {
        let member = |optional: bool| {
            with(
                with(
                    path_dep("theos-editor", "/ws/crates/theos-editor"),
                    "features",
                    json!(["engine"]),
                ),
                "optional",
                json!(optional),
            )
        };
        let findings = declared(&[member(false)], &json!({}))?;
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].message.contains("`theos-editor`"),
            "{findings:?}"
        );
        assert!(findings[0].message.contains("not optional"), "{findings:?}");

        assert_eq!(
            declared(&[member(true)], &json!({"engine": ["dep:theos-editor"]}))?,
            []
        );
        let other = json!({"engine": ["dep:theos-editor"], "render": ["dep:theos-editor"]});
        assert_eq!(declared(&[member(true)], &other)?.len(), 1);
        Ok(())
    }

    #[test]
    fn a_feature_may_switch_on_the_engine_feature_of_a_member_only_as_engine() -> TestResult {
        let ours = || path_dep("theos-editor", "/ws/crates/theos-editor");
        let behind_engine = json!({"engine": ["theos-editor/engine"]});
        assert_eq!(declared(&[ours()], &behind_engine)?, []);
        let weak_behind_engine = json!({"engine": ["theos-editor?/engine"]});
        assert_eq!(declared(&[ours()], &weak_behind_engine)?, []);

        for value in ["theos-editor/engine", "theos-editor?/engine"] {
            let findings = declared(&[ours()], &json!({"engine": [], "render": [value]}))?;
            assert_eq!(findings.len(), 1, "{value}: {findings:?}");
            let message = &findings[0].message;
            assert!(
                message.contains("feature `render`") && message.contains("`theos-editor`"),
                "{message}"
            );
        }
        // Other features of the member are none of the gate's business.
        let other = json!({"engine": [], "render": ["theos-editor/extra"]});
        assert_eq!(declared(&[ours()], &other)?, []);
        Ok(())
    }

    #[test]
    fn identical_findings_are_reported_once() -> TestResult {
        let twice = [
            with(
                path_dep("theos-core", "/elsewhere/theos-core"),
                "kind",
                json!("dev"),
            ),
            with(
                path_dep("theos-core", "/elsewhere/theos-core"),
                "kind",
                json!("dev"),
            ),
        ];
        assert_eq!(declared(&twice, &json!({}))?.len(), 1);
        Ok(())
    }

    #[test]
    fn resolved_finds_engine_packages_and_says_what_needs_them() -> TestResult {
        let a = package("a", "/ws/crates/a", None, &[], &json!({}));
        let via = package("via", "/ws/crates/via", None, &[], &json!({}));
        let core = package("theos-core", "/elsewhere/theos-core", None, &[], &json!({}));
        let git = package("odd", "/git/odd", Some(GIT), &[], &json!({}));
        let serde = package("serde", "/reg/serde", Some(REGISTRY), &[], &json!({}));
        let named = package("theos-editor", "/reg/game", Some(REGISTRY), &[], &json!({}));
        let meta = metadata(
            &[a, via, core, git, serde, named],
            &["/ws/crates/a", "/ws/crates/via"],
            Some(&[
                (
                    "/ws/crates/a",
                    &["/reg/serde", "/ws/crates/via", "/git/odd"],
                ),
                ("/ws/crates/via", &["/elsewhere/theos-core", "/reg/game"]),
                ("/elsewhere/theos-core", &[]),
                ("/git/odd", &[]),
                ("/reg/serde", &[]),
                ("/reg/game", &[]),
            ]),
        )?;
        let findings = resolved_findings(&meta, &engine());
        let messages = messages(&findings);
        assert_eq!(messages.len(), 3, "{messages:?}");
        for (name, needed_by) in [("theos-core", "via"), ("odd", "a"), ("theos-editor", "via")] {
            let message = messages
                .iter()
                .find(|message| message.contains(&format!("`{name}`")))
                .ok_or(name)?;
            assert!(message.contains("dependency graph"), "{message}");
            assert!(
                message.contains(&format!("needed by `{needed_by}`")),
                "{message}"
            );
        }
        assert!(findings.iter().all(|finding| finding.file.is_none()));
        Ok(())
    }

    #[test]
    fn resolved_leaves_out_packages_the_graph_does_not_use() -> TestResult {
        let a = package("a", "/ws/crates/a", None, &[], &json!({}));
        let core = package("theos-core", "/elsewhere/theos-core", None, &[], &json!({}));
        let meta = metadata(
            &[a, core],
            &["/ws/crates/a"],
            Some(&[("/ws/crates/a", &[])]),
        )?;
        assert_eq!(resolved_findings(&meta, &engine()), []);
        Ok(())
    }

    #[test]
    fn resolved_without_a_graph_looks_at_every_package() -> TestResult {
        let core = package("theos-core", "/elsewhere/theos-core", None, &[], &json!({}));
        let meta = metadata(&[core], &[], None)?;
        assert_eq!(resolved_findings(&meta, &engine()).len(), 1);
        Ok(())
    }

    #[test]
    fn resolved_takes_our_members_for_ours_but_not_one_outside_the_root() -> TestResult {
        let editor = package(
            "theos-editor",
            "/ws/crates/theos-editor",
            None,
            &[],
            &json!({}),
        );
        let meta = metadata(
            &[editor],
            &["/ws/crates/theos-editor"],
            Some(&[("/ws/crates/theos-editor", &[])]),
        )?;
        assert_eq!(resolved_findings(&meta, &engine()), []);

        // A member that lives outside the root is an engine crate like any path package there,
        // and a member name does not excuse it.
        let outside = package(
            "theos-editor",
            "/elsewhere/theos-editor",
            None,
            &[],
            &json!({}),
        );
        let meta = metadata(
            &[outside],
            &["/elsewhere/theos-editor"],
            Some(&[("/elsewhere/theos-editor", &[])]),
        )?;
        assert_eq!(resolved_findings(&meta, &engine()).len(), 1);

        // Nor does a member excuse a path package of the same name next to it.
        let editor = package(
            "theos-editor",
            "/ws/crates/theos-editor",
            None,
            &[],
            &json!({}),
        );
        let vendored = package(
            "theos-editor",
            "/ws/vendor/theos-editor",
            None,
            &[],
            &json!({}),
        );
        let meta = metadata(
            &[editor, vendored],
            &["/ws/crates/theos-editor"],
            Some(&[
                ("/ws/crates/theos-editor", &[]),
                ("/ws/vendor/theos-editor", &[]),
            ]),
        )?;
        let findings = resolved_findings(&meta, &engine());
        assert_eq!(findings.len(), 1, "{findings:?}");
        Ok(())
    }
}
