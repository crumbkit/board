//! `tree`: one task and its descendants, or every top-level tree in scope
//! (plugins/tasks/README.md, Projects and trees).

use std::collections::BTreeMap;

use board_ops::{Host, PluginError};
use board_plugin::shape::Actor;
use board_repos::scope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::available::SHOWN;
use crate::listing::{Crossing, Links, Task, grown, held_by, listing, read, resolve, waited};
use crate::tree::{Forest, Nodes, Subtasks, subtasks, tree_path};
use crate::{PLUGIN, chosen_by, refusal};

/// The tree to list: one task's, or every top-level node's in the repos
/// in scope.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TreeInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = r"^/.*\.md$"),
        description = "The task whose tree to list, such as /tasks/ship-flows-editor.md."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Whether to list every top-level node of the repos in scope, with its \
                       descendants."
    )]
    pub(crate) root: Option<bool>,
    #[schemars(
        regex(pattern = r"^/"),
        description = "One repo's tasks: / for the board's own tree, or a notes repo's mount path \
                       such as /repos/notes-personal; the caller's git remote chooses the repos \
                       when left out."
    )]
    pub(crate) repo: Option<String>,
    #[schemars(
        length(min = 1),
        description = "A git remote, or owner/name, to choose the repos by in place of the origin \
                       of the directory board was started in."
    )]
    pub(crate) remote: Option<String>,
}

/// A task in a tree, with its children by title.
///
/// `blocked_by` holds its open `after` targets, then its open descendants
/// nearest to it.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct TreeNode {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) kind: String,
    pub(crate) brief: Option<String>,
    pub(crate) owner: Option<Actor>,
    pub(crate) blocked_by: Vec<String>,
    pub(crate) subtasks: Subtasks,
    pub(crate) tree_path: String,
    pub(crate) children: Vec<TreeNode>,
}

/// A tree, or the top-level trees.
///
/// With `--path`, `tree` is that task's tree. With `--root`, `roots` holds
/// at most 100 top-level nodes, open ones first and then by title, and
/// `total` counts them all. `repos` are the repos read, `cycles` the tasks
/// in them where a loop of `parent` links was cut, and `cross_repo` the
/// references in them to another repo.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "TreeOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) tree: Option<TreeNode>,
    pub(crate) roots: Vec<TreeNode>,
    pub(crate) total: usize,
    pub(crate) repos: Vec<String>,
    pub(crate) cycles: Vec<String>,
    pub(crate) cross_repo: Vec<Crossing>,
}

/// Lists the tree of `--path`, or with `--root` every top-level node's,
/// read at the repos' pins.
pub(crate) fn tree(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "tree", input)?;
    let root = given.root == Some(true);
    let path = given.path.as_deref();
    if path.is_none() != root {
        let why = "give either --path <task> for one task's tree or --root true for every \
                   top-level tree, not both"
            .to_owned();
        return Err(refusal(
            "tasks::tree::path_or_root",
            why,
            [("path", json!(path)), ("root", json!(given.root))],
        ));
    }
    let (found, rows) = listing(host)?;
    let roots = if let Some(path) = path {
        vec![held_by(&found, path)]
    } else {
        let (name, remote) = (given.repo.as_deref(), given.remote.as_deref());
        let (remote, unread) = chosen_by(host, name, remote);
        scope(PLUGIN, &found, name, remote.as_deref(), unread.as_deref())?
    };
    let (tasks, _) = read(&rows, &roots, host.settings().utc_offset);
    let links = resolve(host, &tasks)?;
    let (nodes, forest) = grown(&tasks, &links);
    let by_path: BTreeMap<&str, &Task> = (tasks.iter())
        .map(|task| (task.path.as_str(), task))
        .collect();
    if let Some(path) = path
        && !by_path.contains_key(path)
    {
        let why =
            format!("no task is at {path}; list the trees with `board tasks tree --root true`");
        return Err(refusal(
            "tasks::task::not_found",
            why,
            [("path", json!(path))],
        ));
    }
    let mut shown: Vec<&str> = if path.is_some() {
        Vec::new()
    } else {
        forest.roots.iter().map(String::as_str).collect()
    };
    shown.sort_by_key(|each| {
        let task = by_path[each];
        (
            task.status != "open",
            task.title.as_str(),
            task.path.as_str(),
        )
    });
    let grove = Grove {
        by_path: &by_path,
        links: &links,
        forest: &forest,
        nodes: &nodes,
    };
    Ok(json!(Output {
        tree: path.map(|path| grove.node(path)),
        roots: shown
            .iter()
            .take(SHOWN)
            .map(|each| grove.node(each))
            .collect(),
        total: shown.len(),
        repos: roots,
        cycles: forest.cycles.clone(),
        cross_repo: links.crossing.clone(),
    }))
}

/// What a tree's nodes are read from.
struct Grove<'a> {
    by_path: &'a BTreeMap<&'a str, &'a Task>,
    links: &'a Links,
    forest: &'a Forest,
    nodes: &'a Nodes,
}

impl Grove<'_> {
    /// The node at `at`, with its children by title.
    fn node(&self, at: &str) -> TreeNode {
        let task = self.by_path[at];
        let children = self.forest.children.get(at).map_or(&[][..], Vec::as_slice);
        TreeNode {
            path: at.to_owned(),
            title: task.title.clone(),
            status: task.status.clone(),
            kind: task.kind.clone(),
            brief: task.brief.clone(),
            owner: task.owner.clone(),
            blocked_by: waited(self.links, self.forest, self.nodes, at),
            subtasks: subtasks(self.forest, self.nodes, at),
            tree_path: tree_path(self.forest, at),
            children: children.iter().map(|child| self.node(child)).collect(),
        }
    }
}
