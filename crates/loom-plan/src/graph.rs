//! The dependency graph of epics and stories (spec §5.5), the input of the `cycle` rule and,
//! later, of P08's scheduling.
//!
//! Nodes are epics and stories. An edge `a -> b` reads "`a` waits for `b`". Four kinds of edge:
//! a story waits for each prerequisite in its `after` and its `needs`; an epic waits for each
//! prerequisite in its file's `after`, its table's `after` and its `needs`; an epic waits for each
//! of its stories; a story waits for each prerequisite of its epic. A dropped item is still a node
//! and its edges count (R-r). A reference that names nothing adds no edge (`broken-reference`
//! reports it). Stories outside an epic have no epic edges (R-q).

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use petgraph::algo::tarjan_scc;
use petgraph::graphmap::DiGraphMap;

pub use crate::index::ItemKind;
use crate::index::{AfterTarget, Index, StoryHome, resolve_after, resolve_epic_prerequisite};
use crate::model::Key;
use crate::repo::{Epic, Initiative, PlanningRepo, Story};

/// The name of a node: the item's `key`, or its repo-relative path when it has none.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(String);

impl NodeId {
    /// A node name from its text.
    #[must_use]
    pub fn new(text: &str) -> NodeId {
        NodeId(text.to_owned())
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the graph knows about one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    /// Its name.
    pub id: NodeId,
    /// Epic or story.
    pub kind: ItemKind,
    /// The repo-relative path findings about it name.
    pub path: String,
}

/// Who waits for whom among the epics and stories of a planning repo.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    nodes: Vec<NodeInfo>,
    edges: DiGraphMap<usize, ()>,
}

impl DependencyGraph {
    /// Builds the graph of `repo`.
    #[must_use]
    pub fn of(repo: &PlanningRepo) -> DependencyGraph {
        DependencyGraph::from_index(&Index::build(repo))
    }

    pub(crate) fn from_index(index: &Index<'_>) -> DependencyGraph {
        let mut builder = Builder::default();
        for at in &index.epics {
            builder.add(at.epic, at.epic.key(), ItemKind::Epic, at.epic.path.clone());
        }
        for at in &index.stories {
            builder.add(
                at.story,
                at.story.key(),
                ItemKind::Story,
                story_path(at.story),
            );
        }
        for at in &index.epics {
            let me = builder.node_of(at.epic);
            builder.after_epic(me, at.epic, at.initiative);
            builder.needs(me, at.epic.needs());
            for story in &at.epic.stories {
                let to = builder.node_of(story);
                builder.edge(me, to);
            }
        }
        for at in &index.stories {
            let me = builder.node_of(at.story);
            for on in at.story.after() {
                let target = resolve_after(on, Some(at.mates()), at.initiative());
                builder.edge_to(me, target);
            }
            builder.needs(me, at.story.needs());
            if let StoryHome::Epic(initiative, epic) = at.home {
                builder.after_epic(me, epic, initiative);
            }
        }
        DependencyGraph {
            nodes: builder.nodes,
            edges: builder.edges,
        }
    }

    /// Every node, epics first, each group in tree order.
    #[must_use]
    pub fn nodes(&self) -> &[NodeInfo] {
        &self.nodes
    }

    /// What the node `id` waits for directly, sorted; empty for an unknown node. A name carried
    /// by several items (a duplicate key) gives the union. A dropped prerequisite is listed like
    /// any other (R-r).
    #[must_use]
    pub fn waits_for(&self, id: &NodeId) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = self
            .indices_of(id)
            .flat_map(|n| self.edges.neighbors(n))
            .map(|n| self.nodes[n].id.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The path findings about the node `id` name.
    #[must_use]
    pub fn path_of(&self, id: &NodeId) -> Option<&str> {
        self.indices_of(id)
            .next()
            .map(|n| self.nodes[n].path.as_str())
    }

    /// Every cycle: each strongly connected component of more than one node, and each node that
    /// waits for itself. Members are sorted by name and so are the cycles, so the output is the
    /// same on every run.
    #[must_use]
    pub fn cycles(&self) -> Vec<Vec<NodeId>> {
        let mut out: Vec<Vec<NodeId>> = tarjan_scc(&self.edges)
            .into_iter()
            .filter(|scc| scc.len() > 1 || scc.iter().any(|&n| self.edges.contains_edge(n, n)))
            .map(|scc| {
                let mut members: Vec<NodeId> =
                    scc.into_iter().map(|n| self.nodes[n].id.clone()).collect();
                members.sort();
                members
            })
            .collect();
        out.sort();
        out
    }

    fn indices_of<'a>(&'a self, id: &'a NodeId) -> impl Iterator<Item = usize> + 'a {
        self.nodes
            .iter()
            .enumerate()
            .filter(move |(_, node)| &node.id == id)
            .map(|(n, _)| n)
    }
}

/// The path a story's node carries: its file, else its entry's `tickets.toml` and id.
fn story_path(story: &Story) -> String {
    match (&story.file, &story.id) {
        (Some(file), _) => file.path.clone(),
        (None, Some(id)) => format!("{}/tickets.toml#{id}", story.folder),
        (None, None) => story.path(),
    }
}

/// The graph under construction: items are found again by address.
#[derive(Default)]
struct Builder {
    nodes: Vec<NodeInfo>,
    edges: DiGraphMap<usize, ()>,
    by_address: HashMap<usize, usize>,
    by_key: BTreeMap<Key, Vec<usize>>,
}

impl Builder {
    fn add<T>(&mut self, item: &T, key: Option<&Key>, kind: ItemKind, path: String) {
        let n = self.nodes.len();
        let id = key.map_or_else(|| NodeId(path.clone()), |k| NodeId(k.to_string()));
        self.nodes.push(NodeInfo { id, kind, path });
        self.edges.add_node(n);
        self.by_address.insert(std::ptr::from_ref(item) as usize, n);
        if let Some(key) = key {
            self.by_key.entry(key.clone()).or_default().push(n);
        }
    }

    /// The node of an item the index listed.
    fn node_of<T>(&self, item: &T) -> usize {
        self.by_address
            .get(&(std::ptr::from_ref(item) as usize))
            .copied()
            .unwrap_or_default()
    }

    fn edge(&mut self, from: usize, to: usize) {
        self.edges.add_edge(from, to, ());
    }

    fn edge_to(&mut self, from: usize, target: Option<AfterTarget<'_>>) {
        let to = match target {
            Some(AfterTarget::Epic(epic)) => self.node_of(epic),
            Some(AfterTarget::Story(story)) => self.node_of(story),
            None => return,
        };
        self.edge(from, to);
    }

    /// An edge from `from` to each prerequisite of `epic`.
    fn after_epic(&mut self, from: usize, epic: &Epic, initiative: &Initiative) {
        for prerequisite in epic.after() {
            let target = resolve_epic_prerequisite(&prerequisite, initiative);
            self.edge_to(from, target);
        }
    }

    /// An edge from `from` to every item carrying one of `keys`.
    fn needs(&mut self, from: usize, keys: &[Key]) {
        for key in keys {
            let targets = self.by_key.get(key).cloned().unwrap_or_default();
            for to in targets {
                self.edge(from, to);
            }
        }
    }
}
