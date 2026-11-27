//! The `cargo xtask` command line: gate tooling (`docs/gate.md`).
//!
//! Every check is a subcommand named like the check. Exit code 0 means the check passed, 1 that
//! it found something, and 2 that it could not run.
//!
//! Adding a check is one arm in `run`: call the check's `run(root, ci, out)`, which gathers its
//! own inputs and returns a verdict, and let `report::emit` write it. Also add it to
//! `gate::run_check`, which the compiler asks for. `gate` is the one subcommand with many
//! verdicts: `gate::run` and `gate::run_pr` write their own table (through `report::emit_all`),
//! so their arms do not go through `report::emit`.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use xtask::branch::Phase;
use xtask::checks::{
    ac_trace, approval, cargo_cmds, commit_msg, coverage, lints, pr_body, pr_title, secrets, ui,
};
use xtask::gate;
use xtask::git::Git;
use xtask::github::settings;
use xtask::verdict::{Check, Verdict};
use xtask::view::Shared;
use xtask::{Error, ci, report, tools};

/// Gate tooling for the Loom workspace (`docs/gate.md`).
#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    /// The repository root. Defaults to the workspace this xtask was built from.
    #[arg(long, global = true, value_name = "DIR")]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check that the installed tools have the versions pinned in `gate/tools.toml`.
    Tools {
        /// Check only the tools these gate parts need (comma-separated), as a gate run that
        /// runs them would. Without it, every pin is checked.
        #[arg(long = "for", value_delimiter = ',', conflicts_with = "print_pin")]
        parts: Vec<String>,
        /// Install the pinned cargo tools that are missing or at another version (takes minutes
        /// and needs the network), and print how to install Node, pnpm and gitleaks; then check.
        #[arg(long, conflicts_with = "print_install_spec")]
        install: bool,
        /// Print the pins of the cargo tools as `name@version,…`, the form the setup action
        /// takes, and nothing else. With `--for`, only the tools those gate parts need.
        #[arg(long, conflicts_with_all = ["install", "print_pin"])]
        print_install_spec: bool,
        /// Print the pinned version of one tool, such as `gitleaks`, and nothing else.
        #[arg(long, value_name = "TOOL", conflicts_with_all = ["install", "print_install_spec"])]
        print_pin: Option<String>,
    },
    /// Check the formatting of every crate.
    Fmt,
    /// Run clippy on every crate with warnings denied.
    Clippy,
    /// Run the tests and the doc tests of every crate.
    Test,
    /// Build the documentation of every crate with warnings denied.
    Docs,
    /// Check licences, advisories, bans and sources.
    Deny,
    /// Measure line coverage per crate and compare it with the floors and the baseline.
    ///
    /// The baseline is the merge base of `HEAD` and the base revision, measured in a temporary
    /// work tree and cached in `target/xtask/coverage/cache`. Under GitHub Actions a push to
    /// `main` also stores its own result there.
    Coverage {
        /// The revision to compare with. Default: the base commit of the pull request under
        /// GitHub Actions, else `origin/main`.
        #[arg(long, value_name = "REV")]
        base: Option<String>,
    },
    /// Check the lint list, the tool configuration and the attributes in the sources.
    Lints,
    /// Check that every acceptance criterion of the phase has a tagged test, and that every tag
    /// names a criterion.
    ///
    /// The phase is the one of the branch: the head branch of the pull request under GitHub
    /// Actions, else the branch that is checked out. Only `phase/PNN-…` has a phase; elsewhere
    /// only the rules for tags are checked.
    AcTrace {
        /// The phase to check, instead of the phase of the branch.
        #[arg(long, value_name = "PNN")]
        phase: Option<Phase>,
    },
    /// Scan the commits of the change for secrets with gitleaks.
    ///
    /// Scans the commits of the pull request or push, or the branch since its merge base with
    /// `origin/main`; a detached `HEAD` and other events scan the whole history. Findings never
    /// show the secret.
    Secrets,
    /// Type-check, lint and test the web app in `web/` with pnpm.
    ///
    /// Runs `pnpm install --frozen-lockfile`, then `pnpm exec tsc -b`, `eslint` and `vitest run` in
    /// `web/`, with fixed arguments; it never runs a `package.json` script.
    Ui,
    /// Check that every commit of the branch is a Conventional Commit.
    ///
    /// Under GitHub Actions the commits and the branch come from the event. Any flag here
    /// replaces the event and takes the rest from git instead.
    CommitMsg {
        /// The commit before the first one to check. Default: the merge base of the head and
        /// `origin/main`.
        #[arg(long, value_name = "REV")]
        base: Option<String>,
        /// The last commit to check. Default: `HEAD`.
        #[arg(long, value_name = "REV")]
        head: Option<String>,
        /// The branch the commits are on, which decides whether `Phase:` is needed. Default:
        /// the branch that is checked out.
        #[arg(long, value_name = "NAME")]
        branch: Option<String>,
    },
    /// Check that the pull request title is a Conventional Commit that matches the branch.
    ///
    /// Under GitHub Actions the title and the branch come from the pull request event. A title
    /// only exists on a pull request, so anywhere else `--title` is needed; it replaces the event.
    PrTitle {
        /// The title to check.
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        /// The branch the pull request is from, which decides how the title must end. Default:
        /// the branch that is checked out.
        #[arg(long, value_name = "NAME", requires = "title")]
        branch: Option<String>,
    },
    /// Check that the pull request description has every section of the template, filled.
    ///
    /// Under GitHub Actions the description comes from the pull request event. A description
    /// only exists on a pull request, so anywhere else `--body-file` is needed; it replaces the
    /// event.
    PrBody {
        /// A file with the description to check.
        #[arg(long, value_name = "PATH")]
        body_file: Option<PathBuf>,
    },
    /// Check that a pull request touching a protected path has the label `approved`, given for
    /// the content it has now.
    ///
    /// Only runs under GitHub Actions for a pull request, with `GH_TOKEN` and
    /// `GITHUB_REPOSITORY` set: it reads and changes the labels and comments of the pull request.
    /// Run from the base commit, it reads `.loom/protected.toml` there.
    Approval,
    /// Run every check that needs no pull request, in order, and print one verdict table.
    ///
    /// Runs `tools`, `fmt`, `lints`, `commit-msg`, `secrets`, `clippy`, `docs`, `deny`, `test`, `coverage`,
    /// `ui` and `ac-trace` with the same code as their CI jobs, and goes on after a check fails. It
    /// leaves out `pr-title`, `pr-body` and `approval`. When a tool is not the pinned version,
    /// nothing but `tools` runs, because versions decide verdicts. `--only` and `--skip` run a
    /// part of it; `tools` always runs first. Exit code 1 when any check failed.
    Gate {
        /// Run only these checks (comma-separated), after `tools`.
        #[arg(
            long,
            value_delimiter = ',',
            value_name = "CHECKS",
            conflicts_with = "skip"
        )]
        only: Vec<String>,
        /// Run every check but these (comma-separated).
        #[arg(long, value_delimiter = ',', value_name = "CHECKS")]
        skip: Vec<String>,
    },
    /// Run `pr-title` and then `pr-body`, and print one verdict table. Both always run.
    ///
    /// Under GitHub Actions the title, the description and the branch come from the pull request
    /// event. Elsewhere, or to try other text, pass `--title` and `--body-file`; each replaces
    /// the event for its check.
    Pr {
        /// The title to check.
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        /// A file with the description to check.
        #[arg(long, value_name = "PATH")]
        body_file: Option<PathBuf>,
        /// The branch the pull request is from. Default: the branch that is checked out.
        #[arg(long, value_name = "NAME", requires = "title")]
        branch: Option<String>,
    },
    /// Check or apply the repository settings: the rulesets, the merge settings and the labels
    /// kept in `gate/` (`docs/gate.md` §6).
    Github {
        /// The repository as `owner/name`. Default: `GITHUB_REPOSITORY`, else the URL of the
        /// remote `origin`.
        #[arg(long, global = true, value_name = "OWNER/NAME")]
        repo: Option<String>,
        #[command(subcommand)]
        action: GithubAction,
    },
}

#[derive(Subcommand)]
enum GithubAction {
    /// Compare the repository on GitHub with `gate/ruleset-*.json` and `gate/repo-settings.toml`
    /// and report every difference.
    ///
    /// Only reads. `gh` takes its token from `GH_TOKEN`, or else uses the login it has stored.
    /// Settings that the token cannot read are reported as not checked.
    Check,
    /// Change the repository on GitHub to match `gate/ruleset-*.json` and
    /// `gate/repo-settings.toml`: create or replace both rulesets, patch the merge settings and
    /// set the labels.
    ///
    /// This changes security settings of the live repository. Run it only after Jann has agreed.
    Apply,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging();
    match run(&cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            // `{error}`, not `{error:#}`: the alternate form adds the chain of sources, and
            // `Error::Io` already has its source in its own message, so it would be said twice.
            // Nothing is lost, because no error here is wrapped by anyhow: `run` folds the
            // cause into the message where it adds context.
            // `write_error` marks every line, so no text of an error can start a workflow command.
            // If stderr is closed there is nobody left to tell.
            let _ = report::write_error(&mut std::io::stderr(), &error);
            ExitCode::from(2)
        }
    }
}

/// Runs the subcommand and says whether it passed.
fn run(cli: &Cli) -> anyhow::Result<bool> {
    let root = cli.root.clone().unwrap_or_else(default_root);
    tracing::debug!(root = %root.display(), "repository root");
    // The cause goes into the message, so that the plain `{error}` of `main` shows it.
    let ci = ci::from_env()
        .map_err(|error| anyhow::anyhow!("cannot read the GitHub Actions event: {error}"))?;
    let shared = Shared::new();
    let mut out = std::io::stdout().lock();
    // One arm per subcommand. A built check reads `<module>::run(&root, ci.as_ref(), &mut out)?`;
    // the checks that read code (`lints`, `ac-trace`) also take `&shared`, the view built on first
    // use.
    let verdict: Verdict = match &cli.command {
        Command::Tools {
            print_install_spec: true,
            parts,
            ..
        } => return print_install_spec(&root, parts, &mut out),
        Command::Tools {
            print_pin: Some(name),
            ..
        } => return print_pin(&root, name, &mut out),
        Command::Tools { install, parts, .. } => {
            tools_verdict(&root, ci.as_ref(), *install, parts, &mut out)?
        }
        Command::Fmt => cargo_cmds::run(Check::Fmt, &root, ci.as_ref(), &mut out)?,
        Command::Clippy => cargo_cmds::run(Check::Clippy, &root, ci.as_ref(), &mut out)?,
        Command::Test => cargo_cmds::run(Check::Test, &root, ci.as_ref(), &mut out)?,
        Command::Docs => cargo_cmds::run(Check::Docs, &root, ci.as_ref(), &mut out)?,
        Command::Deny => cargo_cmds::run(Check::Deny, &root, ci.as_ref(), &mut out)?,
        Command::Coverage { base } => {
            coverage::run_with_base(&root, ci.as_ref(), base.as_deref(), &mut out)?
        }
        Command::Lints => lints::run(&root, ci.as_ref(), &shared, &mut out)?,
        Command::AcTrace { phase: None } => ac_trace::run(&root, ci.as_ref(), &shared, &mut out)?,
        Command::AcTrace { phase } => {
            ac_trace::run_phase(&root, *phase, ci.is_some(), &shared, &mut out)?
        }
        Command::CommitMsg {
            base: None,
            head: None,
            branch: None,
        } => commit_msg::run(&root, ci.as_ref(), &mut out)?,
        Command::CommitMsg { base, head, branch } => {
            let git = Git::new(&root);
            let range = commit_msg::local_range(&git, base.as_deref(), head.as_deref())?;
            let branch = commit_msg::branch_kind(&git, branch.as_deref())?;
            let known_acs = commit_msg::known_acs(&root)?;
            commit_msg::check(&git, &range, &branch, &known_acs)?
        }
        Command::Secrets => secrets::run(&root, ci.as_ref(), &mut out)?,
        Command::Ui => ui::run(&root, ci.as_ref(), &mut out)?,
        Command::PrTitle { title: None, .. } => pr_title::run(&root, ci.as_ref(), &mut out)?,
        Command::PrTitle {
            title: Some(title),
            branch,
        } => {
            let branch = commit_msg::branch_kind(&Git::new(&root), branch.as_deref())?;
            pr_title::check_title(title, &branch, None)
        }
        Command::PrBody { body_file: None } => pr_body::run(&root, ci.as_ref(), &mut out)?,
        Command::PrBody {
            body_file: Some(path),
        } => {
            let template = pr_body::read_template(&root)?;
            let body = std::fs::read_to_string(path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            pr_body::check_body(&template, Some(&body))
        }
        Command::Approval => approval::run(&root, ci.as_ref(), &mut out)?,
        Command::Gate { only, skip } => {
            let selection = gate::selection(only, skip)?;
            let verdicts = gate::run(&root, ci.as_ref(), &selection, &mut out)?;
            return Ok(verdicts.iter().all(Verdict::passed));
        }
        Command::Pr {
            title,
            body_file,
            branch,
        } => {
            let inputs = gate::PrInputs {
                title: title.clone(),
                body_file: body_file.clone(),
                branch: branch.clone(),
            };
            let verdicts = gate::run_pr(&root, ci.as_ref(), &inputs, &mut out)?;
            return Ok(verdicts.iter().all(Verdict::passed));
        }
        Command::Github {
            repo,
            action: GithubAction::Check,
        } => settings::run_check(&root, repo.as_deref())?,
        Command::Github {
            repo,
            action: GithubAction::Apply,
        } => settings::run_apply(&root, repo.as_deref())?,
    };
    report::emit(&verdict, ci.is_some(), &mut out)?;
    Ok(verdict.passed())
}

/// The workspace root: the parent of the directory this xtask was built from.
/// The verdict of `tools`: every pin, or with `--for` only the tools of those gate parts.
fn tools_verdict(
    root: &Path,
    ci: Option<&ci::CiEvent>,
    install: bool,
    parts: &[String],
    out: &mut dyn Write,
) -> Result<Verdict, Error> {
    if install {
        tools::install_missing(root, out)?;
    }
    if parts.is_empty() {
        return tools::run(root, ci, out);
    }
    tools::run_for(root, &part_checks(parts)?, out)
}

/// Prints the spec line of the cargo tools to install, for the parts or (without any) for all.
/// Only that line goes to stdout: the setup action captures it.
fn print_install_spec(root: &Path, parts: &[String], out: &mut dyn Write) -> anyhow::Result<bool> {
    let pins = tools::load_pins(root)?;
    let spec = if parts.is_empty() {
        tools::install_spec(&pins)
    } else {
        tools::install_spec_for(&pins, &part_checks(parts)?)
    };
    writeln!(out, "{spec}")?;
    Ok(true)
}

/// Prints the pinned version of one tool and nothing else: the workflows capture it.
fn print_pin(root: &Path, name: &str, out: &mut dyn Write) -> anyhow::Result<bool> {
    let pins = tools::load_pins(root)?;
    writeln!(out, "{}", tools::pin_version(&pins, name)?)?;
    Ok(true)
}

/// The gate parts that `--for` names; a name that is not a part is a usage error.
fn part_checks(parts: &[String]) -> Result<Vec<Check>, Error> {
    match gate::selection(parts, &[])? {
        gate::Selection::Only(checks) => Ok(checks),
        gate::Selection::All | gate::Selection::Skip(_) => Err(Error::Usage {
            message: "`--for` needs the names of gate parts".to_owned(),
        }),
    }
}

fn default_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// Logs to stderr, filtered by `XTASK_LOG` (default: warnings and errors).
fn init_logging() {
    let filter = EnvFilter::builder()
        .with_env_var("XTASK_LOG")
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    // A subscriber that is already installed is as good as ours.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .without_time()
        .try_init();
}
