//! Branch names, as `CONTRIBUTING.md` §1 defines them, and the phase a branch belongs to.
//!
//! `phase/PNN-<slug>` and `spec/PNN-<slug>` carry a phase; `gate/<slug>`, `docs/<slug>` and
//! `fix/<slug>` do not; `release-plz-*` is the release pipeline and `main` is the trunk. The
//! slug is lower-case letters, digits and hyphens, starting with a letter or a digit. Anything
//! else is [`BranchKind::Other`], and a check that needs a known kind says so.

use std::fmt;
use std::str::FromStr;

use crate::Error;

/// A phase of the build, `P00` to `P99`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Phase(u8);

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{:02}", self.0)
    }
}

impl FromStr for Phase {
    type Err = Error;

    /// Accepts exactly `P` and two ASCII digits.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.as_bytes() {
            [b'P', tens @ b'0'..=b'9', ones @ b'0'..=b'9'] => {
                Ok(Self((tens - b'0') * 10 + (ones - b'0')))
            }
            _ => Err(Error::Parse {
                what: "phase".to_owned(),
                message: format!("`{text}` is not `P` followed by two digits"),
            }),
        }
    }
}

/// What a branch name says about the work on it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BranchKind {
    /// `phase/PNN-<slug>`: the build of a phase.
    Phase(Phase),
    /// `spec/PNN-<slug>`: the spec of a phase.
    Spec(Phase),
    /// `gate/<slug>`: a change to the gate.
    Gate,
    /// `docs/<slug>`: a change to the documentation.
    Docs,
    /// `fix/<slug>`: a fix.
    Fix,
    /// `release-plz-*`: a release pull request from the pipeline.
    Release,
    /// `main`.
    Main,
    /// Any other name.
    Other(String),
}

/// Sorts a branch name into its [`BranchKind`].
#[must_use]
pub fn parse_branch(name: &str) -> BranchKind {
    let kind = if name == "main" {
        Some(BranchKind::Main)
    } else if name.starts_with("release-plz-") {
        Some(BranchKind::Release)
    } else if let Some(rest) = name.strip_prefix("phase/") {
        phase_and_slug(rest).map(BranchKind::Phase)
    } else if let Some(rest) = name.strip_prefix("spec/") {
        phase_and_slug(rest).map(BranchKind::Spec)
    } else if let Some(slug) = name.strip_prefix("gate/") {
        is_slug(slug).then_some(BranchKind::Gate)
    } else if let Some(slug) = name.strip_prefix("docs/") {
        is_slug(slug).then_some(BranchKind::Docs)
    } else if let Some(slug) = name.strip_prefix("fix/") {
        is_slug(slug).then_some(BranchKind::Fix)
    } else {
        None
    };
    kind.unwrap_or_else(|| BranchKind::Other(name.to_owned()))
}

/// The phase of a `phase/` branch; `None` for every other kind, `spec/` branches included.
#[must_use]
pub fn phase_of(kind: &BranchKind) -> Option<Phase> {
    match kind {
        BranchKind::Phase(phase) => Some(*phase),
        _ => None,
    }
}

/// The phase in `PNN-<slug>`, the part of a branch name after `phase/` or `spec/`.
fn phase_and_slug(text: &str) -> Option<Phase> {
    let (phase, slug) = text.split_once('-')?;
    if !is_slug(slug) {
        return None;
    }
    phase.parse().ok()
}

/// `[a-z0-9][a-z0-9-]*`.
fn is_slug(text: &str) -> bool {
    let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    text.chars().next().is_some_and(word) && text.chars().all(|c| word(c) || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn branch_kinds() -> TestResult {
        let p02: Phase = "P02".parse()?;
        let p10: Phase = "P10".parse()?;
        assert_eq!(
            parse_branch("phase/P02-records-and-grammars"),
            BranchKind::Phase(p02)
        );
        assert_eq!(parse_branch("spec/P10-prose-editor"), BranchKind::Spec(p10));
        assert_eq!(parse_branch("gate/x"), BranchKind::Gate);
        assert_eq!(parse_branch("docs/x"), BranchKind::Docs);
        assert_eq!(parse_branch("fix/x"), BranchKind::Fix);
        assert_eq!(parse_branch("release-plz-2026-10-31"), BranchKind::Release);
        assert_eq!(parse_branch("main"), BranchKind::Main);
        for other in ["feature/x", "phase/P2-x", "phase/P02"] {
            assert_eq!(
                parse_branch(other),
                BranchKind::Other(other.to_owned()),
                "{other}"
            );
        }
        Ok(())
    }

    #[test]
    fn slugs_are_lower_case_letters_digits_and_hyphens() {
        for bad in [
            "phase/P02-",
            "phase/P02-Records",
            "phase/P02--x",
            "phase/P02-a_b",
            "phase/P100-x",
            "phase/p02-x",
            "spec/P02",
            "gate/",
            "gate/Upper",
            "gate/-x",
            "docs/a/b",
            "fix/with space",
            "Main",
            "origin/main",
            "",
        ] {
            assert_eq!(
                parse_branch(bad),
                BranchKind::Other(bad.to_owned()),
                "{bad:?}"
            );
        }
        for good in ["gate/a", "gate/0x", "docs/a-b-c", "fix/x1-y2"] {
            assert!(
                !matches!(parse_branch(good), BranchKind::Other(_)),
                "{good}"
            );
        }
    }

    #[test]
    fn phase_display_and_parse() -> TestResult {
        let phase: Phase = "P07".parse()?;
        assert_eq!(phase.to_string(), "P07");
        assert_eq!("P00".parse::<Phase>()?.to_string(), "P00");
        assert_eq!("P99".parse::<Phase>()?.to_string(), "P99");
        for bad in [
            "", "P", "P7", "P007", "p07", "07", "PXX", "P0a", " P07", "P07 ", "P٠٧",
        ] {
            assert!(bad.parse::<Phase>().is_err(), "{bad:?}");
        }
        assert!("P01".parse::<Phase>()? < "P02".parse::<Phase>()?);
        Ok(())
    }

    #[test]
    fn only_phase_branches_have_a_phase() -> TestResult {
        let p03: Phase = "P03".parse()?;
        assert_eq!(phase_of(&parse_branch("phase/P03-x")), Some(p03));
        for name in [
            "spec/P03-x",
            "gate/x",
            "docs/x",
            "fix/x",
            "main",
            "release-plz-1",
            "x",
        ] {
            assert_eq!(phase_of(&parse_branch(name)), None, "{name}");
        }
        Ok(())
    }
}
