//! The tools the gate needs besides cargo, pinned in `gate/tools.toml`.
//!
//! cargo-nextest runs the tests, cargo-deny checks the dependencies and cargo-llvm-cov measures
//! coverage. CI installs exactly the pinned versions, and `cargo xtask tools` reports any local
//! tool that is missing or at another version, so the same code cannot give different verdicts.
//! `cargo xtask tools --install` installs what is wrong, and `--print-install-spec` prints the
//! pins in the form the setup action takes.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::Deserialize;

use crate::ci::CiEvent;
use crate::process::{CargoCommand, capture_cargo, run_cargo};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The file that pins the versions, relative to the repository root.
const TOOLS_FILE: &str = "gate/tools.toml";

/// The tools the gate knows how to check, in the order they are listed and installed. Every one
/// must be pinned, and no other may be.
const KNOWN: [&str; 3] = ["cargo-nextest", "cargo-deny", "cargo-llvm-cov"];

/// A tool and the exact version the gate runs with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolPin {
    /// The crate, which is also the name its version output starts with, such as `cargo-deny`.
    pub name: String,
    /// The version, such as `0.20.2`.
    pub version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFile {
    tools: BTreeMap<String, String>,
}

/// The pins of `gate/tools.toml`, in the fixed order `cargo-nextest`, `cargo-deny`,
/// `cargo-llvm-cov`.
///
/// # Errors
///
/// Fails when the file cannot be read, is not the expected TOML, lacks one of the three tools,
/// pins another tool, or has a version that is not one.
pub fn load_pins(root: &Path) -> Result<Vec<ToolPin>> {
    let path = root.join(TOOLS_FILE);
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
    parse_pins(&text)
}

/// The pins in `text`. A TOML error is reported by its line and nothing of the text, because the
/// parser's own message quotes the lines it complains about.
fn parse_pins(text: &str) -> Result<Vec<ToolPin>> {
    let file: ToolsFile = toml::from_str(text).map_err(|error| {
        let line = error.span().map(|span| {
            let before = text.get(..span.start).unwrap_or(text);
            before.matches('\n').count() + 1
        });
        let shape = "it has the table `[tools]` that maps `cargo-nextest`, `cargo-deny` and \
                     `cargo-llvm-cov` to versions, and nothing else";
        config_error(match line {
            Some(line) => format!("not the expected TOML, near line {line}: {shape}"),
            None => format!("not the expected TOML: {shape}"),
        })
    })?;
    if let Some(unknown) = file
        .tools
        .keys()
        .find(|name| !KNOWN.contains(&name.as_str()))
    {
        return Err(config_error(format!(
            "`{unknown}` is not a tool the gate checks; it checks {}",
            KNOWN.join(", ")
        )));
    }
    KNOWN
        .iter()
        .map(|name| {
            let version = file
                .tools
                .get(*name)
                .ok_or_else(|| config_error(format!("no version for `{name}` under [tools]")))?;
            if !is_version(version) {
                return Err(config_error(format!(
                    "`{version}` is not a version of `{name}`"
                )));
            }
            Ok(ToolPin {
                name: (*name).to_owned(),
                version: version.clone(),
            })
        })
        .collect()
}

fn config_error(message: String) -> Error {
    Error::Config {
        file: TOOLS_FILE.to_owned(),
        message,
    }
}

/// True for text that can be a version and cannot be read as an option: it starts with a digit
/// and has only letters, digits and `.`, `-`, `+`.
fn is_version(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// The version `tool` reports in its `--version` output: the second word of the line that starts
/// with the tool's name, such as `0.9.150` in `cargo-nextest 0.9.150 (a1b2c3d 2026-10-01)`.
/// `None` when no line has that shape.
#[must_use]
pub fn parse_version(tool: &str, output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if words.next()? != tool {
            return None;
        }
        words
            .next()
            .filter(|version| is_version(version))
            .map(str::to_owned)
    })
}

/// The pins as the setup action takes them: `name@version`, joined with commas.
#[must_use]
pub fn install_spec(pins: &[ToolPin]) -> String {
    pins.iter()
        .map(|pin| format!("{}@{}", pin.name, pin.version))
        .collect::<Vec<_>>()
        .join(",")
}

/// `cargo install --locked <tool> --version <version>`, as a command to run.
fn install_command(pin: &ToolPin) -> CargoCommand {
    CargoCommand {
        args: ["install", "--locked", &pin.name, "--version", &pin.version]
            .map(str::to_owned)
            .to_vec(),
        env: Vec::new(),
        env_remove: Vec::new(),
    }
}

/// The install command as a person types it, for findings.
fn install_text(pin: &ToolPin) -> String {
    format!("cargo {}", install_command(pin).args.join(" "))
}

/// What asking a tool for its version gave.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Installed {
    /// Cargo knows no such subcommand, or the tool failed.
    Missing,
    /// The tool answered with something that has no version in it.
    Unreadable(String),
    /// The version the tool reported.
    Version(String),
}

fn probe(root: &Path, pin: &ToolPin) -> Result<Installed> {
    let subcommand = pin.name.strip_prefix("cargo-").unwrap_or(&pin.name);
    let cmd = CargoCommand {
        args: vec![subcommand.to_owned(), "--version".to_owned()],
        env: Vec::new(),
        env_remove: Vec::new(),
    };
    Ok(match capture_cargo(root, &cmd)? {
        None => Installed::Missing,
        Some(output) => match parse_version(&pin.name, &output) {
            Some(version) => Installed::Version(version),
            None => Installed::Unreadable(output.trim().to_owned()),
        },
    })
}

/// A finding when the installed tool is not the pinned one, naming the install command.
fn judge(pin: &ToolPin, installed: &Installed) -> Option<Finding> {
    let ToolPin { name, version } = pin;
    let fix = install_text(pin);
    let message = match installed {
        Installed::Version(found) if found == version => return None,
        Installed::Version(found) => {
            format!("{name} {found} is installed but {TOOLS_FILE} pins {version}; run `{fix}`")
        }
        Installed::Missing => {
            format!("{name} is not installed; {TOOLS_FILE} pins {version}; run `{fix}`")
        }
        Installed::Unreadable(output) => format!(
            "cannot read the version of {name} from its output ({output:?}); {TOOLS_FILE} pins \
             {version}; run `{fix}`"
        ),
    };
    Some(Finding::new(message))
}

/// Asks every pinned tool for its version and reports each one that is missing or differs.
///
/// # Errors
///
/// Fails when the pins cannot be loaded or cargo cannot be started.
pub fn check_installed(root: &Path) -> Result<Verdict> {
    let mut findings = Vec::new();
    for pin in load_pins(root)? {
        findings.extend(judge(&pin, &probe(root, &pin)?));
    }
    Ok(Verdict::with(Check::Tools, findings))
}

/// Runs `cargo install --locked <tool> --version <v>` for every pinned tool that is missing or
/// at another version. Cargo shows its own output; installing compiles the tool, which takes
/// minutes and needs the network.
///
/// # Errors
///
/// Fails when the pins cannot be loaded, cargo cannot be started, or an install fails.
pub fn install_missing(root: &Path) -> Result<()> {
    for pin in load_pins(root)? {
        if matches!(probe(root, &pin)?, Installed::Version(found) if found == pin.version) {
            continue;
        }
        let cmd = install_command(&pin);
        if !run_cargo(root, &cmd)? {
            return Err(Error::Command {
                program: "cargo".to_owned(),
                args: cmd.args,
                status: None,
                stderr: "the install failed; cargo's output is above".to_owned(),
            });
        }
    }
    Ok(())
}

/// The `tools` check: [`check_installed`], with the same arguments as every other check's `run`.
/// `_ci` and `_out` are not used.
///
/// # Errors
///
/// As [`check_installed`].
pub fn run(root: &Path, _ci: Option<&CiEvent>, _out: &mut dyn Write) -> Result<Verdict> {
    check_installed(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pin(name: &str, version: &str) -> ToolPin {
        ToolPin {
            name: name.to_owned(),
            version: version.to_owned(),
        }
    }

    fn pins() -> Vec<ToolPin> {
        vec![
            pin("cargo-nextest", "0.9.150"),
            pin("cargo-deny", "0.20.2"),
            pin("cargo-llvm-cov", "0.9.1"),
        ]
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn io_error(path: PathBuf) -> impl FnOnce(std::io::Error) -> crate::Error {
        move |source| crate::Error::Io { path, source }
    }

    #[test]
    fn parses_tool_versions() {
        assert_eq!(
            parse_version(
                "cargo-nextest",
                "cargo-nextest 0.9.150 (a1b2c3d4e 2026-10-01)\n"
            ),
            Some("0.9.150".to_owned())
        );
        assert_eq!(
            parse_version("cargo-deny", "cargo-deny 0.20.2\n"),
            Some("0.20.2".to_owned())
        );
        assert_eq!(
            parse_version("cargo-llvm-cov", "cargo-llvm-cov 0.9.1\n"),
            Some("0.9.1".to_owned())
        );
        assert_eq!(parse_version("cargo-deny", "garbage"), None);
        assert_eq!(parse_version("cargo-deny", ""), None);
        assert_eq!(parse_version("cargo-deny", "cargo-deny"), None);
        assert_eq!(parse_version("cargo-deny", "cargo-deny nightly"), None);
        assert_eq!(parse_version("cargo-deny", "cargo-nextest 0.9.150"), None);
    }

    #[test]
    fn install_spec_lists_pins_in_order() {
        assert_eq!(
            install_spec(&pins()),
            "cargo-nextest@0.9.150,cargo-deny@0.20.2,cargo-llvm-cov@0.9.1"
        );
        assert_eq!(install_spec(&[]), "");
    }

    #[test]
    fn pins_load_from_gate_tools_toml() -> crate::Result<()> {
        assert_eq!(load_pins(&repo_root())?, pins());
        Ok(())
    }

    #[test]
    fn pins_come_out_in_the_fixed_order_whatever_the_file_order() -> crate::Result<()> {
        let text = "[tools]\ncargo-llvm-cov = \"0.9.1\"\ncargo-deny = \"0.20.2\"\ncargo-nextest = \"0.9.150\"\n";
        assert_eq!(parse_pins(text)?, pins());
        Ok(())
    }

    #[test]
    fn a_tools_file_with_a_missing_unknown_or_malformed_pin_is_a_config_error() {
        let complete =
            "cargo-nextest = \"0.9.150\"\ncargo-deny = \"0.20.2\"\ncargo-llvm-cov = \"0.9.1\"\n";
        let cases = [
            (
                "[tools]\ncargo-nextest = \"0.9.150\"\ncargo-deny = \"0.20.2\"\n".to_owned(),
                "cargo-llvm-cov",
            ),
            (
                format!("[tools]\n{complete}cargo-fuzz = \"0.12.0\"\n"),
                "cargo-fuzz",
            ),
            (
                format!("[tools]\n{complete}\n[other]\nx = 1\n"),
                "near line 6",
            ),
            (
                "[tools]\ncargo-nextest = \"--git\"\ncargo-deny = \"0.20.2\"\ncargo-llvm-cov = \"0.9.1\"\n"
                    .to_owned(),
                "--git",
            ),
            ("[tools\n".to_owned(), "tools"),
        ];
        for (text, mentions) in cases {
            let result = parse_pins(&text);
            assert!(
                matches!(&result, Err(crate::Error::Config { file, message })
                    if file == "gate/tools.toml" && message.contains(mentions)),
                "{text:?}: {result:?}"
            );
        }
    }

    #[test]
    fn a_tools_file_that_is_not_the_expected_toml_is_reported_by_line_only() {
        // The error of the TOML parser quotes the lines it complains about, so the message is
        // built from the line number alone, as it is for the other files of `gate/`.
        let cases = [
            ("[tools\nSECRET = \"x\"\n", 1),
            (
                "[tools]\ncargo-nextest = \"0.9.150\"\n\n[SECRET]\nx = 1\n",
                4,
            ),
            ("tools = \"SECRET\"\n", 1),
            ("[tools]\ncargo-nextest = [\"SECRET\"]\n", 2),
            ("# SECRET\n[tools]\nSECRET = 5\n", 3),
        ];
        for (text, line) in cases {
            let result = parse_pins(text);
            let Err(crate::Error::Config { file, message }) = &result else {
                panic!("{text:?}: {result:?}");
            };
            assert_eq!(file, "gate/tools.toml");
            assert!(
                message.starts_with(&format!("not the expected TOML, near line {line}")),
                "{text:?}: {message}"
            );
            assert!(!message.contains("SECRET"), "{message}");
            assert!(!message.contains('\n'), "{message:?}");
        }
    }

    #[test]
    fn a_missing_tools_file_is_an_io_error() {
        let root = std::env::temp_dir().join("xtask-no-such-repo");
        let result = load_pins(&root);
        assert!(
            matches!(&result, Err(crate::Error::Io { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn the_install_command_is_cargo_install_locked_with_the_pinned_version() {
        let command = install_command(&pin("cargo-deny", "0.20.2"));
        let expected = CargoCommand {
            args: ["install", "--locked", "cargo-deny", "--version", "0.20.2"]
                .map(str::to_owned)
                .to_vec(),
            env: Vec::new(),
            env_remove: Vec::new(),
        };
        assert_eq!(command, expected);
        assert_eq!(
            install_text(&pin("cargo-deny", "0.20.2")),
            "cargo install --locked cargo-deny --version 0.20.2"
        );
    }

    #[test]
    fn the_installed_version_must_equal_the_pin() {
        let deny = pin("cargo-deny", "0.20.2");
        assert_eq!(judge(&deny, &Installed::Version("0.20.2".to_owned())), None);
    }

    #[test]
    fn every_other_state_is_a_finding_with_the_install_command() {
        let deny = pin("cargo-deny", "0.20.2");
        let states = [
            (Installed::Version("0.20.1".to_owned()), "0.20.1"),
            (Installed::Missing, "not installed"),
            (Installed::Unreadable("oops".to_owned()), "oops"),
        ];
        for (state, mentions) in states {
            let message = judge(&deny, &state)
                .map(|finding| finding.message)
                .unwrap_or_default();
            assert!(message.contains(mentions), "{message}");
            assert!(
                message.contains("cargo install --locked cargo-deny --version 0.20.2"),
                "{message}"
            );
        }
    }

    #[test]
    fn a_tool_that_is_not_at_its_pin_fails_the_check() -> crate::Result<()> {
        // No tool is ever released as 0.0.1, so this fails whatever is installed.
        let root = tempfile::tempdir().map_err(io_error(PathBuf::from("<temp>")))?;
        let gate = root.path().join("gate");
        std::fs::create_dir(&gate).map_err(io_error(gate.clone()))?;
        let file = gate.join("tools.toml");
        std::fs::write(
            &file,
            "[tools]\ncargo-nextest = \"0.0.1\"\ncargo-deny = \"0.0.1\"\ncargo-llvm-cov = \"0.0.1\"\n",
        )
        .map_err(io_error(file))?;
        let verdict = check_installed(root.path())?;
        assert_eq!(verdict.check, Check::Tools);
        assert_eq!(verdict.findings.len(), 3);
        for (finding, tool) in
            verdict
                .findings
                .iter()
                .zip(["cargo-nextest", "cargo-deny", "cargo-llvm-cov"])
        {
            let expected = format!("cargo install --locked {tool} --version 0.0.1");
            assert!(finding.message.contains(&expected), "{}", finding.message);
        }
        Ok(())
    }
}
