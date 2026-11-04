//! The tree that tasks' `parent` links make (plugins/tasks/README.md,
//! Projects and trees). It does no I/O.
//!
//! Each node has at most one parent, so the links make a forest once every
//! loop is cut. A loop is cut at the node with the first path in it, which
//! then has no parent and is listed in `cycles`. A parent that names no
//! node, because it reaches nothing or another repo, leaves its node a
//! root.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::Serialize;

/// A task in the tree: its title, its status, and its parent's board path
/// as resolved, or none. Nodes are kept by their board path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) parent: Option<String>,
}

pub(crate) type Nodes = BTreeMap<String, Node>;

/// The links once loops are cut: each node's parent, its children by
/// title, the roots by path, and the cuts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Forest {
    pub(crate) parents: BTreeMap<String, String>,
    pub(crate) children: BTreeMap<String, Vec<String>>,
    pub(crate) roots: Vec<String>,
    pub(crate) cycles: Vec<String>,
}

/// The forest the nodes' parents make, with each loop cut at its first
/// path.
pub(crate) fn forest(nodes: &Nodes) -> Forest {
    let mut parents: BTreeMap<String, String> = (nodes.iter())
        .filter_map(|(path, node)| {
            let parent = node.parent.as_ref()?;
            nodes
                .contains_key(parent)
                .then(|| (path.clone(), parent.clone()))
        })
        .collect();
    let mut cycles = Vec::new();
    let mut settled = BTreeSet::new();
    for start in nodes.keys() {
        let mut walk: Vec<String> = Vec::new();
        let mut at = Some(start.clone());
        while let Some(here) = at.take() {
            if settled.contains(&here) || walk.contains(&here) {
                at = Some(here);
                break;
            }
            at = parents.get(&here).cloned();
            walk.push(here);
        }
        if let Some(looped) = at.filter(|here| walk.contains(here)) {
            let from = walk.iter().position(|each| *each == looped).unwrap_or(0);
            if let Some(cut) = walk[from..].iter().min().cloned() {
                parents.remove(&cut);
                cycles.push(cut);
            }
        }
        settled.extend(walk);
    }
    let mut ordered: Vec<&String> = parents.keys().collect();
    ordered.sort_by_key(|path| (&nodes[*path].title, *path));
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in ordered {
        (children.entry(parents[path].clone()).or_default()).push(path.clone());
    }
    let roots = (nodes.keys())
        .filter(|path| !parents.contains_key(*path))
        .cloned()
        .collect();
    cycles.sort();
    Forest {
        parents,
        children,
        roots,
        cycles,
    }
}

/// The slug of a task's path: its file name without `.md`.
fn slug(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name)
}

/// The slugs from the node's root to the node, joined by `/`.
pub(crate) fn tree_path(tree: &Forest, path: &str) -> String {
    let mut chain = vec![path];
    while let Some(parent) = chain.last().and_then(|last| tree.parents.get(*last)) {
        chain.push(parent);
    }
    let slugs: Vec<&str> = chain.iter().rev().map(|each| slug(each)).collect();
    slugs.join("/")
}

/// Every node under `path`, depth first, each node's children by title.
pub(crate) fn descendants(tree: &Forest, path: &str) -> Vec<String> {
    let mut found = Vec::new();
    for child in tree.children.get(path).into_iter().flatten() {
        found.push(child.clone());
        found.extend(descendants(tree, child));
    }
    found
}

/// How many of a task's descendants are open, and how many it has in every
/// status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Subtasks {
    pub(crate) open: usize,
    pub(crate) total: usize,
}

pub(crate) fn subtasks(tree: &Forest, nodes: &Nodes, path: &str) -> Subtasks {
    let under = descendants(tree, path);
    let open = (under.iter())
        .filter(|each| nodes[*each].status == "open")
        .count();
    Subtasks {
        open,
        total: under.len(),
    }
}

/// The open descendants a node waits on: each open child, and under a
/// finished child its own open ones.
pub(crate) fn waits_on(tree: &Forest, nodes: &Nodes, path: &str) -> Vec<String> {
    let mut found = Vec::new();
    for child in tree.children.get(path).into_iter().flatten() {
        if nodes[child].status == "open" {
            found.push(child.clone());
        } else {
            found.extend(waits_on(tree, nodes, child));
        }
    }
    found
}

#[cfg(test)]
mod tests;
