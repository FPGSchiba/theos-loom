//! The fingerprint of a pull request: what an approval is tied to (`docs/gate.md` §5).
//!
//! It is the SHA-256 of `git diff --raw --no-renames --no-abbrev <merge-base> <head>`: one line per
//! changed path with the mode and the full object id on both sides. So it moves with every byte
//! of every file the pull request changes, with every mode and with every binary, and with
//! nothing else: a rebase onto a newer `main`, or a merge from `main`, leaves the lines of the
//! pull request's own changes as they were. `git patch-id` is not used because it ignores
//! whitespace by default.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::Result;
use crate::git::Git;

/// A fingerprint: 64 lower-case hex digits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fingerprint(pub String);

impl Fingerprint {
    /// The fingerprint written as `hex`, or `None` when it is not 64 lower-case hex digits.
    #[must_use]
    pub fn parse(hex: &str) -> Option<Self> {
        let valid = hex.len() == 64
            && hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        valid.then(|| Self(hex.to_owned()))
    }
}

/// The fingerprint of the changes from `merge_base` to `head`.
///
/// # Errors
///
/// Fails when either revision is unknown.
pub fn compute(git: &Git, merge_base: &str, head: &str) -> Result<Fingerprint> {
    let raw = git.diff_raw(merge_base, head)?;
    let digest = Sha256::digest(raw.as_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    Ok(Fingerprint(hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_64_lower_case_hex_digits() {
        let hex = "0123456789abcdef".repeat(4);
        assert_eq!(Fingerprint::parse(&hex), Some(Fingerprint(hex)));
        assert_eq!(Fingerprint::parse(""), None);
        assert_eq!(Fingerprint::parse(&"a".repeat(63)), None);
        assert_eq!(Fingerprint::parse(&"a".repeat(65)), None);
        assert_eq!(Fingerprint::parse(&"A".repeat(64)), None);
        assert_eq!(Fingerprint::parse(&"g".repeat(64)), None);
        assert_eq!(Fingerprint::parse(&format!("{} ", "a".repeat(63))), None);
    }
}
