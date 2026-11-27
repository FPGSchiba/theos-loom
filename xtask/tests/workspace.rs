//! Shape of the workspace: its members, their documentation and their shared version.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{MetadataCommand, Package};

type TestResult = Result<(), Box<dyn Error>>;

/// The workspace root and its member packages, as `cargo metadata --no-deps` reports them.
fn load_workspace() -> Result<(PathBuf, Vec<Package>), Box<dyn Error>> {
    let metadata = MetadataCommand::new().no_deps().exec()?;
    let root = metadata.workspace_root.clone().into_std_path_buf();
    let members = metadata.workspace_packages().into_iter().cloned().collect();
    Ok((root, members))
}

/// The directory of a package manifest, relative to the workspace root and joined with `/`.
fn relative_dir(root: &Path, package: &Package) -> Result<String, Box<dyn Error>> {
    let manifest = package.manifest_path.as_std_path();
    let dir = manifest.parent().ok_or("manifest path has no parent")?;
    let relative = dir.strip_prefix(root)?;
    let parts: Vec<&str> = relative
        .components()
        .map(|part| part.as_os_str().to_str().ok_or("non-UTF-8 path component"))
        .collect::<Result<_, _>>()?;
    Ok(parts.join("/"))
}

fn read_manifest(path: &Path) -> Result<toml::Table, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?.parse::<toml::Table>()?)
}

// AC: CE-08.1
#[test]
fn workspace_members_are_the_g4_crates() -> TestResult {
    let (root, packages) = load_workspace()?;
    let mut actual = Vec::new();
    for package in &packages {
        actual.push(format!(
            "{}@{}",
            package.name,
            relative_dir(&root, package)?
        ));
    }
    actual.sort();
    let mut expected: Vec<String> = [
        "theos-content@crates/theos-content",
        "theos-vcs@crates/theos-vcs",
        "theos-studio@crates/theos-studio",
        "theos-map-format@crates/theos-map-format",
        "theos-maps@crates/theos-maps",
        "theos@crates/theos",
        "theos-mcp@crates/theos-mcp",
        "theos-editor@crates/theos-editor",
        "xtask@xtask",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(actual, expected);
    Ok(())
}

// AC: CE-08.1
#[test]
fn every_member_has_readme_and_crate_docs() -> TestResult {
    let (root, packages) = load_workspace()?;
    for package in &packages {
        let dir = root.join(relative_dir(&root, package)?);
        assert!(
            dir.join("README.md").is_file(),
            "{} has no README.md",
            package.name
        );
        let sources: Vec<PathBuf> = ["src/lib.rs", "src/main.rs"]
            .iter()
            .map(|source| dir.join(source))
            .filter(|path| path.is_file())
            .collect();
        assert!(
            !sources.is_empty(),
            "{} has neither src/lib.rs nor src/main.rs",
            package.name
        );
        for source in sources {
            let text = fs::read_to_string(&source)?;
            assert!(
                text.starts_with("//!"),
                "{} does not start with crate docs (`//!`)",
                source.display()
            );
        }
    }
    Ok(())
}

// AC: CE-08.11
#[test]
fn every_member_inherits_the_workspace_version() -> TestResult {
    let (root, packages) = load_workspace()?;
    let root_manifest = read_manifest(&root.join("Cargo.toml"))?;
    let workspace_version = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or("`[workspace.package]` has no `version`")?;
    let inherited = toml::Value::Table(toml::Table::from_iter([(
        "workspace".to_owned(),
        toml::Value::Boolean(true),
    )]));
    for package in &packages {
        assert_eq!(
            package.version.to_string(),
            workspace_version,
            "{} does not have the workspace version",
            package.name
        );
        let manifest = read_manifest(package.manifest_path.as_std_path())?;
        let declared = manifest
            .get("package")
            .and_then(|table| table.get("version"))
            .ok_or("`[package]` has no `version`")?;
        assert_eq!(
            declared, &inherited,
            "{} must declare `version.workspace = true`",
            package.name
        );
    }
    Ok(())
}
