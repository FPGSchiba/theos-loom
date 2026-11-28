//! `tickets.toml`: the agreed children of a container.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::value::MapAccessDeserializer;
use serde::de::{IntoDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::ids::{AfterRef, Key, LoomMark, TicketId, TicketIdOrString};
use super::leaf::LeafKind;
use super::ticket_common::{Risk, Scalar};
use crate::finding::Finding;
use crate::parse::toml_file::toml_file;

/// A `tickets.toml`. One kind of table per file; [`Breakdown::kinds`] lets a check say so.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Breakdown {
    /// The folder's id counter.
    pub next_id: Option<u32>,
    /// `[[epic]]` tables (in an initiative's folder).
    #[serde(default)]
    pub epic: Vec<EpicRow>,
    /// `[[entry]]` tables (in an epic's folder).
    #[serde(default)]
    pub entry: Vec<EntryRow>,
    /// `[[initiative]]` tables (in a parent initiative's folder).
    #[serde(default)]
    pub initiative: Vec<InitiativeRow>,
}

/// An epic waits on an epic: a bare reference or an `{ epic, needs }` table.
#[derive(Debug, Clone, PartialEq)]
pub enum EpicAfter {
    /// An id, `2`, or a slug, `"epic-cart"`.
    Ref(AfterRef),
    /// The epic and what it needs from it.
    Needs(EpicNeeds),
}

/// `{ epic = 1, needs = "the pricing contract" }`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpicNeeds {
    /// The epic waited on.
    pub epic: TicketId,
    /// What is needed from it.
    pub needs: Option<String>,
}

/// An initiative waits on an initiative: a bare id or an `{ initiative, needs }` table.
#[derive(Debug, Clone, PartialEq)]
pub enum InitiativeAfter {
    /// An id of a sibling initiative.
    Ref(TicketId),
    /// The initiative and what is needed from it.
    Needs(InitiativeNeeds),
}

/// `{ initiative = 1, needs = "the order-paid event" }`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitiativeNeeds {
    /// The initiative waited on.
    pub initiative: TicketId,
    /// What is needed from it.
    pub needs: Option<String>,
}

/// One `[[epic]]` table.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpicRow {
    /// Permanent id.
    pub id: TicketId,
    /// The epic's folder name.
    pub slug: String,
    /// The outcome.
    pub title: String,
    /// What is true when done.
    pub description: Option<String>,
    /// Parent requirement ids.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Epics it waits on.
    #[serde(default)]
    pub after: Vec<EpicAfter>,
}

/// One `[[entry]]` table: a planned story, spike or bug.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryRow {
    /// Permanent id; the leaf file's `id`.
    pub id: TicketId,
    /// Story, spike or bug.
    #[serde(rename = "type")]
    pub kind: LeafKind,
    /// What works when done.
    pub title: String,
    /// One sentence.
    pub description: Option<String>,
    /// How it will be checked.
    pub verify: Option<String>,
    /// Epic requirement ids.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Prerequisites.
    #[serde(default)]
    pub after: Vec<AfterRef>,
    /// True when a person does part of the work.
    pub hitl: Option<bool>,
    /// Risk.
    pub risk: Option<Risk>,
    /// Estimate, as the user gives it.
    pub estimate: Option<Scalar>,
    /// A question that blocks the entry until removed.
    pub unknown: Option<String>,
    /// References beyond the epic's.
    #[serde(default)]
    pub references: Vec<String>,
    /// The user's words, copied to Notes.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Refine to full criteria first.
    pub refine: Option<bool>,
    /// A person reviews the plan first.
    pub plan_checkpoint: Option<bool>,
    /// Pause after it until a person says continue.
    pub done_checkpoint: Option<bool>,
    /// (Loom) project-unique key.
    pub key: Option<Key>,
    /// (Loom) whether Loom may work on it.
    pub loom: Option<LoomMark>,
    /// (Loom) cross-component prerequisites.
    #[serde(default)]
    pub needs: Vec<Key>,
}

/// One `[[initiative]]` table: a child initiative of a parent.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitiativeRow {
    /// Permanent id.
    pub id: TicketId,
    /// The child's folder name.
    pub slug: String,
    /// The outcome.
    pub title: String,
    /// What is true when done.
    pub description: Option<String>,
    /// Parent requirement ids.
    #[serde(default)]
    pub covers: Vec<Scalar>,
    /// Initiatives it waits on.
    #[serde(default)]
    pub after: Vec<InitiativeAfter>,
}

/// What an `after` item was: a plain reference or a table.
enum RefOrTable<R, N> {
    Ref(R),
    Table(N),
}

/// Reads an int or string as `R` and a table as `N`. Not an untagged enum, so an unknown key in
/// the table keeps serde's own error and is reported as `unknown-key`.
struct RefOrTableVisitor<R, N>(PhantomData<(R, N)>);

impl<R, N> RefOrTableVisitor<R, N>
where
    R: TryFrom<TicketIdOrString>,
    R::Error: fmt::Display,
{
    fn reference<E: serde::de::Error>(
        value: Result<TicketIdOrString, E>,
    ) -> Result<RefOrTable<R, N>, E> {
        R::try_from(value?).map(RefOrTable::Ref).map_err(E::custom)
    }
}

impl<'de, R, N> Visitor<'de> for RefOrTableVisitor<R, N>
where
    R: TryFrom<TicketIdOrString>,
    R::Error: fmt::Display,
    N: Deserialize<'de>,
{
    type Value = RefOrTable<R, N>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an id, a reference or a table with `needs`")
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Self::reference(TicketIdOrString::deserialize(v.into_deserializer()))
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Self::reference(TicketIdOrString::deserialize(v.into_deserializer()))
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Self::reference(TicketIdOrString::deserialize(v.into_deserializer()))
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Self::reference(TicketIdOrString::deserialize(v.into_deserializer()))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        N::deserialize(MapAccessDeserializer::new(map)).map(RefOrTable::Table)
    }
}

fn ref_or_table<'de, D, R, N>(deserializer: D) -> Result<RefOrTable<R, N>, D::Error>
where
    D: Deserializer<'de>,
    R: TryFrom<TicketIdOrString>,
    R::Error: fmt::Display,
    N: Deserialize<'de>,
{
    deserializer.deserialize_any(RefOrTableVisitor(PhantomData))
}

impl<'de> Deserialize<'de> for EpicAfter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ref_or_table(deserializer)? {
            RefOrTable::Ref(r) => Self::Ref(r),
            RefOrTable::Table(n) => Self::Needs(n),
        })
    }
}

impl<'de> Deserialize<'de> for InitiativeAfter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ref_or_table(deserializer)? {
            RefOrTable::Ref(r) => Self::Ref(r),
            RefOrTable::Table(n) => Self::Needs(n),
        })
    }
}

impl JsonSchema for EpicAfter {
    fn schema_name() -> Cow<'static, str> {
        "EpicAfter".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let reference = generator.subschema_for::<AfterRef>();
        let table = generator.subschema_for::<EpicNeeds>();
        json_schema!({ "anyOf": [reference, table] })
    }
}

impl JsonSchema for InitiativeAfter {
    fn schema_name() -> Cow<'static, str> {
        "InitiativeAfter".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let reference = generator.subschema_for::<TicketId>();
        let table = generator.subschema_for::<InitiativeNeeds>();
        json_schema!({ "anyOf": [reference, table] })
    }
}

impl Breakdown {
    /// Parses the `tickets.toml` at `path` from its `text`.
    ///
    /// # Errors
    /// A `parse` or `unknown-key` [`Finding`].
    pub fn parse(path: &str, text: &str) -> Result<Breakdown, Finding> {
        toml_file(path, text)
    }

    /// How many kinds of table the file holds (it should be at most one).
    #[must_use]
    pub fn kinds(&self) -> usize {
        usize::from(!self.epic.is_empty())
            + usize::from(!self.entry.is_empty())
            + usize::from(!self.initiative.is_empty())
    }
}
