//! Links in Markdown files are references: `refs` lists them both ways,
//! `check` degrades a file with a link to nothing, and a warm index answers
//! as a cold one. put/tests/file.rs holds what `put` reports.

use board_model::{Code, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use super::{board, open, status, task};
use crate::{Absent, RefsReport};

/// A note with a relative link carrying a fragment, an absolute link, a
/// link to a record's file, a fragment alone, links in a code span and in
/// fenced code, an external URL and an autolink.
const NOTE: &str = "\
See [b](b.md#intro), [the project](/projects/p), [a task](../tasks/a.json)
and [the top](#top). Not `[code](/nowhere)`.

```
[fenced](/nowhere)
```

[web](https://example.com/x.md) and <https://example.com/y.md>.
";

/// The board, with the note, the note it links to, which links back, and a
/// task that names the note.
fn noted() -> Scenario {
    let scenario = board();
    scenario
        .write("notes/a.md", Item::File(NOTE))
        .write("notes/b.md", Item::File("Back to [a](./a.md)."))
        .write(
            "tasks/a.json",
            Item::File(&task(r#", "see": "/notes/a.md""#)),
        );
    scenario
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn refs(scenario: &Scenario, text: &str, cache: Option<&std::path::Path>) -> RefsReport {
    let mut answers: Vec<_> = open(scenario)
        .refs(&[path(text)], Absent::Refuse, &[], None, cache)
        .unwrap()
        .collect();
    RefsReport {
        memory: None,
        ..answers.remove(0).unwrap()
    }
}

/// Each link `refs` lists from `report`: as written, its target and status.
fn from(report: &RefsReport) -> Vec<(&str, String, Status)> {
    report
        .from
        .iter()
        .map(|link| (link.pointer.as_str(), link.target.to_string(), link.status))
        .collect()
}

#[test]
fn refs_lists_the_paths_a_notes_links_reach_and_what_reaches_the_note() {
    let scenario = noted();
    let report = refs(&scenario, "/notes/a.md", None);
    assert_eq!(
        from(&report),
        [
            ("b.md#intro", "/notes/b.md".to_owned(), Status::Ok),
            ("/projects/p", "/projects/p".to_owned(), Status::Ok),
            ("../tasks/a.json", "/tasks/a".to_owned(), Status::Ok),
        ]
    );
    let to: Vec<(&str, Vec<&str>)> = report
        .to
        .iter()
        .map(|incoming| {
            let pointers = incoming.pointers.iter().map(String::as_str).collect();
            (incoming.path.as_str(), pointers)
        })
        .collect();
    assert_eq!(
        to,
        [
            ("notes/b.md", vec!["./a.md"]),
            ("tasks/a.json", vec!["/see"])
        ]
    );
    let project = refs(&scenario, "/projects/p", None);
    assert_eq!(project.to[0].path, "notes/a.md");
}

#[test]
fn a_note_with_a_link_to_nothing_is_degraded_and_its_twin_with_the_target_is_not() {
    let scenario = board();
    scenario.write("notes/a.md", Item::File("[gone](gone.md) [p](/projects/p)"));
    assert_eq!(
        status(&scenario, "notes/a.md"),
        Status::Degraded(Code::RefDangling)
    );
    let report = refs(&scenario, "/notes/a.md", None);
    assert_eq!(
        from(&report)[0],
        (
            "gone.md",
            "/notes/gone.md".to_owned(),
            Status::Degraded(Code::RefDangling)
        )
    );
    scenario.write("notes/gone.md", Item::File("here"));
    assert_eq!(status(&scenario, "notes/a.md"), Status::Ok);
}

#[test]
fn links_in_code_and_to_other_sites_are_not_references_and_their_twins_are() {
    let scenario = board();
    let quoted = "`[a](/nowhere)`\n\n```\n[b](/nowhere)\n```\n\n[c](https://x.example/nowhere)";
    scenario.write("notes/a.md", Item::File(quoted));
    assert_eq!(status(&scenario, "notes/a.md"), Status::Ok);
    assert!(refs(&scenario, "/notes/a.md", None).from.is_empty());
    scenario.write("notes/a.md", Item::File("[a](/nowhere)"));
    assert_eq!(
        status(&scenario, "notes/a.md"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn refs_of_a_note_answers_from_a_warm_index_as_from_a_cold_one_after_its_links_change() {
    let scenario = noted();
    let cache = tempfile::tempdir().unwrap();
    let before = refs(&scenario, "/notes/a.md", Some(cache.path()));
    scenario.write("notes/a.md", Item::File("[gone](/gone) [b](b.md)"));
    let warm = refs(&scenario, "/notes/a.md", Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, refs(&scenario, "/notes/a.md", None));
}
