//! A Markdown file whose YAML frontmatter names a `$type` is a record:
//! `check` validates the frontmatter against the type's schema and follows
//! its references and the body's links, `query` reads the frontmatter as
//! the record's body, and `get` returns it. put/tests/file.rs holds what
//! `put` refuses.

use board_model::{Code, Kind, LogicalPath};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::json;

use super::{board, check, open, status};
use crate::{Absent, Plugins};

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// A task written as Markdown, with `fields` after its type and a body.
fn note(fields: &str, body: &str) -> String {
    format!("---\n$type: lr/task/v1\n{fields}---\n{body}")
}

/// The board with a Markdown task that names the project and links to a
/// note, the note, and a Markdown file with frontmatter but no type.
fn noted() -> Scenario {
    let scenario = board();
    scenario
        .write(
            "tasks/a.md",
            Item::File(&note(
                "title: Buy milk\ndue: 2026-10-26\nproject: /projects/p\n",
                "# Buy milk\n\nSee [b](b.md).\n",
            )),
        )
        .write("tasks/b.md", Item::File("# B\n"))
        .write("tasks/untyped.md", Item::File("---\ntitle: x\n---\n# x\n"));
    scenario
}

#[test]
fn a_markdown_file_with_a_type_is_a_record_and_one_without_is_a_file() {
    let scenario = noted();
    let report = check(&scenario);
    let kind = |path: &str| {
        report
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| (entry.kind, entry.status))
    };
    assert_eq!(kind("tasks/a.md"), Some((Kind::Record, Status::Ok)));
    assert_eq!(kind("tasks/untyped.md"), Some((Kind::File, Status::Ok)));
    assert!(!report.degraded());
}

#[test]
fn a_markdown_record_its_schema_refuses_is_invalid_and_its_valid_twin_is_ok() {
    let scenario = board();
    scenario
        .write("ok.md", Item::File(&note("title: t\n", "")))
        .write("bad.md", Item::File(&note("title: 1\n", "")));
    assert_eq!(status(&scenario, "ok.md"), Status::Ok);
    assert_eq!(
        status(&scenario, "bad.md"),
        Status::Degraded(Code::RecordInvalid)
    );
}

#[test]
fn a_markdown_records_references_and_links_are_checked_and_its_twin_with_targets_is_ok() {
    let scenario = board();
    scenario
        .write(
            "ok.md",
            Item::File(&note("title: t\nproject: /projects/p\n", "[ok](ok.md)")),
        )
        .write(
            "ref.md",
            Item::File(&note("title: t\nproject: /projects/q\n", "")),
        )
        .write(
            "wrong.md",
            Item::File(&note("title: t\nproject: /ok.md\n", "")),
        )
        .write(
            "link.md",
            Item::File(&note("title: t\n", "[gone](gone.md)")),
        );
    assert_eq!(status(&scenario, "ok.md"), Status::Ok);
    assert_eq!(
        status(&scenario, "ref.md"),
        Status::Degraded(Code::RefDangling)
    );
    assert_eq!(
        status(&scenario, "wrong.md"),
        Status::Degraded(Code::RefWrongType)
    );
    assert_eq!(
        status(&scenario, "link.md"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn typed_frontmatter_that_does_not_parse_degrades_a_file_and_untyped_is_left_alone() {
    let scenario = board();
    scenario
        .write(
            "typed.md",
            Item::File("---\n$type: lr/task/v1\ntitle: [\n---\n"),
        )
        .write("repeated.md", Item::File(&note("title: a\ntitle: b\n", "")))
        .write("untyped.md", Item::File("---\ntitle: a\ntitle: [\n---\n"))
        .write("inf.md", Item::File(&note("title: t\nn: .inf\n", "")))
        .write("twin.md", Item::File("---\ntitle: a\n---\n"));
    assert_eq!(
        status(&scenario, "typed.md"),
        Status::Degraded(Code::FrontmatterParse)
    );
    assert_eq!(
        status(&scenario, "repeated.md"),
        Status::Degraded(Code::FrontmatterParse)
    );
    assert_eq!(status(&scenario, "untyped.md"), Status::Ok);
    assert_eq!(
        status(&scenario, "inf.md"),
        Status::Degraded(Code::FrontmatterUnsupported)
    );
    assert_eq!(status(&scenario, "twin.md"), Status::Ok);
}

#[test]
fn links_inside_typed_frontmatter_are_not_links_and_inside_untyped_frontmatter_are() {
    let scenario = board();
    scenario
        .write(
            "a.md",
            Item::File(&note("title: '[gone](gone.md)'\n", "[b](b.md)\n")),
        )
        .write(
            "untyped.md",
            Item::File("---\ntitle: '[gone](gone.md)'\n---\n"),
        )
        .write("b.md", Item::File(""));
    assert_eq!(status(&scenario, "a.md"), Status::Ok);
    assert_eq!(
        status(&scenario, "untyped.md"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn query_reads_a_markdown_records_frontmatter_as_its_body_and_its_type() {
    let scenario = noted();
    let report = open(&scenario)
        .query(
            "SELECT path, type, status, json_extract(body, '$.due') FROM records \
             WHERE path LIKE 'tasks/%'",
            10,
            None,
            None,
            &Plugins::default(),
        )
        .unwrap();
    assert_eq!(
        report.rows,
        [vec![
            json!("tasks/a.md"),
            json!("lr/task/v1"),
            json!("ok"),
            json!("2026-10-26")
        ]]
    );
}

#[test]
fn refs_lists_a_markdown_records_references_and_links_and_what_reaches_it() {
    let scenario = noted();
    let refs = |text: &str| {
        open(&scenario)
            .refs(&[path(text)], Absent::Refuse, &[], None, None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
    };
    let report = refs("/tasks/a.md");
    let from: Vec<(&str, bool, String)> = report
        .from
        .iter()
        .map(|reference| {
            let target = reference.target.to_string();
            (reference.pointer.as_str(), reference.link, target)
        })
        .collect();
    assert_eq!(
        from,
        [
            ("/project", false, "/projects/p".to_owned()),
            ("b.md", true, "/tasks/b.md".to_owned())
        ]
    );
    assert_eq!(refs("/tasks/b.md").to[0].path, "tasks/a.md");
}

#[test]
fn get_returns_a_markdown_records_frontmatter_and_refuses_frontmatter_that_does_not_parse() {
    let scenario = noted();
    scenario.write("broken.md", Item::File(&note("a: 1\na: 2\n", "")));
    let board = open(&scenario);
    let got = board.get(&path("/tasks/a.md")).unwrap();
    assert_eq!(got.record.unwrap()["due"], "2026-10-26");
    let refusal = board.get(&path("/broken.md")).unwrap().record.unwrap_err();
    assert_eq!(refusal.code.as_str(), Code::FrontmatterParse.as_str());
    assert_eq!(
        refusal.message,
        r#"/broken.md is not a record: frontmatter repeats the key "a" in one mapping on line 4"#
    );
    let untyped = board
        .get(&path("/tasks/untyped.md"))
        .unwrap()
        .record
        .unwrap_err();
    assert_eq!(untyped.code.as_str(), Code::RecordNotARecord.as_str());
}
