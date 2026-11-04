//! Which GitHub repositories a `notes/repo/v1` record covers, the repo
//! listing, and the repos a call reads. These were
//! `python/board-plugin/tests/test_repos.py`; how a remote is shown is
//! board-model's `shown_remote`, and the caller's origin is the host's.

use serde_json::{Value, json};

use super::*;
use crate::fake::local;

fn record_of(value: &Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[test]
fn a_record_covers_a_github_remote_by_owner_or_by_repository() {
    let record = record_of(&json!({"covers": ["example-org/*", "crumbkit/board"]}));
    let cases = [
        (Some("https://github.com/example-org/home.git"), true),
        (Some("https://github.com/Example-Org/Home"), true),
        (Some("git@github.com:example-org/home.git"), true),
        (Some("ssh://git@github.com/example-org/home.git"), true),
        (Some("git@github.com-work:example-org/home.git"), true),
        (Some("example-org/home"), true),
        (Some("crumbkit/board"), true),
        (Some("another-org/board"), false),
        (Some("https://gitlab.com/example-org/home.git"), false),
        (Some("git@gitlab.com:example-org/home.git"), false),
        (Some("git@github.com.evil.io:example-org/home.git"), false),
        (
            Some("ssh://git@github.com.evil.io/example-org/home.git"),
            false,
        ),
        (Some("ssh://git@gitlab.com/example-org/home.git"), false),
        (Some("/Users/me/Code/home"), false),
        (None, false),
    ];
    for (remote, covered) in cases {
        assert_eq!(covers(Some(&record), remote), covered, "{remote:?}");
    }
    assert!(!covers(None, Some("crumbkit/board")));
}

fn row(values: &Value) -> Vec<Value> {
    values.as_array().unwrap().clone()
}

fn slices(rows: &[Vec<Value>]) -> Vec<&[Value]> {
    rows.iter().map(Vec::as_slice).collect()
}

#[test]
fn a_listed_mount_carries_its_pin_and_clone_and_the_boards_own_tree_neither() {
    let rows = [
        row(&json!([
            "/repos/work/",
            "c0ffee",
            "/clones/work",
            null,
            null,
            null,
            null
        ])),
        row(&json!(["/", "worktree", "/board", null, null, null, null])),
    ];
    let found = parsed("tasks", &slices(&rows)).unwrap();
    let heads: Vec<_> = (found.iter())
        .map(|listed| {
            (
                listed.root.as_str(),
                listed.source.as_deref(),
                listed.clone.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        heads,
        [
            ("/", None, None),
            ("/repos/work", Some("c0ffee"), Some("/clones/work"))
        ]
    );
    assert_eq!(record("tasks", &found[1]).unwrap(), None);
}

#[test]
fn a_mount_whose_clone_board_does_not_name_is_refused() {
    let rows = [row(&json!([
        "/repos/lost/",
        "c0ffee",
        null,
        null,
        null,
        null,
        null
    ]))];
    let refused = parsed("tasks", &slices(&rows)).unwrap_err();
    assert_eq!(refused.code, "tasks::board::refused");
    assert_eq!(
        Value::Object(refused.fields),
        json!({"path": "/repos/lost"})
    );
    assert_eq!(
        refused.message,
        "Board names no clone for the mount at /repos/lost; check the mount with `board check \
         /repos/lost`"
    );
}

fn listed_with(root: &str, record: Option<Value>) -> Listed {
    let row = match record {
        None => [Value::Null, Value::Null, Value::Null, Value::Null],
        Some(body) => [
            json!(RECORD_TYPE),
            json!("ok"),
            Value::Null,
            json!(body.to_string()),
        ],
    };
    Listed {
        root: root.to_owned(),
        source: (root != "/").then(|| "c0ffee".to_owned()),
        clone: (root != "/").then(|| format!("/clones{root}")),
        row,
    }
}

fn found() -> Vec<Listed> {
    vec![
        listed_with("/", None),
        listed_with("/repos/home", Some(json!({"covers": [], "personal": true}))),
        listed_with("/repos/work", Some(json!({"covers": ["acme/*"]}))),
        listed_with("/repos/other", Some(json!({"covers": ["other/*"]}))),
    ]
}

#[test]
fn covering_names_the_repos_covering_the_remote_and_the_personal_ones() {
    let (covered, personal) =
        covering("notes", &found(), Some("git@github.com:acme/x.git")).unwrap();
    assert_eq!(
        (covered, personal),
        (
            vec!["/repos/work".to_owned()],
            vec!["/repos/home".to_owned()]
        )
    );
    let (covered, _) = covering("notes", &found(), None).unwrap();
    assert!(covered.is_empty());
}

#[test]
fn scope_reads_every_repo_outside_git_and_else_the_board_the_personal_and_covering_repos() {
    let all = scope("tasks", &found(), None, None, None).unwrap();
    assert_eq!(all, ["/", "/repos/home", "/repos/work", "/repos/other"]);
    let routed = scope("tasks", &found(), None, Some("acme/x"), None).unwrap();
    assert_eq!(routed, ["/", "/repos/home", "/repos/work"]);
    let unread = scope("tasks", &found(), None, None, Some("no origin")).unwrap();
    assert_eq!(unread, ["/", "/repos/home"]);
    let named = scope(
        "tasks",
        &found(),
        Some("/repos/other/"),
        Some("acme/x"),
        None,
    )
    .unwrap();
    assert_eq!(named, ["/repos/other"]);
}

#[test]
fn scope_refuses_a_name_no_repo_is_at_and_reads_one_that_is() {
    let refused = scope("tasks", &found(), Some("/repos/none"), None, None).unwrap_err();
    assert_eq!(refused.code, "tasks::repo::not_found");
    assert_eq!(
        refused.fields["repos"],
        json!(["/", "/repos/home", "/repos/work", "/repos/other"])
    );
    assert_eq!(
        scope("tasks", &found(), Some("/repos/home"), None, None).unwrap(),
        ["/repos/home"]
    );
}

#[test]
fn a_record_that_is_not_valid_is_refused_when_a_remote_routes_and_a_valid_one_routes() {
    let mut broken = found();
    broken[2].row = [
        json!(RECORD_TYPE),
        json!("degraded"),
        json!("board::record::invalid"),
        Value::Null,
    ];
    let refused = scope("tasks", &broken, None, Some("acme/x"), None).unwrap_err();
    assert_eq!(refused.code, "tasks::repo::invalid_record");
    assert_eq!(refused.fields["path"], json!("/repos/work/notes-repo"));
    assert!(scope("tasks", &found(), None, Some("acme/x"), None).is_ok());
}

#[test]
fn a_record_body_with_a_duplicate_key_or_that_does_not_parse_is_refused_and_a_valid_one_is_read() {
    let valid = json!(r#"{"covers": ["acme/*"]}"#);
    let parsed = json_body("prompts", "/repos/a/notes-repo", &valid).unwrap();
    assert_eq!(parsed, json!({"covers": ["acme/*"]}));
    for text in [r#"{"covers": [], "covers": ["acme/*"]}"#, "{"] {
        let error = json_body("prompts", "/repos/a/notes-repo", &json!(text)).unwrap_err();
        assert_eq!(error.code, "prompts::board::refused", "{text}");
        assert!(
            error.message.starts_with(
                "board query gave the body of /repos/a/notes-repo as text that does not parse: "
            ),
            "{}",
            error.message
        );
    }
}

#[test]
fn the_boards_own_tree_is_a_notes_repo_once_it_tracks_a_file_under_daily() {
    let without = local(&[("notes/a.md", "# A\n")]);
    without.scenario.commit(&[("notes/a.md", "# A\n")]);
    assert_eq!(listed(&without, "notes").unwrap(), []);
    let with = local(&[("daily/2026-10-25.md", "# Day\n")]);
    with.scenario.commit(&[("daily/2026-10-25.md", "# Day\n")]);
    let found = listed(&with, "notes").unwrap();
    assert_eq!(found, [listed_with("/", None)]);
}
