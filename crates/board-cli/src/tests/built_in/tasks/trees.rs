//! Projects and tasks as one tree through the binary: `parent` in
//! `available`, and `tree`. These were `test_trees.py`; the rules of the
//! tree itself are the crate's.

use super::*;

// Ship flows editor (project)
// ├── Dispatch owns flow documents
// │   └── Flow document store (project)
// │       ├── Choose store
// │       └── Migrate existing flows (done)
// ├── Flows UX agreed
// └── Pin lines to revisions (done)
fn flows() -> Tasks {
    let tasks = Tasks::new();
    tasks.write("projects/flows/README.md", "# Flows\n");
    let under = |parent: &str| json!({"parent": format!("/tasks/{parent}.md")});
    let files = [
        (
            "ship-flows-editor",
            open(
                "Ship flows editor",
                json!({"kind": "project", "brief": "/projects/flows/README.md"}),
            ),
        ),
        (
            "flows-ux-agreed",
            open("Flows UX agreed", under("ship-flows-editor")),
        ),
        (
            "dispatch-owns-flow-documents",
            open(
                "Dispatch owns flow documents",
                json!({"parent": "/tasks/ship-flows-editor.md", "owner": "lead"}),
            ),
        ),
        (
            "flow-document-store",
            open(
                "Flow document store",
                json!({"kind": "project", "parent": "/tasks/dispatch-owns-flow-documents.md"}),
            ),
        ),
        (
            "choose-store",
            open("Choose store", under("flow-document-store")),
        ),
        (
            "migrate-existing-flows",
            task(
                "Migrate existing flows",
                "done",
                &under("flow-document-store"),
                "",
            ),
        ),
        (
            "pin-lines-to-revisions",
            task(
                "Pin lines to revisions",
                "done",
                &under("ship-flows-editor"),
                "",
            ),
        ),
    ];
    for (name, text) in files {
        tasks.write(&format!("tasks/{name}.md"), &text);
    }
    tasks
}

/// Every task `available` lists, by title. No fixture task has
/// `reviewed`, so each one not available is stale.
fn listed(output: &Value) -> BTreeMap<String, Value> {
    (["available", "stale"].iter())
        .flat_map(|section| output[section]["tasks"].as_array().unwrap().clone())
        .map(|item| (item["title"].as_str().unwrap().to_owned(), item))
        .collect()
}

#[test]
fn a_parent_waits_on_its_open_subtasks_until_each_is_finished() {
    let tasks = flows();
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(shown(&output), ["Choose store", "Flows UX agreed"]);
    let found = listed(&output);
    let waiting: BTreeMap<&str, Value> = (found.iter())
        .filter(|(_, item)| !item["blocked_by"].as_array().unwrap().is_empty())
        .map(|(title, item)| (title.as_str(), item["blocked_by"].clone()))
        .collect();
    let ship = json!([
        "/tasks/dispatch-owns-flow-documents.md",
        "/tasks/flows-ux-agreed.md"
    ]);
    let expected = BTreeMap::from([
        (
            "Dispatch owns flow documents",
            json!(["/tasks/flow-document-store.md"]),
        ),
        ("Flow document store", json!(["/tasks/choose-store.md"])),
        ("Ship flows editor", ship),
    ]);
    assert_eq!(waiting, expected);
    let dispatch = &found["Dispatch owns flow documents"];
    assert_eq!(
        (
            &dispatch["parent"],
            &dispatch["subtasks"],
            &dispatch["tree_path"]
        ),
        (
            &json!("/tasks/ship-flows-editor.md"),
            &json!({"open": 2, "total": 3}),
            &json!("ship-flows-editor/dispatch-owns-flow-documents")
        )
    );
    let ship = &found["Ship flows editor"];
    assert_eq!(
        (&ship["parent"], &ship["subtasks"], &ship["tree_path"]),
        (
            &Value::Null,
            &json!({"open": 4, "total": 6}),
            &json!("ship-flows-editor")
        )
    );
    assert_eq!(found["Choose store"]["waiting"], 1);
    let done = task(
        "Choose store",
        "done",
        &json!({"parent": "/tasks/flow-document-store.md"}),
        "",
    );
    tasks.write("tasks/choose-store.md", &done);
    let output = tasks.call("available", &[]).unwrap();
    assert_eq!(shown(&output), ["Flow document store", "Flows UX agreed"]);
}

#[test]
fn a_parent_marked_done_early_still_holds_its_open_subtasks_back_from_its_own_parent() {
    let tasks = flows();
    let parent = json!({"parent": "/tasks/dispatch-owns-flow-documents.md"});
    tasks.write(
        "tasks/flow-document-store.md",
        &task("Flow document store", "done", &parent, ""),
    );
    let found = listed(&tasks.call("available", &[]).unwrap());
    let dispatch = &found["Dispatch owns flow documents"];
    assert_eq!(dispatch["blocked_by"], json!(["/tasks/choose-store.md"]));
}

/// A tree node's fields that most nodes leave as they are, with `fields`.
fn node(fields: Value) -> Value {
    let mut node =
        json!({"brief": null, "kind": "task", "owner": null, "blocked_by": [], "children": []});
    let Value::Object(fields) = fields else {
        panic!("{fields} is an object")
    };
    for (key, value) in fields {
        node[key] = value;
    }
    node
}

#[test]
fn tree_returns_the_task_and_its_descendants_in_every_status_nested_by_title() {
    let tasks = flows();
    let path = "/tasks/dispatch-owns-flow-documents.md";
    let output = tasks.call("tree", &["--path", path]).unwrap();
    let store = "ship-flows-editor/dispatch-owns-flow-documents/flow-document-store";
    let leaf = |slug: &str, title: &str, status: &str| {
        node(json!({
            "path": format!("/tasks/{slug}.md"),
            "title": title,
            "status": status,
            "tree_path": format!("{store}/{slug}"),
            "subtasks": {"open": 0, "total": 0},
        }))
    };
    let tree = node(json!({
        "path": path,
        "title": "Dispatch owns flow documents",
        "status": "open",
        "owner": "lead",
        "tree_path": "ship-flows-editor/dispatch-owns-flow-documents",
        "blocked_by": ["/tasks/flow-document-store.md"],
        "subtasks": {"open": 2, "total": 3},
        "children": [node(json!({
            "path": "/tasks/flow-document-store.md",
            "title": "Flow document store",
            "status": "open",
            "kind": "project",
            "tree_path": store,
            "blocked_by": ["/tasks/choose-store.md"],
            "subtasks": {"open": 1, "total": 2},
            "children": [
                leaf("choose-store", "Choose store", "open"),
                leaf("migrate-existing-flows", "Migrate existing flows", "done"),
            ],
        }))],
    }));
    let expected = json!({
        "tree": tree, "roots": [], "total": 0, "repos": ["/"], "cycles": [], "cross_repo": [],
    });
    assert_eq!(output, expected);
}

#[test]
fn tree_root_lists_every_top_level_node_open_ones_first_with_its_descendants() {
    let tasks = flows();
    tasks.write("tasks/alone.md", &open("Alone", json!({})));
    tasks.write("tasks/old.md", &task("Old", "done", &json!({}), ""));
    let output = tasks.call("tree", &["--root", "true"]).unwrap();
    let roots = output["roots"].as_array().unwrap();
    let shape: Vec<(&str, usize)> = (roots.iter())
        .map(|root| {
            (
                root["title"].as_str().unwrap(),
                root["children"].as_array().unwrap().len(),
            )
        })
        .collect();
    assert_eq!(shape, [("Alone", 0), ("Ship flows editor", 3), ("Old", 0)]);
    assert_eq!(
        (&roots[1]["kind"], &roots[1]["brief"]),
        (&json!("project"), &json!("/projects/flows/README.md"))
    );
    assert_eq!(
        (&output["tree"], &output["total"], &output["repos"]),
        (&Value::Null, &json!(3), &json!(["/"]))
    );
}

#[test]
fn a_title_change_keeps_the_tasks_path_and_its_tree_path() {
    let tasks = flows();
    let flags = [
        "--title",
        "Write the brief",
        "--parent",
        "/tasks/ship-flows-editor.md",
    ];
    let added = tasks.call("add", &flags).unwrap();
    assert_eq!(added["path"], "/tasks/write-the-brief.md");
    let text = tasks.read("tasks/write-the-brief.md");
    let renamed = text.replace("\"Write the brief\"", "\"Draft the flows brief\"");
    tasks.write("tasks/write-the-brief.md", &renamed);
    let output = tasks.call("available", &[]).unwrap();
    let items = output["available"]["tasks"].as_array().unwrap();
    let item = (items.iter())
        .find(|item| item["title"].as_str().unwrap().starts_with("Draft"))
        .unwrap();
    assert_eq!(
        (&item["path"], &item["tree_path"]),
        (
            &json!("/tasks/write-the-brief.md"),
            &json!("ship-flows-editor/write-the-brief")
        )
    );
}

#[test]
fn tree_refuses_a_path_that_holds_no_task_and_admits_a_task_beside_it() {
    let tasks = flows();
    tasks.write("tasks/note.md", "# A note, not a task\n");
    for path in ["/tasks/gone.md", "/tasks/note.md"] {
        let refused = tasks.refused("tree", &["--path", path]);
        assert_eq!(
            (&refused["code"], &refused["fields"]["path"]),
            (&json!("tasks::task::not_found"), &json!(path))
        );
    }
    assert!(
        tasks
            .call("tree", &["--path", "/tasks/choose-store.md"])
            .is_ok()
    );
}

#[test]
fn tree_takes_either_a_path_or_root_and_refuses_both_or_neither() {
    let tasks = Tasks::new();
    tasks.write("tasks/alone.md", &open("Alone", json!({})));
    let cases: [(&[&str], bool); 5] = [
        (&[], false),
        (&["--path", "/tasks/alone.md", "--root", "true"], false),
        (&["--root", "false"], false),
        (&["--path", "/tasks/alone.md"], true),
        (&["--root", "true"], true),
    ];
    for (flags, admitted) in cases {
        match tasks.call("tree", flags) {
            Ok(_) => assert!(admitted, "{flags:?}"),
            Err(refused) => {
                assert!(!admitted, "{flags:?}: {refused}");
                assert_eq!(refused["code"], "tasks::tree::path_or_root");
            }
        }
    }
}

#[test]
fn a_parent_in_another_repo_is_listed_and_not_followed_and_one_in_its_own_repo_is() {
    let tasks = Tasks::new();
    let plan = open("Work plan", json!({}));
    tasks.notes_repo(
        "work",
        Some(json!({"covers": []})),
        &[("plan", &plan)],
        true,
    );
    tasks.write("tasks/plan.md", &open("Plan", json!({})));
    let crossing = json!({"parent": "/repos/work/tasks/plan.md"});
    tasks.write("tasks/crossing.md", &open("Crossing", crossing));
    tasks.write(
        "tasks/inside.md",
        &open("Inside", json!({"parent": "/tasks/plan.md"})),
    );
    let output = tasks.call("available", &[]).unwrap();
    let crossed = json!([{"path": "/tasks/crossing.md", "parent": "/repos/work/tasks/plan.md"}]);
    assert_eq!(output["cross_repo"], crossed);
    let found = listed(&output);
    assert_eq!(
        (
            &found["Crossing"]["parent"],
            &found["Crossing"]["tree_path"]
        ),
        (&Value::Null, &json!("crossing"))
    );
    assert_eq!(
        (&found["Inside"]["parent"], &found["Inside"]["tree_path"]),
        (&json!("/tasks/plan.md"), &json!("plan/inside"))
    );
    let tree = tasks
        .call("tree", &["--root", "true", "--repo", "/"])
        .unwrap();
    assert_eq!(tree["cross_repo"], crossed);
}

#[test]
fn a_parent_in_a_mount_resolves_in_that_mount() {
    let tasks = Tasks::new();
    let plan = open("Plan", json!({}));
    let piece = open("Piece", json!({"parent": "/tasks/plan.md"}));
    let personal = json!({"covers": [], "personal": true});
    tasks.notes_repo(
        "personal",
        Some(personal),
        &[("plan", &plan), ("piece", &piece)],
        true,
    );
    tasks.write(
        "tasks/plan.md",
        &task("The board's plan", "done", &json!({}), ""),
    );
    let found = listed(
        &tasks
            .call("available", &["--repo", "/repos/personal"])
            .unwrap(),
    );
    assert_eq!(
        found["Plan"]["blocked_by"],
        json!(["/repos/personal/tasks/piece.md"])
    );
    assert_eq!(
        (&found["Piece"]["parent"], &found["Piece"]["tree_path"]),
        (
            &json!("/repos/personal/tasks/plan.md"),
            &json!("plan/piece")
        )
    );
    let output = tasks
        .call("tree", &["--path", "/repos/personal/tasks/plan.md"])
        .unwrap();
    let children: Vec<&Value> = (output["tree"]["children"].as_array().unwrap().iter())
        .map(|child| &child["path"])
        .collect();
    assert_eq!(children, [&json!("/repos/personal/tasks/piece.md")]);
    assert_eq!(output["repos"], json!(["/repos/personal"]));
}

#[test]
fn a_parent_loop_is_cut_at_its_first_path_and_listed_in_cycles() {
    let tasks = Tasks::new();
    tasks.write("tasks/a.md", &open("A", json!({"parent": "/tasks/b.md"})));
    tasks.write("tasks/b.md", &open("B", json!({"parent": "/tasks/a.md"})));
    let output = tasks.call("tree", &["--root", "true"]).unwrap();
    let roots: Vec<&Value> = (output["roots"].as_array().unwrap().iter())
        .map(|root| &root["path"])
        .collect();
    assert_eq!(roots, [&json!("/tasks/a.md")]);
    assert_eq!(output["cycles"], json!(["/tasks/a.md"]));
    assert_eq!(shown(&tasks.call("available", &[]).unwrap()), ["B"]);
}
