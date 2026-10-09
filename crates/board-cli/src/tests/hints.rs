//! Hints that name the values an error knows, and the typed `fields` of
//! `board/error/v1` (STANDARDS.md 25).

use serde_json::json;

use super::*;

/// The board with commits off, so a put writes only the working tree.
fn uncommitted() -> Scenario {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    scenario
}

fn put(scenario: &Scenario, arguments: &[&str], record: &str) -> Output {
    board_with(scenario.workdir(), None, arguments, record.as_bytes())
}

#[test]
fn a_missing_path_hints_ls_of_its_parent_and_that_listing_runs() {
    let scenario = board();
    scenario.write("tasks/t1.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    let missing = board_in(scenario.workdir(), &["get", "/tasks/t9"]);
    assert_eq!(
        hint_of(&missing),
        "run `board ls /tasks` to see what is there; paths leave out `.json`"
    );
    let error = json(&board_in(
        scenario.workdir(),
        &["get", "/tasks/t9", "--json"],
    ));
    assert_eq!(error["fields"], json!({"path": "/tasks/t9"}));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/tasks"]).code, 0);
    let top = board_in(scenario.workdir(), &["ls", "/t9"]);
    assert_eq!(
        hint_of(&top),
        "run `board ls /` to see what is there; paths leave out `.json`"
    );
}

#[test]
fn a_stale_put_hints_the_put_that_succeeds_and_that_put_runs() {
    let scenario = uncommitted();
    let record = r#"{"$type": "lr/project/v1"}"#;
    let current = scenario.blob(record);
    let stale = put(
        &scenario,
        &["put", "/projects/p", "--expect", "none"],
        record,
    );
    assert_eq!(
        hint_of(&stale),
        format!(
            "read the record again with `board get /projects/p`, reapply your change, \
             and put it with `board put /projects/p --expect {current}`"
        )
    );
    let error = json(&put(
        &scenario,
        &["put", "/projects/p", "--expect", "none", "--json"],
        record,
    ));
    assert_eq!(error["code"], "board::put::stale");
    assert_eq!(
        error["fields"],
        json!({"path": "/projects/p", "blob": current.to_string(), "found": current.to_string()})
    );
    let hinted = ["put", "/projects/p", "--expect", &current.to_string()];
    assert_eq!(put(&scenario, &hinted, record).code, 0);
}

#[test]
fn a_stale_put_of_a_path_that_holds_nothing_hints_expect_none_and_that_put_runs() {
    let scenario = uncommitted();
    let record = r#"{"$type": "lr/project/v1"}"#;
    let blob = scenario.blob(record).to_string();
    let stale = put(
        &scenario,
        &["put", "/projects/q", "--expect", &blob],
        record,
    );
    assert_eq!(stale.code, 1);
    assert_eq!(
        hint_of(&stale),
        "nothing is at /projects/q; put it as a new file with \
         `board put /projects/q --expect none`"
    );
    let error = json(&put(
        &scenario,
        &["put", "/projects/q", "--expect", &blob, "--json"],
        record,
    ));
    assert_eq!(
        error["fields"],
        json!({"path": "/projects/q", "found": null})
    );
    let hinted = put(
        &scenario,
        &["put", "/projects/q", "--expect", "none"],
        record,
    );
    assert_eq!(hinted.code, 0);
}

#[test]
fn a_record_of_an_unknown_type_names_its_schema_path_and_a_known_type_is_written() {
    let scenario = uncommitted();
    let refused = put(
        &scenario,
        &["put", "/notes/a"],
        r#"{"$type": "lr/note/v1"}"#,
    );
    assert_eq!(
        hint_of(&refused),
        "add the type's schema at `.board/types/lr/note/v1.schema.json`, or fix `$type`"
    );
    let error = json(&put(
        &scenario,
        &["put", "/notes/a", "--json"],
        r#"{"$type": "lr/note/v1"}"#,
    ));
    assert_eq!(error["code"], "board::record::unknown_type");
    assert_eq!(
        error["fields"],
        json!({
            "path": "/notes/a",
            "type": "lr/note/v1",
            "schema": ".board/types/lr/note/v1.schema.json",
        })
    );
    let admitted = put(
        &scenario,
        &["put", "/notes/a"],
        r#"{"$type": "lr/project/v1"}"#,
    );
    assert_eq!(admitted.code, 0);
}

#[test]
fn check_names_the_schema_an_unknown_type_needs_and_a_known_type_checks_clean() {
    let scenario = uncommitted();
    scenario.write("notes/a.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    let refused = board_in(scenario.workdir(), &["check"]);
    assert_eq!(refused.code, 1);
    assert!(
        String::from_utf8_lossy(&refused.stdout).contains(
            "degraded board::record::unknown_type /notes/a: no schema for lr/note/v1 at \
             .board/types/lr/note/v1.schema.json\n"
        ),
        "{refused:?}"
    );
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["logical"] == "/notes/a")
        .unwrap()
        .clone();
    assert_eq!(entry["schema"], ".board/types/lr/note/v1.schema.json");
    scenario.write("notes/a.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    let admitted = json(&board_in(scenario.workdir(), &["check", "--json"]));
    assert_eq!(admitted["degraded"], false);
    assert!(
        admitted["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry.get("schema").is_none())
    );
}

#[test]
fn a_record_its_schema_refuses_names_the_pointer_and_a_valid_one_is_written() {
    let scenario = uncommitted();
    let invalid = r#"{"$type": "lr/task/v1", "title": 5}"#;
    let refused = put(&scenario, &["put", "/tasks/a"], invalid);
    assert_eq!(
        hint_of(&refused),
        "change the value at `/title` so the record satisfies its schema in `.board/types/`"
    );
    let error = json(&put(&scenario, &["put", "/tasks/a", "--json"], invalid));
    assert_eq!(error["code"], "board::record::invalid");
    assert_eq!(
        error["fields"],
        json!({"path": "/tasks/a", "pointer": "/title"})
    );
    let valid = r#"{"$type": "lr/task/v1", "title": "a"}"#;
    assert_eq!(put(&scenario, &["put", "/tasks/a"], valid).code, 0);
}

#[test]
fn a_mount_of_a_missing_commit_hints_updating_that_mount_and_its_path_parses() {
    let mounted = Scenario::new();
    let pinned = mounted.commit(&[("x.json", "{}")]);
    let scenario = board();
    let repo = mounted.workdir().to_str().unwrap();
    scenario
        .write(
            "vendor/m.json",
            Item::File(&mount_of(repo, ObjectId::Sha1([9; 20]))),
        )
        .write("vendor/n.json", Item::File(&mount_of(repo, pinned)));
    let refused = board_in(scenario.workdir(), &["get", "/vendor/m/x"]);
    assert_eq!(
        hint_of(&refused),
        "fetch the commit into the mounted repository, or run `board mount update /vendor/m`"
    );
    let error = json(&board_in(
        scenario.workdir(),
        &["get", "/vendor/m/x", "--json"],
    ));
    assert_eq!(error["code"], "board::mount::commit_missing");
    assert_eq!(error["fields"], json!({"path": "/vendor/m"}));
    assert_eq!(
        board_in(scenario.workdir(), &["get", "/vendor/n/x"]).code,
        0
    );
}

#[test]
fn get_json_gives_the_blob_to_expect_and_a_file_that_is_not_a_record_names_its_blob() {
    let scenario = uncommitted();
    let stored = r#"{"$type": "lr/project/v1"}"#;
    let report = json(&board_in(
        scenario.workdir(),
        &["get", "/projects/p", "--json"],
    ));
    let blob = scenario.blob(stored).to_string();
    assert_eq!(report["blob"], blob);
    let hinted = ["put", "/projects/p", "--expect", &blob];
    assert_eq!(put(&scenario, &hinted, stored).code, 0);
    scenario.write("notes.md", Item::File("# n\n"));
    let refused = board_in(scenario.workdir(), &["get", "/notes.md", "--json"]);
    let error = json(&refused);
    assert_eq!(error["code"], "board::record::not_a_record");
    assert_eq!(
        error["fields"],
        json!({"path": "/notes.md", "blob": scenario.blob("# n\n").to_string()})
    );
    assert_eq!(
        error["hint"],
        "only a record has a JSON report or can be put; \
         `board get /notes.md` without `--json` prints the file's bytes"
    );
    assert_eq!(board_in(scenario.workdir(), &["get", "/notes.md"]).code, 0);
}

#[test]
fn a_record_with_a_merge_conflict_hints_checking_its_path_and_a_resolved_one_is_written() {
    let scenario = uncommitted();
    let conflicted = "<<<<<<< ours\n{\"$type\": \"lr/project/v1\"}\n=======\n{}\n>>>>>>> theirs\n";
    let refused = put(&scenario, &["put", "/projects/q"], conflicted);
    assert_eq!(
        hint_of(&refused),
        "resolve the git merge conflict in the file, then run `board check /projects/q`"
    );
    let error = json(&put(
        &scenario,
        &["put", "/projects/q", "--json"],
        conflicted,
    ));
    assert_eq!(error["code"], "board::json::merge_conflict");
    assert_eq!(error["fields"], json!({"path": "/projects/q"}));
    let resolved = put(
        &scenario,
        &["put", "/projects/q"],
        r#"{"$type": "lr/project/v1"}"#,
    );
    assert_eq!(resolved.code, 0);
    assert_eq!(
        board_in(scenario.workdir(), &["check", "/projects/q"]).code,
        0
    );
}

#[test]
fn a_directory_asked_for_as_a_record_hints_listing_it_and_a_record_is_read() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["get", "/projects"]);
    assert_eq!(
        hint_of(&refused),
        "only a record has a JSON report or can be put; `board ls /projects` shows what is there"
    );
    let error = json(&board_in(
        scenario.workdir(),
        &["get", "/projects", "--json"],
    ));
    assert_eq!(error["code"], "board::record::not_a_record");
    assert_eq!(error["fields"], json!({"path": "/projects"}));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/projects"]).code, 0);
    assert_eq!(
        board_in(scenario.workdir(), &["get", "/projects/p"]).code,
        0
    );
}

#[test]
fn a_malformed_reference_names_its_pointer_and_a_logical_path_is_written() {
    let scenario = uncommitted();
    let malformed = r#"{"$type": "lr/task/v1", "title": "a", "project": "projects/p"}"#;
    let refused = put(&scenario, &["put", "/tasks/a"], malformed);
    assert_eq!(
        hint_of(&refused),
        "change the reference at `/project` to a logical path that starts with `/`, \
         such as `/projects/p`"
    );
    let error = json(&put(&scenario, &["put", "/tasks/a", "--json"], malformed));
    assert_eq!(error["code"], "board::ref::malformed");
    assert_eq!(
        error["fields"],
        json!({"path": "/tasks/a", "pointer": "/project"})
    );
    let valid = r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#;
    assert_eq!(put(&scenario, &["put", "/tasks/a"], valid).code, 0);
}

#[test]
fn a_dangling_link_hints_at_the_link_and_a_link_to_a_record_is_followed() {
    let scenario = board();
    scenario
        .write(
            "links/gone.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/none"}"#),
        )
        .write(
            "links/p.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        );
    let refused = board_in(scenario.workdir(), &["get", "/links/gone"]);
    assert_eq!(
        hint_of(&refused),
        "create the file the link at /links/gone names, or change the link's `path`; \
         `board ls /links` shows the link"
    );
    let error = json(&board_in(
        scenario.workdir(),
        &["get", "/links/gone", "--json"],
    ));
    assert_eq!(error["code"], "board::ref::dangling");
    assert_eq!(error["fields"], json!({"path": "/links/gone"}));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/links"]).code, 0);
    assert_eq!(board_in(scenario.workdir(), &["get", "/links/p"]).code, 0);
}
