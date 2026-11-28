//! `cargo check` as the source of the workspace's artifacts and their files.
//!
//! Cargo says what it compiled (`compiler-artifact` messages, read with `cargo_metadata`), and
//! rustc says what each artifact read (its dep-info file, read by [`super::dep_info`]).

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cargo_metadata::{Message, PackageId, TargetKind as CargoKind};

use super::dep_info::{build_script_dep_info, dep_info_of, parse_dep_info};
use super::{Artifact, TargetKind, ViewOptions};
use crate::process::{CargoCommand, cargo_metadata, cargo_output_showing_stderr, workspace_root};
use crate::repo_path::repo_relative;
use crate::{Error, Result};

/// The command whose artifacts make the view of the workspace at `root`.
///
/// With [`ViewOptions::own_target_dir`] it builds into `<root>/target`, which beats
/// `CARGO_TARGET_DIR` and `CARGO_BUILD_TARGET_DIR` in the environment.
#[must_use]
pub fn check_command(options: ViewOptions, root: &Path) -> CargoCommand {
    let mut args: Vec<String> = [
        "check",
        "--workspace",
        "--all-targets",
        "--message-format=json",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    if options.locked {
        args.push("--locked".to_owned());
    }
    if options.offline {
        args.push("--offline".to_owned());
    }
    if options.own_target_dir {
        args.push("--target-dir".to_owned());
        args.push(root.join("target").to_string_lossy().into_owned());
    }
    CargoCommand {
        args,
        env: options
            .extra_env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        env_remove: Vec::new(),
    }
}

/// What the error says when the check fails; it does not quote the diagnostics, because they can
/// quote the pull request's code.
const CHECK_FAILED: &str = "the check failed; run `cargo check --workspace --all-targets \
                            --locked` in the repository to read the diagnostics";

/// The artifacts of the members of the workspace at `root`, as `cargo check --workspace
/// --all-targets` builds them, each with the files its dep-info lists.
///
/// # Errors
///
/// Fails when cargo cannot give the metadata, cannot be started, or fails to check the
/// workspace (the error names the command to run and does not echo cargo's diagnostics), when
/// its messages are not what was expected, or when an artifact has no readable dep-info.
///
/// Gives the workspace root as cargo names it (`process::workspace_root`) with the
/// artifacts: the paths of cargo's messages are made relative to that root, and it is the root
/// to read the files from. The artifacts are sorted by package, kind, name and test, whatever
/// order cargo built them in.
pub fn artifacts(root: &Path, options: ViewOptions) -> Result<(PathBuf, Vec<Artifact>)> {
    let metadata = cargo_metadata(root, true, options.locked, options.offline)?;
    let cargo_root = workspace_root(&metadata);
    let root = cargo_root.as_path();
    let members: BTreeMap<&PackageId, (&str, String)> = metadata
        .packages
        .iter()
        .map(|package| {
            let manifest = package.manifest_path.as_std_path();
            let dir = manifest.parent().unwrap_or(manifest);
            let dir = repo_relative(root, &dir.to_string_lossy());
            (&package.id, (package.name.as_str(), dir))
        })
        .collect();

    let command = check_command(options, root);
    let output = cargo_output_showing_stderr(root, &command)?;
    let failed = || Error::Command {
        program: "cargo".to_owned(),
        args: command.args.clone(),
        status: output.status.code(),
        stderr: CHECK_FAILED.to_owned(),
    };
    if !output.status.success() {
        return Err(failed());
    }

    let mut artifacts = Vec::new();
    for message in Message::parse_stream(Cursor::new(&output.stdout)) {
        let message = message.map_err(|_| Error::Parse {
            what: "the output of `cargo check`".to_owned(),
            message: "a line is not a cargo message".to_owned(),
        })?;
        match message {
            Message::BuildFinished(finished) if !finished.success => {
                return Err(failed());
            }
            Message::CompilerArtifact(artifact) => {
                let Some((package, manifest_dir)) = members.get(&artifact.package_id) else {
                    continue;
                };
                let kind = target_kind(artifact.target.kind.first())?;
                let first = artifact.filenames.first().ok_or_else(|| Error::Parse {
                    what: "a cargo artifact".to_owned(),
                    message: "it has no files".to_owned(),
                })?;
                let dep_info = find_dep_info(first.as_std_path(), kind)?;
                let text = fs::read_to_string(&dep_info).map_err(|source| Error::Io {
                    path: dep_info.clone(),
                    source,
                })?;
                artifacts.push(Artifact {
                    package: (*package).to_owned(),
                    manifest_dir: manifest_dir.clone(),
                    kind,
                    name: artifact.target.name.clone(),
                    src_path: repo_relative(root, artifact.target.src_path.as_str()),
                    test: artifact.profile.test,
                    files: parse_dep_info(&text)
                        .iter()
                        .map(|file| repo_relative(root, file))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                });
            }
            _ => {}
        }
    }
    artifacts.sort_by(|a, b| {
        (&a.package, a.kind, &a.name, a.test).cmp(&(&b.package, b.kind, &b.name, b.test))
    });
    Ok((cargo_root, artifacts))
}

fn target_kind(kind: Option<&CargoKind>) -> Result<TargetKind> {
    Ok(match kind {
        Some(CargoKind::Bin) => TargetKind::Bin,
        Some(CargoKind::Test) => TargetKind::Test,
        Some(CargoKind::Bench) => TargetKind::Bench,
        Some(CargoKind::Example) => TargetKind::Example,
        Some(CargoKind::CustomBuild) => TargetKind::BuildScript,
        Some(
            CargoKind::Lib
            | CargoKind::ProcMacro
            | CargoKind::CDyLib
            | CargoKind::DyLib
            | CargoKind::RLib
            | CargoKind::StaticLib,
        ) => TargetKind::Lib,
        _ => {
            return Err(Error::Parse {
                what: "a cargo artifact".to_owned(),
                message: "its target kind is not one this gate knows".to_owned(),
            });
        }
    })
}

/// The dep-info file of the artifact whose first file is `first`.
///
/// Cargo reports a build script as a hard link without the hash (`build-script-build`), on every
/// OS, so its dep-info is found from the folder (`<package>-<hash>`) instead.
fn find_dep_info(first: &Path, kind: TargetKind) -> Result<PathBuf> {
    if kind != TargetKind::BuildScript {
        return Ok(dep_info_of(first));
    }
    first
        .parent()
        .and_then(build_script_dep_info)
        .ok_or_else(|| Error::Parse {
            what: "a cargo build script artifact".to_owned(),
            message: "its folder has no hash to name its dep-info file by".to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_covers_the_workspace_and_every_target() {
        let root = Path::new("some").join("root");
        let args = |options| check_command(options, &root).args;
        assert_eq!(
            args(ViewOptions::REPOSITORY),
            [
                "check",
                "--workspace",
                "--all-targets",
                "--message-format=json",
                "--locked"
            ]
        );
        assert_eq!(
            args(ViewOptions::FIXTURE),
            [
                "check",
                "--workspace",
                "--all-targets",
                "--message-format=json",
                "--offline",
                "--target-dir",
                &root.join("target").to_string_lossy()
            ]
        );
    }
}
