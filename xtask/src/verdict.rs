//! What a check returns: a verdict made of findings, and the order the gate runs the checks in.
//!
//! A check is a pure core (inputs in, [`Verdict`] out) with the real git, cargo and GitHub wired
//! in elsewhere. A verdict passes exactly when it has no findings.

/// A check of `docs/gate.md` §1. Each one is a `cargo xtask <name>` subcommand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// The tool versions match `gate/tools.toml`.
    Tools,
    /// `cargo fmt --all --check`.
    Fmt,
    /// `cargo clippy` with warnings denied.
    Clippy,
    /// The tests, through nextest, and the doc tests.
    Test,
    /// `cargo doc` with warnings denied.
    Docs,
    /// `cargo deny check`.
    Deny,
    /// Line coverage per crate against floors and the baseline on `main`.
    Coverage,
    /// The lint list, the tool configuration and the attribute scan.
    Lints,
    /// Every acceptance criterion of the phase has a tagged test and every tag names one.
    AcTrace,
    /// Every commit of the PR is a Conventional Commit.
    CommitMsg,
    /// The PR title is a Conventional Commit that matches the branch.
    PrTitle,
    /// The PR description has every section of the template.
    PrBody,
    /// A PR that touches a protected path carries the label `approved`.
    Approval,
    /// The rulesets, settings and labels of the repository on GitHub are those of `gate/`. Not a
    /// check of the gate: it needs GitHub and is run on its own.
    Github,
}

impl Check {
    /// Every check, in the order of the enum.
    pub const ALL: [Check; 14] = [
        Check::Tools,
        Check::Fmt,
        Check::Clippy,
        Check::Test,
        Check::Docs,
        Check::Deny,
        Check::Coverage,
        Check::Lints,
        Check::AcTrace,
        Check::CommitMsg,
        Check::PrTitle,
        Check::PrBody,
        Check::Approval,
        Check::Github,
    ];

    /// The check with this name (see [`Check::name`]), or `None` when there is none.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Check> {
        Self::ALL.into_iter().find(|check| check.name() == name)
    }

    /// The name of the check: its subcommand, its CI job and its status check on GitHub.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Test => "test",
            Self::Docs => "docs",
            Self::Deny => "deny",
            Self::Coverage => "coverage",
            Self::Lints => "lints",
            Self::AcTrace => "ac-trace",
            Self::CommitMsg => "commit-msg",
            Self::PrTitle => "pr-title",
            Self::PrBody => "pr-body",
            Self::Approval => "approval",
            Self::Github => "github",
        }
    }
}

/// The checks `cargo xtask gate` runs, in order: cheap and local ones first. `ac-trace` is last,
/// because it reads the reports of `test`.
pub const GATE_ORDER: [Check; 10] = [
    Check::Tools,
    Check::Fmt,
    Check::Lints,
    Check::CommitMsg,
    Check::Clippy,
    Check::Docs,
    Check::Deny,
    Check::Test,
    Check::Coverage,
    Check::AcTrace,
];

/// The checks that need a pull request, which the gate leaves out.
pub const PR_ONLY: [Check; 3] = [Check::PrTitle, Check::PrBody, Check::Approval];

/// One thing a check found wrong, with where it is when that is known.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Finding {
    /// The file, repo-relative with `/`.
    pub file: Option<String>,
    /// The line in the file, counting from 1.
    pub line: Option<u32>,
    /// What is wrong, and how to fix it.
    pub message: String,
}

impl Finding {
    /// A finding that is not tied to a place.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            file: None,
            line: None,
            message: message.into(),
        }
    }

    /// A finding at `line` of `file`.
    #[must_use]
    pub fn at(file: impl Into<String>, line: u32, message: impl Into<String>) -> Self {
        Self {
            file: Some(file.into()),
            line: Some(line),
            message: message.into(),
        }
    }
}

/// The outcome of one check.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// The check that ran.
    pub check: Check,
    /// What it found wrong; empty when it passed.
    pub findings: Vec<Finding>,
    /// Remarks that do not fail the check, such as a crate with no instrumented lines.
    pub notes: Vec<String>,
    /// Why the check was not run, when it was not needed. Such a verdict passes.
    pub not_needed: Option<String>,
}

impl Verdict {
    /// A verdict with nothing to report.
    #[must_use]
    pub const fn pass(check: Check) -> Self {
        Self {
            check,
            findings: Vec::new(),
            notes: Vec::new(),
            not_needed: None,
        }
    }

    /// A passing verdict for a check that was not run because its result cannot differ, with the
    /// reason. It shows as `skip` in the table and as *not needed* in the summary.
    #[must_use]
    pub fn not_needed(check: Check, reason: impl Into<String>) -> Self {
        Self {
            not_needed: Some(reason.into()),
            ..Self::pass(check)
        }
    }

    /// A verdict with these findings. It passes if the list is empty.
    #[must_use]
    pub const fn with(check: Check, findings: Vec<Finding>) -> Self {
        Self {
            check,
            findings,
            notes: Vec::new(),
            not_needed: None,
        }
    }

    /// True when there are no findings.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// True when the check was not run because it was not needed.
    #[must_use]
    pub const fn is_not_needed(&self) -> bool {
        self.not_needed.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Check; 14] = Check::ALL;

    fn names(checks: &[Check]) -> Vec<&'static str> {
        checks.iter().map(|check| check.name()).collect()
    }

    #[test]
    fn verdict_passed_iff_no_findings() {
        assert!(Verdict::pass(Check::Fmt).passed());
        assert!(Verdict::with(Check::Fmt, Vec::new()).passed());
        assert!(!Verdict::with(Check::Fmt, vec![Finding::new("drifted")]).passed());
    }

    #[test]
    fn notes_do_not_fail_a_verdict() {
        let mut verdict = Verdict::pass(Check::Coverage);
        verdict
            .notes
            .push("theos-vcs has no instrumented lines".to_owned());
        assert!(verdict.passed());
    }

    #[test]
    fn findings_know_where_they_are() {
        let anywhere = Finding::new("broken");
        assert_eq!(anywhere.file, None);
        assert_eq!(anywhere.line, None);
        assert_eq!(anywhere.message, "broken");
        let here = Finding::at("src/lib.rs", 7, "broken");
        assert_eq!(here.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(here.line, Some(7));
        assert_eq!(here.message, "broken");
    }

    #[test]
    fn check_names_are_the_subcommand_names() {
        assert_eq!(
            names(&ALL),
            [
                "tools",
                "fmt",
                "clippy",
                "test",
                "docs",
                "deny",
                "coverage",
                "lints",
                "ac-trace",
                "commit-msg",
                "pr-title",
                "pr-body",
                "approval",
                "github",
            ]
        );
    }

    #[test]
    fn gate_runs_cheap_checks_first_and_leaves_out_the_pr_checks() {
        assert_eq!(
            names(&GATE_ORDER),
            [
                "tools",
                "fmt",
                "lints",
                "commit-msg",
                "clippy",
                "docs",
                "deny",
                "test",
                "coverage",
                "ac-trace",
            ]
        );
        assert_eq!(names(&PR_ONLY), ["pr-title", "pr-body", "approval"]);
        for check in ALL {
            let in_gate = GATE_ORDER.contains(&check);
            let pr_only = PR_ONLY.contains(&check);
            if check == Check::Github {
                // The settings of the repository are compared with `cargo xtask github check`.
                assert!(!in_gate && !pr_only, "github is in a list of the gate");
                continue;
            }
            assert!(
                in_gate != pr_only,
                "{} is in both lists or neither",
                check.name()
            );
        }
    }
}
