//! The shared lookup tables of `check`, built once from a [`PlanningRepo`]: every initiative,
//! epic and story with its place in the tree, and every `key` with all the items that carry it.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{AfterRef, Key, TicketId};
use crate::repo::{
    Component, Epic, EpicPrerequisite, Initiative, PlanningRepo, PrerequisiteSource, Story,
};

/// Whether a keyed item is an epic or a story (spike and bug included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// An epic.
    Epic,
    /// A story, spike or bug.
    Story,
}

/// One item that carries a `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyed {
    /// Epic or story.
    pub kind: ItemKind,
    /// The component folder it sits in.
    pub component: String,
    /// The file that states the key (a story only in `tickets.toml` gives that file).
    pub path: String,
}

/// An initiative with its place.
#[derive(Debug, Clone, Copy)]
pub struct InitiativeAt<'a> {
    /// The initiative.
    pub initiative: &'a Initiative,
    /// Its parent initiative, for a child.
    pub parent: Option<&'a Initiative>,
}

/// An epic with its place.
#[derive(Debug, Clone, Copy)]
pub struct EpicAt<'a> {
    /// The initiative whose `tickets/` holds it.
    pub initiative: &'a Initiative,
    /// The epic.
    pub epic: &'a Epic,
}

/// Where a story sits: in an epic, directly in an initiative, or in `backlog/`.
#[derive(Debug, Clone, Copy)]
pub enum StoryHome<'a> {
    /// In an epic of an initiative.
    Epic(&'a Initiative, &'a Epic),
    /// Directly in an initiative without epics.
    Initiative(&'a Initiative),
    /// In the component's `backlog/` (R-q).
    Backlog,
}

/// A story with its place.
#[derive(Debug, Clone, Copy)]
pub struct StoryAt<'a> {
    /// Its component.
    pub component: &'a Component,
    /// Where it sits.
    pub home: StoryHome<'a>,
    /// The story.
    pub story: &'a Story,
}

impl<'a> StoryAt<'a> {
    /// The stories of the same folder, this one included.
    #[must_use]
    pub fn mates(&self) -> &'a [Story] {
        match self.home {
            StoryHome::Epic(_, epic) => &epic.stories,
            StoryHome::Initiative(initiative) => &initiative.stories,
            StoryHome::Backlog => &self.component.backlog,
        }
    }

    /// The initiative whose epics its `after` may name; none for a backlog story.
    #[must_use]
    pub fn initiative(&self) -> Option<&'a Initiative> {
        match self.home {
            StoryHome::Epic(initiative, _) | StoryHome::Initiative(initiative) => Some(initiative),
            StoryHome::Backlog => None,
        }
    }
}

/// What every rule looks things up in.
#[derive(Debug, Clone)]
pub struct Index<'a> {
    /// Every initiative, parents before children.
    pub initiatives: Vec<InitiativeAt<'a>>,
    /// Every epic, in build order per initiative.
    pub epics: Vec<EpicAt<'a>>,
    /// Every story, spike and bug, each once (a file and its entry are one story, R-o).
    pub stories: Vec<StoryAt<'a>>,
    /// Every `key` with all items that carry it, in tree order; more than one is a duplicate.
    pub keys: BTreeMap<&'a Key, Vec<Keyed>>,
}

impl<'a> Index<'a> {
    /// Builds the index of `repo`.
    #[must_use]
    pub fn build(repo: &'a PlanningRepo) -> Index<'a> {
        let mut index = Index {
            initiatives: Vec::new(),
            epics: Vec::new(),
            stories: Vec::new(),
            keys: BTreeMap::new(),
        };
        for component in &repo.components {
            if let Some(initiative) = &component.initiative {
                index.add_initiative(component, initiative, None);
            }
            for story in &component.backlog {
                index.add_story(component, StoryHome::Backlog, story);
            }
        }
        index
    }

    fn add_initiative(
        &mut self,
        component: &'a Component,
        initiative: &'a Initiative,
        parent: Option<&'a Initiative>,
    ) {
        self.initiatives.push(InitiativeAt { initiative, parent });
        for epic in &initiative.epics {
            self.epics.push(EpicAt { initiative, epic });
            if let (Some(key), Some(file)) = (epic.key(), &epic.file) {
                self.add_key(key, ItemKind::Epic, component, file.path.clone());
            }
            for story in &epic.stories {
                self.add_story(component, StoryHome::Epic(initiative, epic), story);
            }
        }
        for story in &initiative.stories {
            self.add_story(component, StoryHome::Initiative(initiative), story);
        }
        for child in &initiative.children {
            self.add_initiative(component, child, Some(initiative));
        }
    }

    fn add_story(&mut self, component: &'a Component, home: StoryHome<'a>, story: &'a Story) {
        self.stories.push(StoryAt {
            component,
            home,
            story,
        });
        if let Some(key) = story.key() {
            self.add_key(key, ItemKind::Story, component, story.path());
        }
    }

    fn add_key(&mut self, key: &'a Key, kind: ItemKind, component: &Component, path: String) {
        self.keys.entry(key).or_default().push(Keyed {
            kind,
            component: component.id.clone(),
            path,
        });
    }
}

/// The epic of `initiative` whose `[[epic]]` table has `id`.
#[must_use]
pub fn epic_by_id<'a>(initiative: &'a Initiative, id: &TicketId) -> Option<&'a Epic> {
    initiative
        .epics
        .iter()
        .find(|e| e.row.as_ref().is_some_and(|r| &r.id == id))
}

/// The epic of `initiative` whose folder is `slug`.
#[must_use]
pub fn epic_by_slug<'a>(initiative: &'a Initiative, slug: &str) -> Option<&'a Epic> {
    initiative.epics.iter().find(|e| e.slug == slug)
}

/// The story among `stories` whose id is `id`.
#[must_use]
pub fn story_by_id<'a>(stories: &'a [Story], id: &TicketId) -> Option<&'a Story> {
    stories.iter().find(|s| s.id.as_ref() == Some(id))
}

/// The `R<n>` numbers of an initiative's requirement lines; `None` without a readable file.
#[must_use]
pub fn initiative_requirements(initiative: &Initiative) -> Option<BTreeSet<u32>> {
    initiative
        .file
        .as_ref()
        .map(|f| f.requirements.iter().map(|r| r.number).collect())
}

/// The `R<n>` numbers of an epic's requirement lines; `None` without a readable file.
#[must_use]
pub fn epic_requirements(epic: &Epic) -> Option<BTreeSet<u32>> {
    epic.body
        .as_ref()
        .map(|b| b.requirements.iter().map(|r| r.number).collect())
}

/// What an `after` reference points at.
#[derive(Debug, Clone, Copy)]
pub enum AfterTarget<'a> {
    /// An epic.
    Epic(&'a Epic),
    /// A story, spike or bug.
    Story(&'a Story),
}

/// Resolves one `after` prerequisite, the one place the PO's forms are read (`tickets.py`
/// `_resolve`). `mates` are the stories of the same folder for a story (a bare id is a sibling
/// story) and `None` for an epic (a bare id is an epic id of its initiative); `initiative` is
/// the one whose epics it may name, `None` for a backlog story. An id or `<epic>.<entry>` that
/// names nothing is tried as a name too, as `tickets.py` does. `None` when it names nothing.
#[must_use]
pub fn resolve_after<'a>(
    on: &AfterRef,
    mates: Option<&'a [Story]>,
    initiative: Option<&'a Initiative>,
) -> Option<AfterTarget<'a>> {
    match on {
        // A digit-only id is an id and nothing else: `tickets.py` refuses a bare number that
        // names no sibling at once, so a typo never lands on a numeric `tracker_id`.
        AfterRef::Sibling(id) if is_digits(id.as_str()) => match mates {
            Some(mates) => story_by_id(mates, id).map(AfterTarget::Story),
            None => initiative
                .and_then(|i| epic_by_id(i, id))
                .map(AfterTarget::Epic),
        },
        AfterRef::Sibling(id) => match mates {
            Some(mates) => story_by_id(mates, id).map(AfterTarget::Story),
            None => initiative
                .and_then(|i| epic_by_id(i, id))
                .map(AfterTarget::Epic),
        }
        .or_else(|| resolve_name(id.as_str(), mates, initiative)),
        // Once the epic id is known, the entry must be in it (`tickets.py` raises); only an
        // `a.b` whose `a` is no epic id is tried as a name (`story-v2.md`-like texts).
        AfterRef::Cross { epic, entry } => match initiative.and_then(|i| epic_by_id(i, epic)) {
            Some(epic) => story_by_id(&epic.stories, entry).map(AfterTarget::Story),
            None => resolve_name(&on.to_string(), mates, initiative),
        },
        AfterRef::Epic(slug) => initiative
            .and_then(|i| epic_by_slug(i, slug))
            .map(AfterTarget::Epic),
        AfterRef::Name(name) => resolve_name(name, mates, initiative),
    }
}

fn is_digits(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// Resolves one prerequisite of `epic`'s `after()`. A prerequisite from the `[[epic]]` table
/// must name an epic (`tickets.py` `declared_after`); one from the epic's file may also name a
/// story. `None` when it names nothing allowed.
#[must_use]
pub fn resolve_epic_prerequisite<'a>(
    prerequisite: &EpicPrerequisite,
    initiative: &'a Initiative,
) -> Option<AfterTarget<'a>> {
    let target = resolve_after(&prerequisite.on, None, Some(initiative))?;
    match (prerequisite.source, target) {
        (PrerequisiteSource::Table, AfterTarget::Story(_)) => None,
        _ => Some(target),
    }
}

/// A name in `after`: for an epic, an epic folder of its initiative (an `{ epic = <slug> }`
/// table); then the `tracker_id` of an epic or story of the initiative or of a mate
/// (`tickets.py` `ids`); then, for a story, a mate's file name with or without `.md`.
fn resolve_name<'a>(
    name: &str,
    mates: Option<&'a [Story]>,
    initiative: Option<&'a Initiative>,
) -> Option<AfterTarget<'a>> {
    if mates.is_none()
        && let Some(epic) = initiative.and_then(|i| epic_by_slug(i, name))
    {
        return Some(AfterTarget::Epic(epic));
    }
    by_tracker_id(name, mates, initiative).or_else(|| {
        mates
            .into_iter()
            .flatten()
            .find(|s| {
                s.file
                    .as_ref()
                    .is_some_and(|f| f.stem == name || name.strip_suffix(".md") == Some(&f.stem))
            })
            .map(AfterTarget::Story)
    })
}

/// The epic or story whose file carries `tracker_id: <name>`, among the epics and stories of
/// `initiative` and the `mates`.
fn by_tracker_id<'a>(
    name: &str,
    mates: Option<&'a [Story]>,
    initiative: Option<&'a Initiative>,
) -> Option<AfterTarget<'a>> {
    let named = |id: Option<&String>| id.is_some_and(|id| !id.is_empty() && id == name);
    let epics = initiative.map_or(&[][..], |i| i.epics.as_slice());
    if let Some(epic) = epics
        .iter()
        .find(|e| named(e.file.as_ref().and_then(|f| f.front.tracker_id.as_ref())))
    {
        return Some(AfterTarget::Epic(epic));
    }
    let stories = epics
        .iter()
        .flat_map(|e| &e.stories)
        .chain(initiative.into_iter().flat_map(|i| &i.stories))
        .chain(mates.into_iter().flatten());
    stories
        .filter(|s| named(s.file.as_ref().and_then(|f| f.front.tracker_id.as_ref())))
        .map(AfterTarget::Story)
        .next()
}
