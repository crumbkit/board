//! Hints that name the values an error knows, and the typed `fields` of
//! `board/failure/v1` (STANDARDS.md 25).

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
    let error = super::get::got(&board_in(
        scenario.workdir(),
        &["get", "/tasks/t9", "--json"],
    ));
    assert_eq!(
        error["fields"],
        json!({"path": "/tasks/t9", "ancestor": "/tasks"})
    );
    assert_eq!(board_in(scenario.workdir(), &["ls", "/tasks"]).code, 0);
    let top = board_in(scenario.workdir(), &["ls", "/t9"]);
    assert_eq!(
        hint_of(&top),
        "run `board ls /` to see what is there; paths leave out `.json`"
    );
}

#[test]
fn a_path_missing_several_levels_deep_hints_ls_of_its_deepest_ancestor_that_exists() {
    let scenario = board();
    for (missing, ancestor) in [
        ("/taks/t1", "/"),
        ("/a/b/c/d", "/"),
        ("/projects/q/r", "/projects"),
        ("/projects/p/x/y", "/projects/p"),
    ] {
        let refused = board_in(scenario.workdir(), &["get", missing]);
        let stderr = text(&refused.stderr);
        assert!(
            stderr.ends_with("  code:  board::path::not_found\n"),
            "{stderr}"
        );
        assert_eq!(
            hint_of(&refused),
            format!("run `board ls {ancestor}` to see what is there; paths leave out `.json`")
        );
        assert_eq!(board_in(scenario.workdir(), &["ls", ancestor]).code, 0);
    }
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
            "degraded board::record::unknown_type /notes/a names an unknown type: no schema for \
             lr/note/v1 at .board/types/lr/note/v1.schema.json\n"
        ),
        "{refused:?}"
    );
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["logical"] == "/notes/a")
        .unwrap()
        .clone();
    assert_eq!(entry["schema"], ".board/types/lr/note/v1.schema.json");
    scenario.write("notes/a.json", Item::File(r#"{"$type": "lr/project/v1"}"#));
    let admitted = json(&board_in(scenario.workdir(), &["check", "--json"]));
    assert_eq!(admitted["summary"]["degraded"], 0);
    assert!(
        admitted["data"]
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
    let error = super::get::got(&board_in(
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
fn get_json_gives_the_blob_to_expect_and_a_file_that_is_not_a_record_its_blob_and_why() {
    let scenario = uncommitted();
    let stored = r#"{"$type": "lr/project/v1"}"#;
    let report = super::get::got(&board_in(
        scenario.workdir(),
        &["get", "/projects/p", "--json"],
    ));
    let blob = scenario.blob(stored).to_string();
    assert_eq!(report["blob"], blob);
    let hinted = ["put", "/projects/p", "--expect", &blob];
    assert_eq!(put(&scenario, &hinted, stored).code, 0);
    scenario.write("notes.md", Item::File("# n\n"));
    let read = board_in(scenario.workdir(), &["get", "/notes.md", "--json"]);
    assert_eq!(read.code, 0);
    let answer = super::get::got(&read);
    assert_eq!(answer["code"], "board::record::not_a_record");
    assert_eq!(answer["blob"], scenario.blob("# n\n").to_string());
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
        "only a record is put without `--file`, and nothing is put beneath a file; `board ls /projects` shows what is there"
    );
    let error = super::get::got(&board_in(
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
fn check_hints_how_to_fix_each_degraded_entry_and_gives_a_healthy_one_no_hint() {
    let scenario = board();
    scenario
        .write(
            "tasks/t.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "t", "project": "/projects/nope"}"#),
        )
        .write(
            "tasks/u.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "u", "project": "/projects/p"}"#),
        )
        .write(
            "links/gone.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/none"}"#),
        )
        .write("notes/n.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    let schema = "add the type's schema at `.board/types/lr/note/v1.schema.json`, or fix `$type`";
    let reference = "create the record the reference at `/project` names, or change \
                     the reference; `board refs /tasks/t` lists the record's references";
    let link = "create the file the link at /links/gone names, or change the link's \
                `path`; `board ls /links` shows the link";
    let checked = board_in(scenario.workdir(), &["check"]);
    let stderr = text(&checked.stderr);
    let hints: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("hint: "))
        .collect();
    assert_eq!(
        hints,
        [
            format!("hint: /links/gone: {link}"),
            format!("hint: /notes/n: {schema}"),
            format!("hint: /tasks/t: {reference}"),
        ]
    );
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let hint = |path: &str| {
        report["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == path)
            .unwrap()
            .get("hint")
            .cloned()
    };
    assert_eq!(hint("tasks/t.json"), Some(json!(reference)));
    assert_eq!(hint("links/gone.json"), Some(json!(link)));
    assert_eq!(hint("notes/n.json"), Some(json!(schema)));
    assert_eq!(hint("tasks/u.json"), None);
    let refs = board_in(scenario.workdir(), &["refs", "/tasks/t"]);
    assert!(text(&refs.stdout).contains("from /project /projects/nope degraded"));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/links"]).code, 0);
}

/// The `hint: <path>: ` lines `check` prints on stderr.
fn check_hints(scenario: &Scenario) -> Vec<String> {
    text(&board_in(scenario.workdir(), &["check"]).stderr)
        .lines()
        .filter(|line| line.starts_with("hint: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn check_hints_name_the_pointer_at_fault_and_the_command_they_name_runs() {
    let mounted = Scenario::new();
    mounted.commit(&[("x.json", "{}")]);
    let scenario = uncommitted();
    let repo = mounted.workdir().to_str().unwrap();
    let link = |path: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{path}"}}"#);
    scenario
        .write(
            "vendor/m.json",
            Item::File(&mount_of(repo, ObjectId::Sha1([9; 20]))),
        )
        .write("links/l.json", Item::File(&link("/vendor/m/x")))
        .write("links/a.json", Item::File(&link("/nope/x")))
        .write("links/b.json", Item::File(&link("/links/a")));
    let hints = check_hints(&scenario);
    for hint in [
        "hint: /links/b: create the file the link at /links/a names, or change the link's \
         `path`; `board ls /links` shows the link",
        "hint: /links/l: fetch the commit into the mounted repository, or run \
         `board mount update /vendor/m`",
    ] {
        assert!(hints.iter().any(|line| line == hint), "{hint}\n{hints:#?}");
    }
    let updated = board_in(scenario.workdir(), &["mount", "update", "/vendor/m"]);
    assert_eq!(updated.code, 0, "{}", text(&updated.stderr));
    assert!(
        !check_hints(&scenario)
            .iter()
            .any(|line| line.starts_with("hint: /links/l:"))
    );
}

#[test]
fn check_says_why_json_does_not_parse_and_json_that_parses_is_ok() {
    let scenario = board();
    scenario
        .write("bad.json", Item::File("{bad"))
        .write("dup.json", Item::File(r#"{"a": 1, "a": 2}"#))
        .write(".board/types/board/x/v1.schema.json", Item::File("{}"))
        .write("good.json", Item::File(r#"{"a": 1}"#));
    let lines = text(&board_in(scenario.workdir(), &["check"]).stdout);
    for line in [
        "degraded board::json::parse /bad is invalid: json does not parse: key must be a string at line 1 column 2",
        r#"degraded board::json::duplicate_key /dup is invalid: json repeats the key "a" in one object"#,
        "degraded board::schema::invalid /.board/types/board/x/v1.schema \
         is in the board namespace, which is Board's own, so a board cannot define a schema in it",
        "ok /good",
    ] {
        assert!(
            lines.lines().any(|printed| printed == line),
            "{line}\n{lines}"
        );
    }
}

#[test]
fn a_record_whose_schema_is_degraded_hints_at_the_schema_and_is_ok_once_it_is_fixed() {
    let scenario = board();
    let schema = ".board/types/lr/broken/v1.schema.json";
    scenario
        .write(schema, Item::File("{"))
        .write("notes/r.json", Item::File(r#"{"$type": "lr/broken/v1"}"#));
    let hint = "hint: /notes/r: fix the schema at `.board/types/lr/broken/v1.schema.json` so it \
                compiles as JSON Schema 2020-12; `board check /.board/types` says what failed";
    let hints = check_hints(&scenario);
    assert!(hints.iter().any(|line| line == hint), "{hints:#?}");
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "notes/r.json")
        .unwrap()
        .clone();
    assert_eq!(entry["schema"], schema);
    assert_eq!(
        board_in(scenario.workdir(), &["check", "/.board/types"]).code,
        1
    );
    scenario.write(schema, Item::File("{}"));
    assert_eq!(board_in(scenario.workdir(), &["check", "/notes"]).code, 0);
}

#[test]
fn check_hints_for_a_mounted_board_send_its_fixes_to_that_boards_repository() {
    let inner = Scenario::new();
    inner.commit(&[("x.json", "{}")]);
    let link = |path: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{path}"}}"#);
    let mounted = Scenario::new();
    let nested = mount_of(inner.workdir().to_str().unwrap(), ObjectId::Sha1([9; 20]));
    let commit = mounted.commit(&[
        ("vendor/n.json", &nested),
        ("links/l.json", &link("/vendor/n/x")),
        (".board/types/lr/broken/v1.schema.json", r#"{"type": 5}"#),
        ("notes/r.json", r#"{"$type": "lr/broken/v1"}"#),
        ("notes/u.json", r#"{"$type": "lr/none/v1"}"#),
        ("links/gone.json", &link("/nope")),
        ("bad.json", "{bad"),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (
            "tasks/t.json",
            r#"{"$type": "lr/task/v1", "title": "t", "project": "/projects/nope"}"#,
        ),
    ]);
    let scenario = uncommitted();
    scenario
        .write(
            "vendor/lib.json",
            Item::File(&mount_of(mounted.workdir().to_str().unwrap(), commit)),
        )
        .write("links/r.json", Item::File(&link("/vendor/lib/vendor/n/x")));
    let there = "in the repository of the board mounted at /vendor/lib";
    let nested = format!(
        "the mount at /vendor/lib/vendor/n is {there}, which Board cannot write; fetch the \
         commit into the repository the mount names, or update the mount there"
    );
    let hints = check_hints(&scenario);
    for hint in [
        format!("hint: /links/r: {nested}"),
        format!("hint: /vendor/lib/links/l: {nested}"),
        format!("hint: /vendor/lib/vendor/n: {nested}"),
        format!(
            "hint: /vendor/lib/notes/r: fix the schema at `.board/types/lr/broken/v1.schema.json` \
             {there}; `board check /vendor/lib/.board/types` says what failed"
        ),
        format!(
            "hint: /vendor/lib/notes/u: add the type's schema at \
             `.board/types/lr/none/v1.schema.json` {there}, or fix `$type` there"
        ),
        format!(
            "hint: /vendor/lib/links/gone: /vendor/lib/links/gone is {there}, which Board cannot \
             write; there, create the file the link names, or change the link's `path`, then \
             run `board mount update /vendor/lib`"
        ),
        format!(
            "hint: /vendor/lib/tasks/t: /vendor/lib/tasks/t is {there}, which Board cannot \
             write; there, create the record the reference at `/project` names, or change the \
             reference, then run `board mount update /vendor/lib`"
        ),
        format!(
            "hint: /vendor/lib/bad: /vendor/lib/bad is {there}, which Board cannot write; fix \
             the file there as the message says, then run `board mount update /vendor/lib`"
        ),
    ] {
        assert!(hints.contains(&hint), "{hint}\n{hints:#?}");
    }
    let lines = text(&board_in(scenario.workdir(), &["check"]).stdout);
    assert!(
        lines.contains(
            "degraded board::record::unknown_type /vendor/lib/notes/u names an unknown type: \
             no schema for lr/none/v1 at .board/types/lr/none/v1.schema.json\n"
        ),
        "{lines}"
    );
    let types = board_in(scenario.workdir(), &["check", "/vendor/lib/.board/types"]);
    assert!(
        text(&types.stdout).contains(
            "degraded board::schema::invalid /vendor/lib/.board/types/lr/broken/v1.schema \
             is invalid: schema does not compile: "
        ),
        "{}",
        text(&types.stdout)
    );
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let entry = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["logical"] == "/vendor/lib/notes/r")
        .unwrap()
        .clone();
    assert_eq!(
        (entry["path"].clone(), entry["schema"].clone()),
        (
            json!("notes/r.json"),
            json!(".board/types/lr/broken/v1.schema.json")
        )
    );
    let updated = board_in(scenario.workdir(), &["mount", "update", "/vendor/lib"]);
    assert_eq!(updated.code, 0, "{}", text(&updated.stderr));
}

#[test]
fn check_hints_for_the_root_board_name_its_own_files_and_mount_update() {
    let inner = Scenario::new();
    inner.commit(&[("x.json", "{}")]);
    let scenario = uncommitted();
    scenario
        .write(
            "vendor/n.json",
            Item::File(&mount_of(
                inner.workdir().to_str().unwrap(),
                ObjectId::Sha1([9; 20]),
            )),
        )
        .write(
            ".board/types/lr/broken/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write("notes/r.json", Item::File(r#"{"$type": "lr/broken/v1"}"#))
        .write("notes/u.json", Item::File(r#"{"$type": "lr/none/v1"}"#))
        .write("bad.json", Item::File("{bad"));
    let hints = check_hints(&scenario);
    for hint in [
        "hint: /bad: fix the JSON syntax at the line and column the message gives",
        "hint: /vendor/n: fetch the commit into the mounted repository, or run \
         `board mount update /vendor/n`",
        "hint: /notes/r: fix the schema at `.board/types/lr/broken/v1.schema.json` so it \
         compiles as JSON Schema 2020-12; `board check /.board/types` says what failed",
        "hint: /notes/u: add the type's schema at `.board/types/lr/none/v1.schema.json`, \
         or fix `$type`",
    ] {
        assert!(hints.iter().any(|line| line == hint), "{hint}\n{hints:#?}");
    }
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
    let error = super::get::got(&board_in(
        scenario.workdir(),
        &["get", "/links/gone", "--json"],
    ));
    assert_eq!(error["code"], "board::ref::dangling");
    assert_eq!(error["fields"], json!({"path": "/links/gone"}));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/links"]).code, 0);
    assert_eq!(board_in(scenario.workdir(), &["get", "/links/p"]).code, 0);
}

#[test]
fn a_put_through_a_mount_hints_updating_that_mount_and_a_put_beside_it_is_written() {
    let mounted = Scenario::new();
    let pinned = mounted.commit(&[("x.json", "{}")]);
    let scenario = uncommitted();
    let repo = mounted.workdir().to_str().unwrap();
    scenario.write("vendor/m.json", Item::File(&mount_of(repo, pinned)));
    let record = r#"{"$type": "lr/project/v1"}"#;
    let error = json(&put(&scenario, &["put", "/vendor/m/new", "--json"], record));
    assert_eq!(error["code"], "board::put::through_mount");
    assert_eq!(
        error["message"],
        "/vendor/m/new cannot be written: it is in the board mounted at /vendor/m, \
         whose repository has no working tree with a .board/board.json"
    );
    assert_eq!(error["fields"], json!({"path": "/vendor/m"}));
    assert_eq!(
        error["recovery"]["text"],
        "write in the mounted repository and run `board mount update /vendor/m`; \
         a put writes through one mount whose board commits its puts and whose HEAD \
         is on the branch the mount follows"
    );
    let hinted = board_in(scenario.workdir(), &["mount", "update", "/vendor/m"]);
    assert_eq!(hinted.code, 0);
    assert_eq!(put(&scenario, &["put", "/vendor/new"], record).code, 0);
}

#[test]
fn updating_a_mount_that_names_nothing_hints_ls_of_its_deepest_ancestor_and_that_listing_runs() {
    let scenario = uncommitted();
    let error = json(&board_in(
        scenario.workdir(),
        &["mount", "update", "/projects/zz", "--json"],
    ));
    assert_eq!(error["code"], "board::path::not_found");
    assert_eq!(
        error["fields"],
        json!({"path": "/projects/zz", "ancestor": "/projects"})
    );
    assert_eq!(
        error["recovery"]["text"],
        "run `board ls /projects` to see what is there; paths leave out `.json`"
    );
    assert_eq!(board_in(scenario.workdir(), &["ls", "/projects"]).code, 0);
}

#[test]
fn a_put_beneath_a_dangling_link_names_the_link_and_a_put_beside_it_is_written() {
    let scenario = uncommitted();
    scenario.write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/none"}"#),
    );
    let record = r#"{"$type": "lr/project/v1"}"#;
    let error = json(&put(&scenario, &["put", "/l/new", "--json"], record));
    assert_eq!(error["code"], "board::ref::dangling");
    assert_eq!(error["fields"], json!({"path": "/l"}));
    assert_eq!(board_in(scenario.workdir(), &["ls", "/"]).code, 0);
    assert_eq!(put(&scenario, &["put", "/projects/new"], record).code, 0);
}

/// A limited check prints a hint for each degraded entry it shows and none
/// for the entries the limit left out.
#[test]
fn a_limited_check_hints_only_the_entries_it_shows() {
    let scenario = board();
    scenario
        .write("notes/m.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("notes/n.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    let hinted = |limit: &str| {
        let checked = board_in(scenario.workdir(), &["check", "--limit", limit]);
        assert_eq!(checked.code, 1);
        let stderr = text(&checked.stderr);
        let hinted: Vec<String> = (stderr.lines())
            .filter_map(|line| line.strip_prefix("hint: "))
            .filter_map(|line| line.split(':').next().map(str::to_owned))
            .collect();
        hinted
    };
    assert_eq!(hinted("1"), ["/notes/m"]);
    assert_eq!(hinted("2"), ["/notes/m", "/notes/n"]);
}

/// A limited check's note counts every degraded entry, those the limit left
/// out included, and the run exits 1 either way.
#[test]
fn a_limited_check_counts_every_degraded_entry_as_an_unlimited_one_does() {
    let scenario = board();
    scenario
        .write("notes/m.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("notes/n.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    let note = "note: 2 of the entries report a problem, so the command exits with 1.";
    for limit in ["1", "10"] {
        let checked = board_in(scenario.workdir(), &["check", "--limit", limit]);
        assert_eq!(checked.code, 1, "--limit {limit}");
        let stderr = text(&checked.stderr);
        assert!(
            stderr.lines().any(|line| line == note),
            "--limit {limit}: {stderr}"
        );
    }
}
