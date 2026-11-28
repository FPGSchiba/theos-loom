//! A story, spike or bug: its `tickets.toml` entry, its file and its plan, joined.

use crate::model::{
    AfterRef, CriterionId, EntryRow, Key, LeafFront, LeafKind, LoomMark, PlanFront, PlanStatus,
    TicketId,
};
use crate::parse::body::Criterion;

/// A leaf of the tree, by file, by entry, or both. The accessors apply R-o: once the file
/// exists, its values count.
#[derive(Debug, Clone)]
pub struct Story {
    /// Story, spike or bug (the file's `type` when there is a file).
    pub kind: LeafKind,
    /// The entry id: the entry's, else the file's `id`.
    pub id: Option<TicketId>,
    /// Its `[[entry]]` table, when listed.
    pub entry: Option<EntryRow>,
    /// Its pulled file, when present and readable.
    pub file: Option<LeafFile>,
    /// Its plan file, joined through `ticket`.
    pub plan: Option<PlanFile>,
    /// Position in build order within its folder, from 0: table order, then unlisted files by
    /// id, then files without id.
    pub order: usize,
    /// Repo-relative path of the folder its entry, file and plan sit in.
    pub folder: String,
}

/// A pulled leaf file.
#[derive(Debug, Clone)]
pub struct LeafFile {
    /// Repo-relative path.
    pub path: String,
    /// The file name without `.md`.
    pub stem: String,
    /// Its front matter.
    pub front: LeafFront,
    /// Its numbered criteria (Requirements for a story, Acceptance Criteria otherwise).
    pub criteria: Vec<Criterion>,
}

/// A plan file `<type>-<slug>-plan.md`.
#[derive(Debug, Clone)]
pub struct PlanFile {
    /// Repo-relative path.
    pub path: String,
    /// Its front matter.
    pub front: PlanFront,
}

impl Story {
    /// The path findings about this story name: its file, else its folder's `tickets.toml`.
    #[must_use]
    pub fn path(&self) -> String {
        self.file.as_ref().map_or_else(
            || format!("{}/tickets.toml", self.folder),
            |f| f.path.clone(),
        )
    }

    /// The title: the file's, else the entry's.
    #[must_use]
    pub fn title(&self) -> &str {
        match (&self.file, &self.entry) {
            (Some(f), _) => &f.front.title,
            (None, Some(e)) => &e.title,
            (None, None) => "",
        }
    }

    /// The `key` (R-o: the file's once it exists).
    #[must_use]
    pub fn key(&self) -> Option<&Key> {
        match (&self.file, &self.entry) {
            (Some(f), _) => f.front.key.as_ref(),
            (None, Some(e)) => e.key.as_ref(),
            (None, None) => None,
        }
    }

    /// The `loom` mark (R-o).
    #[must_use]
    pub fn loom(&self) -> Option<LoomMark> {
        match (&self.file, &self.entry) {
            (Some(f), _) => f.front.loom,
            (None, Some(e)) => e.loom,
            (None, None) => None,
        }
    }

    /// The cross-component prerequisites (R-o).
    #[must_use]
    pub fn needs(&self) -> &[Key] {
        match (&self.file, &self.entry) {
            (Some(f), _) => &f.front.needs,
            (None, Some(e)) => &e.needs,
            (None, None) => &[],
        }
    }

    /// The prerequisites: the file's `after` replaces the entry's, absent reading as empty (the
    /// PO's rule).
    #[must_use]
    pub fn after(&self) -> &[AfterRef] {
        match (&self.file, &self.entry) {
            (Some(f), _) => &f.front.after,
            (None, Some(e)) => &e.after,
            (None, None) => &[],
        }
    }

    /// The numbered criteria; an entry without a file has none.
    #[must_use]
    pub fn criteria(&self) -> &[Criterion] {
        self.file.as_ref().map_or(&[], |f| f.criteria.as_slice())
    }

    /// The criteria ids `<key>.<n>`; empty without a key.
    #[must_use]
    pub fn criterion_ids(&self) -> Vec<CriterionId> {
        let Some(key) = self.key() else {
            return Vec::new();
        };
        self.criteria()
            .iter()
            .map(|c| CriterionId::Story {
                key: key.clone(),
                number: c.number,
            })
            .collect()
    }

    /// True when the plan says `dropped`, or, without a plan, the file's older `status` does.
    #[must_use]
    pub fn is_dropped(&self) -> bool {
        let status = match (&self.plan, &self.file) {
            (Some(p), _) => p.front.status,
            (None, Some(f)) => f.front.status,
            (None, None) => None,
        };
        status == Some(PlanStatus::Dropped)
    }
}
