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

// AC: TU-00.1
#[test]
fn workspace_members_are_the_t1_crates() -> TestResult {
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
        "loom-api@crates/loom-api",
        "loom-context@crates/loom-context",
        "loom-core@crates/loom-core",
        "loom-gate@crates/loom-gate",
        "loom-plan@crates/loom-plan",
        "loom-run@crates/loom-run",
        "loom-store@crates/loom-store",
        "loom@crates/loom",
        "xtask@xtask",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    expected.sort();
    assert_eq!(actual, expected);
    Ok(())
}

// AC: TU-00.1
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

// AC: TU-00.1
#[test]
fn every_member_inherits_the_workspace_package() -> TestResult {
    let (root, packages) = load_workspace()?;
    let root_manifest = read_manifest(&root.join("Cargo.toml"))?;
    let workspace_package = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .ok_or("no `[workspace.package]`")?;
    let workspace_version = workspace_package
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or("`[workspace.package]` has no `version`")?;
    assert_eq!(
        workspace_package
            .get("license")
            .and_then(toml::Value::as_str),
        Some("MIT"),
        "`[workspace.package]` must have `license = \"MIT\"`"
    );
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
        for key in [
            "version",
            "edition",
            "rust-version",
            "publish",
            "repository",
            "license",
        ] {
            let declared = manifest
                .get("package")
                .and_then(|table| table.get(key))
                .ok_or_else(|| format!("{} `[package]` has no `{key}`", package.name))?;
            assert_eq!(
                declared, &inherited,
                "{} must declare `{key}.workspace = true`",
                package.name
            );
        }
    }
    Ok(())
}
