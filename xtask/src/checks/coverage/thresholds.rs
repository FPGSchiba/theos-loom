//! The contents of `gate/thresholds.toml`: the floors of the two classes of crate, the largest
//! drop below the baseline, and the class of every workspace member.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{Error, Result};

/// The file with the floors, relative to the repository root.
pub(super) const THRESHOLDS_FILE: &str = "gate/thresholds.toml";

/// What a crate is, which decides its floor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrateClass {
    /// A library crate.
    Library,
    /// A binary crate.
    Binary,
}

impl CrateClass {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Binary => "binary",
        }
    }
}

/// The contents of `gate/thresholds.toml`.
#[derive(Clone, PartialEq, Debug)]
pub struct Thresholds {
    /// The largest drop of a crate below the baseline, in percentage points.
    pub max_drop: f64,
    /// The floor of library crates, in percent.
    pub library: f64,
    /// The floor of binary crates, in percent.
    pub binary: f64,
    /// The class of every workspace member, by crate name.
    pub crates: BTreeMap<String, CrateClass>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThresholdsFile {
    max_drop: f64,
    floors: Floors,
    crates: BTreeMap<String, CrateClass>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Floors {
    library: f64,
    binary: f64,
}

impl Thresholds {
    /// Reads `gate/thresholds.toml` of the repository at `root`.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read, is not the expected TOML (`max_drop`, the table
    /// `[floors]` with `library` and `binary`, and the table `[crates]` that maps names to
    /// `"library"` or `"binary"`, and nothing else), or has a number outside 0 to 100 or no
    /// crate. The message gives the line and nothing of the text.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(THRESHOLDS_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Io { path, source })?;
        Self::parse(&text)
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        let file: ThresholdsFile = toml::from_str(text).map_err(|error| {
            let line = error.span().map(|span| {
                let before = text.get(..span.start).unwrap_or(text);
                before.matches('\n').count() + 1
            });
            let shape = "it has `max_drop`, the table `[floors]` with `library` and `binary`, \
                         and the table `[crates]` that maps names to \"library\" or \"binary\", \
                         and nothing else";
            config_error(match line {
                Some(line) => format!("not the expected TOML, near line {line}: {shape}"),
                None => format!("not the expected TOML: {shape}"),
            })
        })?;
        for (name, value) in [
            ("max_drop", file.max_drop),
            ("floors.library", file.floors.library),
            ("floors.binary", file.floors.binary),
        ] {
            if !(0.0..=100.0).contains(&value) {
                return Err(config_error(format!(
                    "`{name}` must be a number from 0 to 100"
                )));
            }
        }
        if file.crates.is_empty() {
            return Err(config_error(
                "`[crates]` is empty, so no member would have a class".to_owned(),
            ));
        }
        Ok(Self {
            max_drop: file.max_drop,
            library: file.floors.library,
            binary: file.floors.binary,
            crates: file.crates,
        })
    }

    /// The floor of a class, in percent.
    #[must_use]
    pub const fn floor(&self, class: CrateClass) -> f64 {
        match class {
            CrateClass::Library => self.library,
            CrateClass::Binary => self.binary,
        }
    }
}

fn config_error(message: String) -> Error {
    Error::Config {
        file: THRESHOLDS_FILE.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testing::TestResult;
    use std::fs;

    const THRESHOLDS: &str = "max_drop = 1.0\n\n[floors]\nlibrary = 80.0\nbinary = 60.0\n\n\
        [crates]\ntheos-content = \"library\"\ntheos = \"binary\"\n";

    #[test]
    fn thresholds_have_the_shape_of_the_file() -> TestResult {
        let thresholds = Thresholds::parse(THRESHOLDS)?;
        assert_eq!(thresholds.max_drop, 1.0);
        assert_eq!(thresholds.floor(CrateClass::Library), 80.0);
        assert_eq!(thresholds.floor(CrateClass::Binary), 60.0);
        assert_eq!(
            thresholds.crates,
            BTreeMap::from([
                ("theos".to_owned(), CrateClass::Binary),
                ("theos-content".to_owned(), CrateClass::Library),
            ])
        );
        Ok(())
    }

    #[test]
    fn thresholds_of_the_repository_load_from_its_root() -> TestResult {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("gate"))?;
        fs::write(dir.path().join("gate").join("thresholds.toml"), THRESHOLDS)?;
        assert_eq!(
            Thresholds::load(dir.path())?,
            Thresholds::parse(THRESHOLDS)?
        );
        let missing = Thresholds::load(&dir.path().join("nowhere"));
        assert!(matches!(missing, Err(Error::Io { .. })), "{missing:?}");
        Ok(())
    }

    #[test]
    fn thresholds_that_are_wrong_are_a_config_error_that_quotes_nothing() {
        let wrong = [
            ("", "an empty file"),
            ("secret-token", "not TOML"),
            (
                &THRESHOLDS.replace("max_drop = 1.0", "max_drop = \"secret-token\""),
                "text",
            ),
            (&THRESHOLDS.replace("max_drop = 1.0\n", ""), "no max_drop"),
            (
                &THRESHOLDS.replace("max_drop = 1.0", "max_drop = -1.0"),
                "negative drop",
            ),
            (
                &THRESHOLDS.replace("max_drop = 1.0", "max_drop = nan"),
                "nan",
            ),
            (
                &THRESHOLDS.replace("library = 80.0", "library = 100.5"),
                "floor over 100",
            ),
            (
                &THRESHOLDS.replace("binary = 60.0", "binary = -0.5"),
                "negative floor",
            ),
            (
                &THRESHOLDS.replace("binary = 60.0\n", ""),
                "no binary floor",
            ),
            (
                &THRESHOLDS.replace("\"binary\"", "\"secret-token\""),
                "unknown class",
            ),
            (
                &format!("{THRESHOLDS}secret-token = 1\n"),
                "unknown crate value",
            ),
            (&format!("secret-token = 1\n{THRESHOLDS}"), "unknown key"),
            (
                &THRESHOLDS.replace("[floors]", "[floors]\nsecret-token = 1.0"),
                "unknown floor",
            ),
            (
                "max_drop = 1.0\n[floors]\nlibrary = 80.0\nbinary = 60.0\n[crates]\n",
                "no crates",
            ),
        ];
        for (text, what) in wrong {
            let result = Thresholds::parse(text);
            let Err(Error::Config { file, message }) = &result else {
                panic!("{what}: {result:?}");
            };
            assert_eq!(file, "gate/thresholds.toml", "{what}");
            assert!(!message.contains("secret-token"), "{what}: {message}");
        }
    }

    #[test]
    fn a_wrong_file_is_reported_near_its_line() {
        let result = Thresholds::parse(&THRESHOLDS.replace("[crates]", "[crates]\nx = 5"));
        let Err(Error::Config { message, .. }) = &result else {
            panic!("{result:?}");
        };
        assert!(message.contains("near line 8"), "{message}");
    }
}
