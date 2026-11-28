//! The shared lookup tables of `check`, built once from a [`PlanningRepo`]: every initiative,
//! epic and story with its place in the tree, and every `key` with all the items that carry it.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Key, TicketId};
use crate::repo::{Component, Epic, Initiative, PlanningRepo, Story};

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
