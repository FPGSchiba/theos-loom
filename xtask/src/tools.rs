//! The tools the gate needs besides cargo, pinned in `gate/tools.toml`.
//!
//! cargo-nextest runs the tests, cargo-deny checks the dependencies and cargo-llvm-cov measures
//! coverage; gitleaks scans for secrets, and Node and pnpm run the UI's checks. CI installs
//! exactly the pinned versions, and `cargo xtask tools` reports any local tool that is missing or
//! at another version, so the same code cannot give different verdicts. Node is pinned by its
//! major version, every other tool exactly. [`run_for`] checks only the tools of the parts that
//! will run, since a tool no part uses cannot change a verdict. `cargo xtask tools --install`
//! installs the cargo tools that are wrong and prints how to install the others, and
//! `--print-install-spec` prints the pins of the cargo tools in the form the setup action takes.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::Deserialize;

use crate::ci::CiEvent;
use crate::process::{CargoCommand, capture_cargo, capture_program, run_cargo};
use crate::verdict::{Check, Finding, Verdict};
use crate::{Error, Result};

/// The file that pins the versions, relative to the repository root.
const TOOLS_FILE: &str = "gate/tools.toml";

/// A tool the gate knows how to check. Every one must be pinned, and no other may be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Runs the tests.
    CargoNextest,
    /// Checks the dependencies.
    CargoDeny,
    /// Measures coverage.
    CargoLlvmCov,
    /// Scans for secrets.
    Gitleaks,
    /// Runs the UI's checks; pinned by major version.
    Node,
    /// Installs and runs the UI's tools.
    Pnpm,
}

impl Tool {
    /// Every tool, in the order they are listed, checked and installed.
    pub const ALL: [Tool; 6] = [
        Tool::CargoNextest,
        Tool::CargoDeny,
        Tool::CargoLlvmCov,
        Tool::Gitleaks,
        Tool::Node,
        Tool::Pnpm,
    ];

    /// The name under `[tools]` in `gate/tools.toml`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CargoNextest => "cargo-nextest",
            Self::CargoDeny => "cargo-deny",
            Self::CargoLlvmCov => "cargo-llvm-cov",
            Self::Gitleaks => "gitleaks",
            Self::Node => "node",
            Self::Pnpm => "pnpm",
        }
    }

    /// The tool called `name`, or `None` when the gate has no such tool.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Tool> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// True for the tools that `cargo install` and the setup action's install spec can provide.
    const fn is_cargo_tool(self) -> bool {
        matches!(
            self,
            Self::CargoNextest | Self::CargoDeny | Self::CargoLlvmCov
        )
    }
}

/// The tools a part runs. There is no wildcard arm, so a new [`Check`] does not compile until
/// it is decided here.
///
/// `Test` runs nextest, `Coverage` runs llvm-cov (which runs the tests through nextest) and
/// `Deny` runs cargo-deny. No part uses gitleaks, Node or pnpm yet: the parts that do (`secrets`
/// and `ui`) are added with their tools.
#[must_use]
pub fn needed_by(check: Check) -> &'static [Tool] {
    match check {
        Check::Test => &[Tool::CargoNextest],
        Check::Coverage => &[Tool::CargoLlvmCov, Tool::CargoNextest],
        Check::Deny => &[Tool::CargoDeny],
        Check::Tools
        | Check::Fmt
        | Check::Clippy
        | Check::Docs
        | Check::Lints
        | Check::AcTrace
        | Check::CommitMsg
        | Check::PrTitle
        | Check::PrBody
        | Check::Approval
        | Check::Github => &[],
    }
}

/// A tool and the exact version the gate runs with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolPin {
    /// The name under `[tools]`, such as `cargo-deny`; for the cargo tools it is also the crate
    /// and the name their version output starts with.
    pub name: String,
    /// The version, such as `0.20.2`; for Node the major version, such as `24`.
    pub version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFile {
    tools: BTreeMap<String, String>,
}

/// The pins of `gate/tools.toml`, in the order of [`Tool::ALL`].
///
/// # Errors
///
/// Fails when the file cannot be read, is not the expected TOML, lacks one of the tools, pins
/// another tool, or has a version that is not one.
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
        let shape = "it has the table `[tools]` that maps every tool of the gate to a version, \
                     and nothing else";
        config_error(match line {
            Some(line) => format!("not the expected TOML, near line {line}: {shape}"),
            None => format!("not the expected TOML: {shape}"),
        })
    })?;
    if let Some(unknown) = file
        .tools
        .keys()
        .find(|name| Tool::from_name(name).is_none())
    {
        let known: Vec<&str> = Tool::ALL.iter().map(|tool| tool.name()).collect();
        return Err(config_error(format!(
            "`{unknown}` is not a tool the gate checks; it checks {}",
            known.join(", ")
        )));
    }
    Tool::ALL
        .iter()
        .map(|tool| {
            let name = tool.name();
            let version = file
                .tools
                .get(name)
                .ok_or_else(|| config_error(format!("no version for `{name}` under [tools]")))?;
            let (valid, what) = if *tool == Tool::Node {
                (
                    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()),
                    "a major version",
                )
            } else {
                (is_version(version), "a version")
            };
            if !valid {
                return Err(config_error(format!(
                    "`{version}` is not {what} of `{name}`"
                )));
            }
            Ok(ToolPin {
                name: name.to_owned(),
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

/// The version `tool` reported in `output`, in the form its pin has: the cargo tools as in
/// [`parse_version`]; gitleaks from `gitleaks version 8.28.0` or `v8.28.0`; pnpm from
/// `10.20.0`; Node's major version from `v24.19.0`. `None` when the output has no such version.
#[must_use]
pub fn parse_installed(tool: Tool, output: &str) -> Option<String> {
    if tool.is_cargo_tool() {
        return parse_version(tool.name(), output);
    }
    let word = output
        .lines()
        .find_map(|line| line.split_whitespace().last())?;
    let version = word.strip_prefix('v').unwrap_or(word);
    if !is_version(version) {
        return None;
    }
    match tool {
        Tool::Node => version
            .split('.')
            .next()
            .filter(|major| major.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_owned),
        _ => Some(version.to_owned()),
    }
}

/// The pins of the cargo tools as the setup action takes them: `name@version`, joined with
/// commas. The other tools are not in it: the action cannot install them.
#[must_use]
pub fn install_spec(pins: &[ToolPin]) -> String {
    pins.iter()
        .filter(|pin| Tool::from_name(&pin.name).is_some_and(Tool::is_cargo_tool))
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

/// How to get the pinned tool, as a person types it, for findings: the install command of a cargo
/// tool, and for the others the steps that `--install` prints instead of running.
fn install_text(pin: &ToolPin) -> String {
    match Tool::from_name(&pin.name) {
        Some(Tool::Gitleaks) if cfg!(windows) => {
            format!("winget install Gitleaks.Gitleaks --version {}", pin.version)
        }
        Some(Tool::Gitleaks) => format!(
            "download the archive of gitleaks v{} from \
             https://github.com/gitleaks/gitleaks/releases and put `gitleaks` on the path",
            pin.version
        ),
        Some(Tool::Node) => format!(
            "install Node {} (for example with nvm, fnm or from https://nodejs.org)",
            pin.version
        ),
        Some(Tool::Pnpm) => format!(
            "corepack enable && corepack prepare pnpm@{} --activate",
            pin.version
        ),
        _ => format!("cargo {}", install_command(pin).args.join(" ")),
    }
}

/// What asking a tool for its version gave.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Installed {
    /// The tool is not installed, or failed.
    Missing,
    /// The tool answered with something that has no version in it.
    Unreadable(String),
    /// The version the tool reported (Node: its major version).
    Version(String),
}

fn probe(root: &Path, pin: &ToolPin) -> Result<Installed> {
    let Some(tool) = Tool::from_name(&pin.name) else {
        return Ok(Installed::Missing);
    };
    let args = |args: &[&str]| CargoCommand {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: Vec::new(),
        env_remove: Vec::new(),
    };
    let output = if tool.is_cargo_tool() {
        let subcommand = pin.name.strip_prefix("cargo-").unwrap_or(&pin.name);
        capture_cargo(root, &args(&[subcommand, "--version"]))?
    } else {
        // `which` finds `pnpm.cmd` on Windows, which `Command` does not find by name.
        let Ok(program) = which::which(tool.name()) else {
            return Ok(Installed::Missing);
        };
        let argument = if tool == Tool::Gitleaks {
            "version"
        } else {
            "--version"
        };
        capture_program(program.as_os_str(), root, &args(&[argument]))?
    };
    Ok(match output {
        None => Installed::Missing,
        Some(output) => match parse_installed(tool, &output) {
            Some(version) => Installed::Version(version),
            None => Installed::Unreadable(output.trim().to_owned()),
        },
    })
}

/// A finding when the installed tool is not the pinned one, naming how to install it.
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

/// Asks the pinned `tools` for their versions and reports each one that is missing or differs.
fn check_tools(root: &Path, tools: &[Tool]) -> Result<Verdict> {
    let mut findings = Vec::new();
    for pin in load_pins(root)? {
        if Tool::from_name(&pin.name).is_some_and(|tool| tools.contains(&tool)) {
            findings.extend(judge(&pin, &probe(root, &pin)?));
        }
    }
    Ok(Verdict::with(Check::Tools, findings))
}

/// Asks every pinned tool for its version and reports each one that is missing or differs.
///
/// # Errors
///
/// Fails when the pins cannot be loaded or a program cannot be started.
pub fn check_installed(root: &Path) -> Result<Verdict> {
    check_tools(root, &Tool::ALL)
}

/// Asks for the version of every tool the parts `checks` need, and of no other: a tool that no
/// part of the run uses cannot change a verdict. Passes with a note when no part needs a tool.
///
/// # Errors
///
/// Fails when the pins cannot be loaded or a program cannot be started.
pub fn run_for(root: &Path, checks: &[Check], _out: &mut dyn Write) -> Result<Verdict> {
    let needed: Vec<Tool> = Tool::ALL
        .into_iter()
        .filter(|tool| checks.iter().any(|check| needed_by(*check).contains(tool)))
        .collect();
    let mut verdict = check_tools(root, &needed)?;
    if needed.is_empty() {
        verdict
            .notes
            .push("no part of this run needs a pinned tool".to_owned());
    }
    Ok(verdict)
}

/// Runs `cargo install --locked <tool> --version <v>` for every pinned cargo tool that is missing
/// or at another version. Cargo shows its own output; installing compiles the tool, which takes
/// minutes and needs the network. Node, pnpm and gitleaks are not installed: for each one that is
/// wrong, the steps to install it are written to `out`.
///
/// # Errors
///
/// Fails when the pins cannot be loaded, a program cannot be started, an install fails, or `out`
/// cannot be written.
pub fn install_missing(root: &Path, out: &mut dyn Write) -> Result<()> {
    for pin in load_pins(root)? {
        if matches!(probe(root, &pin)?, Installed::Version(found) if found == pin.version) {
            continue;
        }
        if !Tool::from_name(&pin.name).is_some_and(Tool::is_cargo_tool) {
            writeln!(out, "{} {}: {}", pin.name, pin.version, install_text(&pin))
                .map_err(crate::report::output_error)?;
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

/// The `tools` check with every pin: [`check_installed`], with the same arguments as every other
/// check's `run`. `_ci` and `_out` are not used.
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
            pin("gitleaks", "8.30.1"),
            pin("node", "24"),
            pin("pnpm", "12.23.0"),
        ]
    }

    /// A repository that pins every tool at a version nothing is ever released as, so that
    /// whichever tool is asked for its version fails.
    fn repo_with_impossible_pins() -> crate::Result<tempfile::TempDir> {
        let root = tempfile::tempdir().map_err(io_error(PathBuf::from("<temp>")))?;
        let gate = root.path().join("gate");
        std::fs::create_dir(&gate).map_err(io_error(gate.clone()))?;
        let file = gate.join("tools.toml");
        std::fs::write(
            &file,
            "[tools]\ncargo-nextest = \"0.0.1\"\ncargo-deny = \"0.0.1\"\ncargo-llvm-cov = \"0.0.1\"\n\
             gitleaks = \"0.0.1\"\nnode = \"1\"\npnpm = \"0.0.1\"\n",
        )
        .map_err(io_error(file))?;
        Ok(root)
    }

    fn mentioned_tools(verdict: &Verdict) -> Vec<&'static str> {
        Tool::ALL
            .into_iter()
            .filter(|tool| {
                verdict
                    .findings
                    .iter()
                    .any(|finding| finding.message.starts_with(tool.name()))
            })
            .map(Tool::name)
            .collect()
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

    // AC: TU-00.3
    #[test]
    fn the_install_spec_lists_only_cargo_installable_tools() {
        // taiki-e/install-action has no manifest for gitleaks, Node or pnpm.
        assert_eq!(
            install_spec(&pins()),
            "cargo-nextest@0.9.150,cargo-deny@0.20.2,cargo-llvm-cov@0.9.1"
        );
        assert_eq!(install_spec(&[]), "");
    }

    // AC: TU-00.3
    #[test]
    fn versions_are_read_from_each_tools_output() {
        let cases = [
            (Tool::Node, "v24.19.0\n", Some("24")),
            (Tool::Pnpm, "10.20.0\n", Some("10.20.0")),
            (Tool::Gitleaks, "gitleaks version 8.28.0\n", Some("8.28.0")),
            (Tool::Gitleaks, "v8.28.0\n", Some("8.28.0")),
            (
                Tool::CargoNextest,
                "cargo-nextest 0.9.150 (a1b2c3d4e 2026-10-01)\n",
                Some("0.9.150"),
            ),
            (Tool::CargoDeny, "cargo-deny 0.20.2\n", Some("0.20.2")),
            (Tool::CargoLlvmCov, "cargo-llvm-cov 0.9.1\n", Some("0.9.1")),
            (Tool::Node, "garbage", None),
            (Tool::Pnpm, "", None),
            (Tool::Gitleaks, "gitleaks version", None),
        ];
        for (tool, output, expected) in cases {
            assert_eq!(
                parse_installed(tool, output).as_deref(),
                expected,
                "{tool:?} {output:?}"
            );
        }
    }

    // AC: TU-00.3
    #[test]
    fn node_matches_its_pin_by_major() {
        let node = pin("node", "24");
        let installed = |output: &str| {
            parse_installed(Tool::Node, output)
                .map_or(Installed::Unreadable(output.to_owned()), Installed::Version)
        };
        assert_eq!(judge(&node, &installed("v24.11.1\n")), None);
        assert_eq!(judge(&node, &installed("v24.19.0\n")), None);
        let message = judge(&node, &installed("v22.1.0\n"))
            .map(|finding| finding.message)
            .unwrap_or_default();
        assert!(message.contains("node 22 is installed"), "{message}");
        assert!(message.contains("pins 24"), "{message}");
    }

    // AC: TU-00.3
    #[test]
    fn only_the_tools_of_the_parts_that_run_are_checked() -> crate::Result<()> {
        let root = repo_with_impossible_pins()?;
        let mut out = Vec::new();
        let run = |checks: &[Check], out: &mut Vec<u8>| run_for(root.path(), checks, out);

        let none = run(&[Check::Tools, Check::Fmt], &mut out)?;
        assert!(none.passed(), "{none:?}");
        assert!(!none.notes.is_empty(), "a run that checks nothing says so");

        assert_eq!(
            mentioned_tools(&run(&[Check::Test], &mut out)?),
            ["cargo-nextest"]
        );
        assert_eq!(
            mentioned_tools(&run(&[Check::Coverage], &mut out)?),
            ["cargo-nextest", "cargo-llvm-cov"]
        );
        assert_eq!(
            mentioned_tools(&run(&[Check::Deny, Check::Lints], &mut out)?),
            ["cargo-deny"]
        );
        // Without a list of parts, every pin is checked.
        assert_eq!(
            mentioned_tools(&check_installed(root.path())?),
            Tool::ALL.map(Tool::name)
        );
        Ok(())
    }

    #[test]
    fn every_part_names_the_tools_it_needs() {
        assert_eq!(needed_by(Check::Test), [Tool::CargoNextest]);
        assert_eq!(
            needed_by(Check::Coverage),
            [Tool::CargoLlvmCov, Tool::CargoNextest]
        );
        assert_eq!(needed_by(Check::Deny), [Tool::CargoDeny]);
        assert_eq!(needed_by(Check::Fmt), []);
    }

    #[test]
    fn pins_load_from_gate_tools_toml() -> crate::Result<()> {
        assert_eq!(load_pins(&repo_root())?, pins());
        Ok(())
    }

    #[test]
    fn pins_come_out_in_the_fixed_order_whatever_the_file_order() -> crate::Result<()> {
        let text = "[tools]\npnpm = \"12.23.0\"\nnode = \"24\"\ngitleaks = \"8.30.1\"\n\
                    cargo-llvm-cov = \"0.9.1\"\ncargo-deny = \"0.20.2\"\ncargo-nextest = \"0.9.150\"\n";
        assert_eq!(parse_pins(text)?, pins());
        Ok(())
    }

    // AC: TU-00.3
    #[test]
    fn pins_name_every_tool() {
        let complete = "cargo-nextest = \"0.9.150\"\ncargo-deny = \"0.20.2\"\n\
                        cargo-llvm-cov = \"0.9.1\"\ngitleaks = \"8.30.1\"\nnode = \"24\"\n\
                        pnpm = \"12.23.0\"\n";
        assert_eq!(
            parse_pins(&format!("[tools]\n{complete}")).ok(),
            Some(pins())
        );
        let cases = [
            (complete.replace("gitleaks = \"8.30.1\"\n", ""), "gitleaks"),
            (complete.replace("node = \"24\"\n", ""), "node"),
            (complete.replace("pnpm = \"12.23.0\"\n", ""), "pnpm"),
            (format!("{complete}yarn = \"4.0.0\"\n"), "yarn"),
            (complete.replace("\"24\"", "\"24.19.0\""), "major version"),
            (complete.replace("\"24\"", "\"v24\""), "major version"),
        ];
        for (tools, mentions) in cases {
            let result = parse_pins(&format!("[tools]\n{tools}"));
            assert!(
                matches!(&result, Err(crate::Error::Config { file, message })
                    if file == "gate/tools.toml" && message.contains(mentions)),
                "{tools:?}: {result:?}"
            );
        }
    }

    #[test]
    fn a_tools_file_with_a_missing_unknown_or_malformed_pin_is_a_config_error() {
        let complete = "cargo-nextest = \"0.9.150\"\ncargo-deny = \"0.20.2\"\n\
                        cargo-llvm-cov = \"0.9.1\"\ngitleaks = \"8.30.1\"\nnode = \"24\"\n\
                        pnpm = \"12.23.0\"\n";
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
                "near line 9",
            ),
            (
                format!("[tools]\n{}", complete.replace("\"0.9.150\"", "\"--git\"")),
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
        let root = repo_with_impossible_pins()?;
        let verdict = check_installed(root.path())?;
        assert_eq!(verdict.check, Check::Tools);
        assert_eq!(verdict.findings.len(), 6);
        for (finding, expected) in verdict.findings.iter().zip([
            "cargo install --locked cargo-nextest --version 0.0.1",
            "cargo install --locked cargo-deny --version 0.0.1",
            "cargo install --locked cargo-llvm-cov --version 0.0.1",
            "gitleaks",
            "install Node 1",
            "corepack prepare pnpm@0.0.1 --activate",
        ]) {
            assert!(finding.message.contains(expected), "{}", finding.message);
        }
        Ok(())
    }
}
