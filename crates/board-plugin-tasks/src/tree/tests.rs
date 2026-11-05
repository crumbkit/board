//! The tree that `parent` links make, from nodes as `available` and `tree`
//! read them. These were `test_tree.py`, which pinned the rules this port
//! keeps: which tasks a parent waits on, what its subtasks count, its tree
//! path, and where a loop is cut.

use super::*;

/// Nodes by path from (slug, status, parent slug, title); a title left
/// empty is the slug.
fn nodes(given: &[(&str, &str, Option<&str>, &str)]) -> Nodes {
    (given.iter())
        .map(|(name, status, parent, title)| {
            let title = if title.is_empty() { name } else { title };
            let node = Node {
                title: (*title).to_owned(),
                status: (*status).to_owned(),
                parent: parent.map(at),
            };
            (at(name), node)
        })
        .collect()
}

fn at(name: &str) -> String {
    format!("/tasks/{name}.md")
}

fn paths(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| at(name)).collect()
}

// Ship flows editor (project)
// ├── Flows UX agreed
// ├── Dispatch owns flow documents (task)
// │   └── Flow document store (project)
// │       ├── Choose store
// │       └── Migrate existing flows (done)
// └── Pin lines to revisions (done)
const FLOWS: [(&str, &str, Option<&str>, &str); 7] = [
    ("ship-flows-editor", "open", None, "Ship flows editor"),
    (
        "flows-ux-agreed",
        "open",
        Some("ship-flows-editor"),
        "Flows UX agreed",
    ),
    (
        "dispatch-owns-flow-documents",
        "open",
        Some("ship-flows-editor"),
        "Dispatch owns flow documents",
    ),
    (
        "flow-document-store",
        "open",
        Some("dispatch-owns-flow-documents"),
        "Flow document store",
    ),
    (
        "choose-store",
        "open",
        Some("flow-document-store"),
        "Choose store",
    ),
    (
        "migrate-existing-flows",
        "done",
        Some("flow-document-store"),
        "Migrate existing flows",
    ),
    (
        "pin-lines-to-revisions",
        "done",
        Some("ship-flows-editor"),
        "Pin lines to revisions",
    ),
];

/// FLOWS with the nodes in `changed` in place of those of the same slug.
fn flows_with(changed: &[(&str, &str, Option<&str>, &str)]) -> Nodes {
    let mut found = nodes(&FLOWS);
    found.extend(nodes(changed));
    found
}

#[test]
fn a_project_under_a_task_puts_its_children_among_the_tasks_descendants() {
    let flows = nodes(&FLOWS);
    let tree = forest(&flows);
    assert_eq!(tree.roots, paths(&["ship-flows-editor"]));
    assert_eq!(
        descendants(&tree, &at("dispatch-owns-flow-documents")),
        paths(&[
            "flow-document-store",
            "choose-store",
            "migrate-existing-flows"
        ])
    );
    let counted = |name| subtasks(&tree, &flows, &at(name));
    assert_eq!(
        counted("dispatch-owns-flow-documents"),
        Subtasks { open: 2, total: 3 }
    );
    assert_eq!(counted("ship-flows-editor"), Subtasks { open: 4, total: 6 });
    assert_eq!(counted("choose-store"), Subtasks { open: 0, total: 0 });
}

#[test]
fn children_are_sorted_by_title() {
    let tree = forest(&nodes(&FLOWS));
    assert_eq!(
        tree.children[&at("ship-flows-editor")],
        paths(&[
            "dispatch-owns-flow-documents",
            "flows-ux-agreed",
            "pin-lines-to-revisions"
        ])
    );
}

#[test]
fn the_tree_path_joins_the_slugs_from_the_root() {
    let tree = forest(&nodes(&FLOWS));
    assert_eq!(
        tree_path(&tree, &at("choose-store")),
        "ship-flows-editor/dispatch-owns-flow-documents/flow-document-store/choose-store"
    );
    assert_eq!(
        tree_path(&tree, &at("ship-flows-editor")),
        "ship-flows-editor"
    );
}

#[test]
fn a_parent_waits_on_its_open_children_and_on_the_open_descendants_of_a_finished_child() {
    let flows = nodes(&FLOWS);
    assert_eq!(
        waits_on(&forest(&flows), &flows, &at("ship-flows-editor")),
        paths(&["dispatch-owns-flow-documents", "flows-ux-agreed"])
    );
    let store = (
        "flow-document-store",
        "done",
        Some("dispatch-owns-flow-documents"),
        "Flow document store",
    );
    let early = flows_with(&[store]);
    assert_eq!(
        waits_on(&forest(&early), &early, &at("dispatch-owns-flow-documents")),
        paths(&["choose-store"])
    );
    let dropped = (
        "choose-store",
        "dropped",
        Some("flow-document-store"),
        "Choose store",
    );
    let finished = flows_with(&[store, dropped]);
    assert_eq!(
        waits_on(
            &forest(&finished),
            &finished,
            &at("dispatch-owns-flow-documents")
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_parent_that_names_no_node_makes_a_root() {
    let given = nodes(&[
        ("orphan", "open", Some("gone"), ""),
        ("child", "open", Some("orphan"), ""),
    ]);
    let tree = forest(&given);
    assert_eq!(
        (tree.roots.clone(), tree.cycles.clone()),
        (paths(&["orphan"]), vec![])
    );
    assert_eq!(tree_path(&tree, &at("child")), "orphan/child");
}

#[test]
fn a_loop_is_cut_at_its_first_path_which_becomes_a_root_and_is_listed() {
    let looped = [
        ("b", "open", Some("c"), ""),
        ("c", "open", Some("a"), ""),
        ("a", "open", Some("b"), ""),
        ("d", "open", Some("a"), ""),
    ];
    let given = nodes(&looped);
    let tree = forest(&given);
    assert_eq!(
        (tree.roots.clone(), tree.cycles.clone()),
        (paths(&["a"]), paths(&["a"]))
    );
    assert_eq!(tree_path(&tree, &at("b")), "a/c/b");
    assert_eq!(descendants(&tree, &at("a")), paths(&["c", "b", "d"]));
    assert_eq!(waits_on(&tree, &given, &at("c")), paths(&["b"]));
    let mut unlooped = given.clone();
    unlooped.extend(nodes(&[("a", "open", None, "")]));
    let tree = forest(&unlooped);
    assert_eq!((tree.roots, tree.cycles), (paths(&["a"]), vec![]));
}

#[test]
fn a_task_that_is_its_own_parent_is_a_loop_of_one() {
    let tree = forest(&nodes(&[("self", "open", Some("self"), "")]));
    assert_eq!(
        (
            tree.roots.clone(),
            tree.cycles.clone(),
            descendants(&tree, &at("self"))
        ),
        (paths(&["self"]), paths(&["self"]), vec![])
    );
}
