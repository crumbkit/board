//! How much of Board a read call uses, counted on a real board: one query
//! for the repos and the tasks, and one refs only when a task has an
//! `after` or a `parent`, however many do. The Python plugin counted the
//! `board` runs it started in Board's timing log (`test_blockers.py` and
//! `test_trees.py`); a built-in plugin starts none, so its calls are
//! counted on the host.

use serde_json::json;

use crate::tests::real::{Race, Real};

const FIRST: &str = "---\n$type: tasks/task/v2\ntitle: First\nstatus: open\n---\n";
const FINISHED: &str = "---\n$type: tasks/task/v2\ntitle: Finished\nstatus: done\n---\n";

/// A task waiting on `after`.
fn waiting(after: &[&str]) -> String {
    format!(
        "---\n$type: tasks/task/v2\ntitle: After\nstatus: open\nafter: {}\n---\n",
        json!(after)
    )
}

/// The queries and refs one call of `capability` makes, and its output.
fn counted(
    host: &Real,
    capability: &str,
    input: &serde_json::Value,
) -> (usize, usize, serde_json::Value) {
    let (queries, refs) = (host.queries.get(), host.refs.get());
    let output = match capability {
        "available" => crate::available::available(host, input),
        _ => crate::trees::tree(host, input),
    };
    let output = output.unwrap();
    (host.queries.get() - queries, host.refs.get() - refs, output)
}

#[test]
fn available_runs_one_query_and_one_refs_when_a_task_waits_and_only_the_query_when_none_does() {
    let own = [("tasks/first.md", FIRST), ("tasks/finished.md", FINISHED)];
    let host = Real::new(Race::Alone, &own, &[]);
    let (queries, refs, _) = counted(&host, "available", &json!({"repo": "/"}));
    assert_eq!((queries, refs), (1, 0));
    let after = waiting(&["/tasks/first.md", "/tasks/finished.md", "/tasks/missing.md"]);
    let host = Real::new(
        Race::Alone,
        &[own[0], own[1], ("tasks/after.md", &after)],
        &[],
    );
    let (queries, refs, output) = counted(&host, "available", &json!({"repo": "/"}));
    assert_eq!((queries, refs), (1, 1));
    let stale = output["stale"]["tasks"].as_array().unwrap();
    let after = stale.iter().find(|item| item["title"] == "After").unwrap();
    assert_eq!(
        after["blocked_by"],
        json!(["/tasks/first.md", "/tasks/missing.md"])
    );
}

#[test]
fn available_runs_one_refs_however_many_tasks_wait() {
    for count in [1, 3, 5] {
        let after = waiting(&["/tasks/first.md"]);
        let paths: Vec<String> = (0..count)
            .map(|waiter| format!("tasks/after-{waiter}.md"))
            .collect();
        let mut own: Vec<(&str, &str)> = vec![("tasks/first.md", FIRST)];
        own.extend(paths.iter().map(|path| (path.as_str(), after.as_str())));
        let host = Real::new(Race::Alone, &own, &[]);
        let (queries, refs, _) = counted(&host, "available", &json!({"repo": "/"}));
        assert_eq!((queries, refs), (1, 1), "{count} waiting tasks");
    }
}

#[test]
fn a_tree_with_no_parent_reads_the_tasks_with_one_query_and_no_refs_and_one_with_a_parent_adds_one_refs()
 {
    let host = Real::new(Race::Alone, &[("tasks/first.md", FIRST)], &[]);
    let (queries, refs, _) = counted(&host, "tree", &json!({"path": "/tasks/first.md"}));
    assert_eq!((queries, refs), (1, 0));
    let child =
        "---\n$type: tasks/task/v2\ntitle: Child\nstatus: open\nparent: /tasks/first.md\n---\n";
    let own = [("tasks/first.md", FIRST), ("tasks/child.md", child)];
    let host = Real::new(Race::Alone, &own, &[]);
    let (queries, refs, output) = counted(&host, "tree", &json!({"path": "/tasks/first.md"}));
    assert_eq!((queries, refs), (1, 1));
    assert_eq!(output["tree"]["children"][0]["path"], "/tasks/child.md");
}
