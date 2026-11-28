//! Value types of the planning repo, checked where they enter.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a value type refused a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// Not a key.
    #[error("`{0}` is not a key: a key is capital letters, a dash and a number, like ED-12")]
    Key(String),
    /// Not a ticket id.
    #[error("`{0}` is not an id: an id is letters and digits only, like 3 or 2b")]
    TicketId(String),
    /// Not an `after` reference.
    #[error(
        "`{0}` is not an `after` reference: use an id like 3, an epic and entry like 2.3, \
         or an epic slug like epic-cart"
    )]
    After(String),
}

fn is_alnum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A project-unique key of an epic or story: `^[A-Z][A-Z0-9]*-[0-9]+$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Key(String);

impl FromStr for Key {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, IdError> {
        let valid = s.split_once('-').is_some_and(|(prefix, number)| {
            prefix.starts_with(|c: char| c.is_ascii_uppercase())
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                && !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
        });
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(IdError::Key(s.to_owned()))
        }
    }
}

impl TryFrom<String> for Key {
    type Error = IdError;

    fn try_from(s: String) -> Result<Self, IdError> {
        s.parse()
    }
}

impl From<Key> for String {
    fn from(key: Key) -> Self {
        key.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for Key {
    fn schema_name() -> Cow<'static, str> {
        "Key".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^[A-Z][A-Z0-9]*-[0-9]+$" })
    }
}

/// A TOML or YAML value that is an integer or a string, as the PO writes ids.
///
/// A float (`1.10` unquoted) is refused with a message asking for quotes: reading it as a number
/// would turn `1.10` into `1.1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketIdOrString {
    /// A bare integer, `id = 6`.
    Int(i64),
    /// A string, `id = "6"`.
    Str(String),
}

impl<'de> Deserialize<'de> for TicketIdOrString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = TicketIdOrString;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer or a string")
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(TicketIdOrString::Int(v))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                i64::try_from(v)
                    .map(TicketIdOrString::Int)
                    .map_err(|_| E::custom(format!("`{v}` is too large for an id")))
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Err(E::custom(format!(
                    "`{v}` is read as a decimal number, which would change a reference like 1.10;                      write it in quotes, like \"{v}\""
                )))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(TicketIdOrString::Str(v.to_owned()))
            }
        }
        deserializer.deserialize_any(V)
    }
}

impl fmt::Display for TicketIdOrString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Str(s) => f.write_str(s),
        }
    }
}

/// A ticket id inside a `tickets.toml`: `^[0-9A-Za-z]+$`. `6` and `"6"` are the same id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "TicketIdOrString", into = "String")]
pub struct TicketId(String);

impl TicketId {
    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TicketId {
    type Error = IdError;

    fn try_from(s: String) -> Result<Self, IdError> {
        if is_alnum(&s) {
            Ok(Self(s))
        } else {
            Err(IdError::TicketId(s))
        }
    }
}

impl TryFrom<TicketIdOrString> for TicketId {
    type Error = IdError;

    fn try_from(v: TicketIdOrString) -> Result<Self, IdError> {
        Self::try_from(v.to_string())
    }
}

impl From<TicketId> for String {
    fn from(id: TicketId) -> Self {
        id.0
    }
}

impl fmt::Display for TicketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for TicketId {
    fn schema_name() -> Cow<'static, str> {
        "TicketId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "integer"], "pattern": "^[0-9A-Za-z]+$" })
    }
}

/// One prerequisite in an `after` list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "TicketIdOrString")]
pub enum AfterRef {
    /// A sibling entry or epic, `3`.
    Sibling(TicketId),
    /// An entry of another epic, `"2.3"`.
    Cross {
        /// The epic's id.
        epic: TicketId,
        /// The entry's id.
        entry: TicketId,
    },
    /// A whole epic by name, `"epic-cart"`.
    Epic(String),
}

impl TryFrom<TicketIdOrString> for AfterRef {
    type Error = IdError;

    fn try_from(v: TicketIdOrString) -> Result<Self, IdError> {
        v.to_string().parse()
    }
}

impl FromStr for AfterRef {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, IdError> {
        let bad = || IdError::After(text.to_owned());
        if let Some(slug) = text.strip_prefix("epic-") {
            return if slug.is_empty() {
                Err(bad())
            } else {
                Ok(Self::Epic(text.to_owned()))
            };
        }
        match text.split_once('.') {
            None => TicketId::try_from(text.to_owned())
                .map(Self::Sibling)
                .map_err(|_| bad()),
            Some((epic, entry)) => {
                let epic = TicketId::try_from(epic.to_owned()).map_err(|_| bad())?;
                let entry = TicketId::try_from(entry.to_owned()).map_err(|_| bad())?;
                Ok(Self::Cross { epic, entry })
            }
        }
    }
}

/// Reads a YAML `after` list as the scalars' source text, so an unquoted `1.10` (which
/// `tickets.py pull` writes) stays `1.10` and is never read as the float `1.1`.
pub(crate) fn yaml_after_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<AfterRef>, D::Error> {
    struct Text(String);
    impl<'de> Deserialize<'de> for Text {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct V;
            impl serde::de::Visitor<'_> for V {
                type Value = Text;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("an id, an `<epic>.<id>` pair or an epic slug")
                }

                fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Text, E> {
                    Ok(Text(v.to_owned()))
                }
            }
            deserializer.deserialize_str(V)
        }
    }
    Vec::<Text>::deserialize(deserializer)?
        .into_iter()
        .map(|t| t.0.parse().map_err(serde::de::Error::custom))
        .collect()
}

impl fmt::Display for AfterRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sibling(id) => write!(f, "{id}"),
            Self::Cross { epic, entry } => write!(f, "{epic}.{entry}"),
            Self::Epic(name) => f.write_str(name),
        }
    }
}

impl Serialize for AfterRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl JsonSchema for AfterRef {
    fn schema_name() -> Cow<'static, str> {
        "AfterRef".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": ["string", "number"],
            "pattern": "^([0-9A-Za-z]+(\\.[0-9A-Za-z]+)?|epic-.+)$"
        })
    }
}

/// The id of an acceptance criterion: `ED-12.3` for a story, `ED-4.R2` for an epic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CriterionId {
    /// Numbered criterion of a story, spike or bug.
    Story {
        /// The story's key.
        key: Key,
        /// The criterion number.
        number: u32,
    },
    /// Requirement line of an epic.
    Epic {
        /// The epic's key.
        key: Key,
        /// The requirement number.
        number: u32,
    },
}

impl fmt::Display for CriterionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Story { key, number } => write!(f, "{key}.{number}"),
            Self::Epic { key, number } => write!(f, "{key}.R{number}"),
        }
    }
}

/// Whether Loom may work on an item (`loom` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LoomMark {
    /// Loom may pick it up.
    Ready,
    /// Loom leaves it alone.
    Paused,
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(serde::Deserialize)]
    struct Doc {
        id: TicketId,
    }

    #[derive(serde::Deserialize)]
    struct AfterDoc {
        after: AfterRef,
    }

    #[test]
    fn key_accepts_ed_12_form_and_rejects_others() -> TestResult {
        assert_eq!("ED-12".parse::<Key>()?.to_string(), "ED-12");
        for bad in ["ed-12", "ED12", "ED-x", ""] {
            assert!(bad.parse::<Key>().is_err(), "{bad:?} must be rejected");
        }
        Ok(())
    }

    #[test]
    fn ticket_id_reads_int_and_string_alike() -> TestResult {
        let a: Doc = toml::from_str("id = 6")?;
        let b: Doc = toml::from_str("id = \"6\"")?;
        assert_eq!(a.id, b.id);
        assert!(toml::from_str::<Doc>("id = \"a-b\"").is_err());
        Ok(())
    }

    #[test]
    fn after_ref_forms() -> TestResult {
        let sib: AfterDoc = toml::from_str("after = 3")?;
        assert_eq!(
            sib.after,
            AfterRef::Sibling(TicketId::try_from("3".to_string())?)
        );
        let cross: AfterDoc = toml::from_str("after = \"2.3\"")?;
        assert!(matches!(cross.after, AfterRef::Cross { .. }));
        let epic: AfterDoc = toml::from_str("after = \"epic-cart\"")?;
        assert_eq!(epic.after, AfterRef::Epic("epic-cart".to_string()));
        assert!(toml::from_str::<AfterDoc>("after = \"x.y.z\"").is_err());
        assert_eq!(cross.after.to_string(), "2.3");
        Ok(())
    }

    #[test]
    fn criterion_ids_display() -> TestResult {
        let key: Key = "ED-12".parse()?;
        assert_eq!(CriterionId::Story { key, number: 3 }.to_string(), "ED-12.3");
        let key: Key = "ED-4".parse()?;
        assert_eq!(CriterionId::Epic { key, number: 2 }.to_string(), "ED-4.R2");
        Ok(())
    }

    #[test]
    fn loom_mark_reads_ready_and_paused() -> TestResult {
        #[derive(serde::Deserialize)]
        struct M {
            loom: LoomMark,
        }
        assert_eq!(
            toml::from_str::<M>("loom = \"ready\"")?.loom,
            LoomMark::Ready
        );
        assert!(toml::from_str::<M>("loom = \"go\"").is_err());
        Ok(())
    }
}
