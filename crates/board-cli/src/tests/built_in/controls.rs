//! The controls plugin through the binary: what enabling it writes, what its
//! schema admits, and what `controls.for-path` answers. `check.rs` and
//! `evaluate.rs` hold its other two capabilities. These were the plugin's
//! pytest parity suite.

use super::*;

mod check;
mod evaluate;

const INVALID: Option<&str> = Some("board::record::invalid");

/// A board that enables controls.
fn board() -> Fixture {
    Fixture::enabling(&["controls"])
}

/// A `controls/control/v1` record with `requirement` and `fields`.
fn control(requirement: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "controls/control/v1",
            "requirement": requirement,
            "purpose": "A test.",
        }),
        fields,
    )
}

/// A new repository holding one commit of `files`, mounted at `path` in
/// `board`. The repository lives as long as the value returned.
fn library(board: &Fixture, path: &str, files: &[(&str, &str)]) -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let commit = library.commit(files);
    mount(board, path, &library, commit);
    (library, commit)
}

/// The paths of the controls `controls.for-path` returns for `path`.
fn applying(board: &Fixture, path: &str) -> Vec<String> {
    let output = board.call("controls.for-path", &["--path", path]);
    (output["controls"].as_array().unwrap().iter())
        .map(|item| item["control"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships() {
    let board = board();
    let report = board.json(&["plugin", "list"]);
    let plugin = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "controls")
        .unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"]),
        (&json!("ok"), &json!(true))
    );
    assert!(plugin.get("run").is_none(), "{plugin}");
    let shipped = shipped("controls");
    let paths: Vec<&str> = shipped.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, [".board/types/controls/control/v1.schema.json"]);
    for (path, text) in shipped {
        assert_eq!(board.read(&path), text, "{path}");
    }
}

#[test]
fn enabling_writes_the_schema_so_a_control_checks_clean_and_a_broken_one_does_not() {
    let board = board();
    board
        .write("controls/ok.json", &control("R.", &json!({})))
        .write(
            "controls/bad.json",
            r#"{"$type": "controls/control/v1", "requirement": "R."}"#,
        );
    let checked = board.checked("/");
    assert_eq!(checked["/controls/ok"], None);
    assert_eq!(checked["/.board/types/controls/control/v1.schema"], None);
    assert_eq!(checked["/controls/bad"].as_deref(), INVALID);
}

#[test]
fn call_help_prints_the_path_flag_with_its_sentence() {
    let board = board();
    let sentence = "The directory or file whose controls to check, by its logical path, \
                    such as /docs; / when left out";
    let relative = "; a path without a leading / is read from the current directory.";
    let help = board.run(&["call", "controls.check", "-h"]);
    assert_eq!(help.code, 0, "{}", text(&help.stderr));
    let help = text(&help.stdout);
    assert!(
        help.contains(&format!("--path <path>  {sentence}{relative}\n")),
        "{help}"
    );
}

#[test]
fn a_budget_measured_per_directory_is_invalid_and_one_measured_per_file_is_not() {
    let board = board();
    board
        .write(
            "controls/file.json",
            &control(
                "R.",
                &json!({"rule": {"budget": "words", "max": 1, "per": "file"}}),
            ),
        )
        .write(
            "controls/directory.json",
            &control(
                "R.",
                &json!({"rule": {"budget": "words", "max": 1, "per": "directory"}}),
            ),
        );
    assert_eq!(
        board.checked("/controls"),
        codes(&[("/controls/directory", INVALID), ("/controls/file", None)])
    );
}

#[test]
fn a_pattern_rule_with_an_unknown_show_is_invalid_and_one_with_a_known_show_is_not() {
    let board = board();
    let rule = |show: &str| {
        control(
            "R.",
            &json!({"rule": {"pattern": "/private/names.txt", "show": show}}),
        )
    };
    board
        .write("private/names.txt", "acme\n")
        .write("controls/known.json", &rule("match-only"))
        .write("controls/unknown.json", &rule("all"));
    assert_eq!(
        board.checked("/controls"),
        codes(&[("/controls/known", None), ("/controls/unknown", INVALID)])
    );
}

#[test]
fn a_rule_file_board_cannot_find_dangles_and_one_it_finds_does_not() {
    let board = board();
    board
        .write("rubrics/plain.md", "Write plainly.\n")
        .write(
            "controls/found.json",
            &control("R.", &json!({"rule": {"judged": "/rubrics/plain.md"}})),
        )
        .write(
            "controls/lost.json",
            &control("R.", &json!({"rule": {"judged": "/rubrics/lost.md"}})),
        );
    assert_eq!(
        board.checked("/controls"),
        codes(&[
            ("/controls/found", None),
            ("/controls/lost", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn a_rule_mixing_two_kinds_is_invalid_and_each_kind_alone_is_not() {
    let board = board();
    let rule = |rule: Value| control("R.", &json!({ "rule": rule }));
    board
        .write("rubrics/plain.md", "Write plainly.\n")
        .write(
            "controls/budget.json",
            &rule(json!({"budget": "words", "max": 1})),
        )
        .write(
            "controls/mixed.json",
            &rule(json!({"budget": "words", "max": 1, "judged": "/rubrics/plain.md"})),
        )
        .write(
            "controls/stray.json",
            &rule(json!({"judged": "/rubrics/plain.md", "show": "pattern"})),
        )
        .write("controls/null.json", &rule(Value::Null));
    assert_eq!(
        board.checked("/controls"),
        codes(&[
            ("/controls/budget", None),
            ("/controls/mixed", INVALID),
            ("/controls/null", INVALID),
            ("/controls/stray", INVALID),
        ])
    );
}

#[test]
fn a_control_applies_to_what_it_references_and_to_what_its_patterns_match() {
    let board = board();
    board
        .write("AGENTS.md", "Rules.\n")
        .write("docs/a.md", "A.\n")
        .write("docs/b.txt", "B.\n")
        .write(
            "controls/agents.json",
            &control("Stay small.", &json!({"applies_to": ["/AGENTS.md"]})),
        )
        .write(
            "controls/docs.json",
            &control("Say what exists.", &json!({"paths": ["docs/*.md"]})),
        );
    assert_eq!(applying(&board, "/AGENTS.md"), ["/controls/agents"]);
    assert_eq!(applying(&board, "/docs/a.md"), ["/controls/docs"]);
    assert!(applying(&board, "/docs/b.txt").is_empty());
}

#[test]
fn the_output_carries_the_requirement_purpose_and_rule_and_null_for_a_missing_rule() {
    let board = board();
    board
        .write("AGENTS.md", "Rules.\n")
        .write(
            "controls/agents.json",
            &control(
                "Stay short.",
                &json!({"applies_to": ["/AGENTS.md"], "rule": {"budget": "words", "max": 10}}),
            ),
        )
        .write(
            "controls/kind.json",
            &control("Be kind.", &json!({"applies_to": ["/AGENTS.md"]})),
        );
    let output = board.call("controls.for-path", &["--path", "/AGENTS.md"]);
    assert_eq!(
        output["controls"],
        json!([
            {
                "control": "/controls/agents",
                "requirement": "Stay short.",
                "purpose": "A test.",
                "rule": {"budget": "words", "max": 10},
            },
            {"control": "/controls/kind", "requirement": "Be kind.", "purpose": "A test.", "rule": null},
        ])
    );
}

#[test]
fn an_invalid_control_applies_to_nothing_and_its_valid_twin_applies() {
    let board = board();
    board
        .write("AGENTS.md", "Rules.\n")
        .write(
            "controls/broken.json",
            &control("R.", &json!({"applies_to": ["/AGENTS.md"], "extra": true})),
        )
        .write(
            "controls/kept.json",
            &control("R.", &json!({"applies_to": ["/AGENTS.md"]})),
        );
    assert_eq!(applying(&board, "/AGENTS.md"), ["/controls/kept"]);
}

#[test]
fn a_path_with_a_quote_is_matched_as_written() {
    let board = board();
    board.write("docs/it's.md", "A.\n").write(
        "controls/docs.json",
        &control("R.", &json!({"paths": ["docs/*"]})),
    );
    assert_eq!(applying(&board, "/docs/it's.md"), ["/controls/docs"]);
}

#[test]
fn a_path_with_no_file_yet_is_matched_by_pattern_and_a_record_by_its_file() {
    let board = board();
    board
        .write(
            "controls/docs.json",
            &control(
                "Say what exists.",
                &json!({"paths": ["docs/*.md", "tasks/*.json"]}),
            ),
        )
        .write("tasks/a.json", "{}");
    assert_eq!(applying(&board, "/docs/new.md"), ["/controls/docs"]);
    assert!(applying(&board, "/docs/new.txt").is_empty());
    assert_eq!(applying(&board, "/tasks/a"), ["/controls/docs"]);
}

/// A board with `docs/a.md`, a link `/alias` to `/docs` and a control on
/// `docs/*.md`.
fn linked() -> Fixture {
    let board = board();
    board
        .write("docs/a.md", "A.\n")
        .write(
            "alias.json",
            r#"{"$type": "board/mount/v1", "path": "/docs"}"#,
        )
        .write(
            "controls/docs.json",
            &control("R.", &json!({"paths": ["docs/*.md"]})),
        );
    board
}

#[test]
fn a_path_asked_through_a_link_is_matched_where_it_leads() {
    let board = linked();
    assert_eq!(applying(&board, "/alias/a.md"), ["/controls/docs"]);
    assert_eq!(applying(&board, "/docs/a.md"), ["/controls/docs"]);
}

#[test]
fn a_new_file_asked_through_a_link_is_matched_where_it_would_be_written() {
    let board = linked();
    assert_eq!(applying(&board, "/alias/new.md"), ["/controls/docs"]);
    assert_eq!(applying(&board, "/alias/sub/new.md"), ["/controls/docs"]);
    assert!(applying(&board, "/alias/new.txt").is_empty());
    assert!(
        applying(&board, "/alias/a.md/new.md").is_empty(),
        "nothing can be beneath a file"
    );
}

#[test]
fn a_pattern_reaching_into_a_mount_matches_files_there_and_one_that_does_not_matches_none() {
    let board = board();
    let _library = library(
        &board,
        "vendor/lib.json",
        &[("docs/a.md", "A.\n"), ("tasks/a.json", "{}")],
    );
    board
        .write(
            "controls/vendor.json",
            &control(
                "R.",
                &json!({"paths": ["vendor/*.md", "vendor/*/tasks/*.json"]}),
            ),
        )
        .write(
            "controls/own.json",
            &control("R.", &json!({"paths": ["docs/*", "tasks/*"]})),
        );
    assert_eq!(
        applying(&board, "/vendor/lib/docs/a.md"),
        ["/controls/vendor"]
    );
    assert_eq!(
        applying(&board, "/vendor/lib/docs/new.md"),
        ["/controls/vendor"],
        "matched where it would be"
    );
    assert_eq!(
        applying(&board, "/vendor/lib/tasks/a"),
        ["/controls/vendor"],
        "a record is matched by its file"
    );
    assert!(applying(&board, "/vendor/lib/docs/a.txt").is_empty());
    board
        .write(
            "controls/beside.json",
            &control("R.", &json!({"paths": ["vendor/*.md"]})),
        )
        .write(
            "controls/vendor.json",
            &control("R.", &json!({"paths": ["vendor/lib/*"]})),
        );
    assert_eq!(
        applying(&board, "/vendor/new.md"),
        ["/controls/beside"],
        "an own file beside the mount"
    );
}

#[test]
fn a_file_in_a_commit_mounted_twice_is_matched_by_the_path_check_counts_it_under() {
    let board = board();
    let (library, commit) = library(&board, "aa/lib.json", &[("docs/a.md", "one")]);
    mount(&board, "zz/long/lib.json", &library, commit);
    let rule = json!({"budget": "words", "max": 9});
    board
        .write(
            "controls/short.json",
            &control("R.", &json!({"paths": ["aa/*"], "rule": rule})),
        )
        .write(
            "controls/long.json",
            &control("R.", &json!({"paths": ["zz/*"], "rule": rule})),
        );
    let measured = board.call("controls.check", &[])["measured"].clone();
    let pairs: Vec<Value> = (measured.as_array().unwrap().iter())
        .map(|item| json!([item["control"], item["files"]]))
        .collect();
    assert_eq!(
        pairs,
        [
            json!(["/controls/long", []]),
            json!(["/controls/short", ["/aa/lib/docs/a.md"]]),
        ]
    );
    assert_eq!(
        applying(&board, "/zz/long/lib/docs/a.md"),
        ["/controls/short"]
    );
    assert_eq!(applying(&board, "/aa/lib/docs/a.md"), ["/controls/short"]);
    assert_eq!(
        applying(&board, "/zz/long/lib/docs/new.md"),
        ["/controls/long"],
        "a missing path is matched as asked"
    );
}

#[test]
fn a_directory_named_json_is_matched_by_its_name_and_a_json_file_or_symlink_by_its_file_name() {
    let board = board();
    board
        .write("docs/t.md", "T.\n")
        .write("docs/a.json", "{}")
        .write("box.json/f.md", "F.\n")
        .write(
            "controls/names.json",
            &control(
                "R.",
                &json!({"paths": ["docs/l.json", "docs/a.json", "box.json"]}),
            ),
        )
        .write(
            "controls/doubled.json",
            &control("R.", &json!({"paths": ["*.json.json"]})),
        );
    board.board.write("docs/l.json", Item::Symlink("t.md"));
    assert_eq!(applying(&board, "/docs/l"), ["/controls/names"]);
    assert_eq!(applying(&board, "/docs/a"), ["/controls/names"]);
    assert_eq!(applying(&board, "/box.json"), ["/controls/names"]);
}

#[test]
fn board_refuses_the_call_when_its_control_schema_is_wider_and_answers_with_the_shipped_one() {
    let board = board();
    board.write("AGENTS.md", "Rules.\n").write(
        "controls/kept.json",
        &control("R.", &json!({"applies_to": ["/AGENTS.md"]})),
    );
    assert_eq!(applying(&board, "/AGENTS.md"), ["/controls/kept"]);
    board.call("controls.check", &[]);
    let schema = ".board/types/controls/control/v1.schema.json";
    let mut widened: Value = serde_json::from_str(&board.read(schema)).unwrap();
    widened["additionalProperties"] = json!(true);
    board.write(schema, &widened.to_string());
    for (target, flags) in [
        ("controls.for-path", &["--path", "/AGENTS.md"][..]),
        ("controls.check", &[]),
    ] {
        let refused = board.refusal(target, flags);
        assert_eq!(
            refused["code"], "board::plugin::schema_differs",
            "{refused}"
        );
        assert_eq!(refused["fields"]["schema"], schema);
    }
}

#[test]
fn a_malformed_path_is_refused_with_boards_code_and_a_well_formed_one_is_answered() {
    let board = board();
    let refused = board.refusal("controls.for-path", &["--path", "/docs/../x"]);
    assert_eq!(refused["code"], "controls::board::refused", "{refused}");
    assert_eq!(refused["fields"]["code"], "board::cli::usage");
    assert!(
        !refused["message"]
            .as_str()
            .unwrap()
            .contains("first on PATH"),
        "{refused}"
    );
    assert!(applying(&board, "/docs/x").is_empty());
}

#[test]
fn a_control_whose_malformed_reference_hides_a_schema_error_is_left_out_and_a_dangling_one_applies()
{
    let board = board();
    board
        .write(
            "controls/broken.json",
            &control(
                "R.",
                &json!({"applies_to": ["docs"], "paths": ["docs/*"], "purpose": 5}),
            ),
        )
        .write(
            "controls/kept.json",
            &control(
                "R.",
                &json!({"applies_to": ["/gone.md"], "paths": ["docs/*"]}),
            ),
        );
    assert_eq!(applying(&board, "/docs/a.md"), ["/controls/kept"]);
}

#[test]
fn a_control_with_one_dangling_reference_still_applies_to_the_others() {
    let board = board();
    board.write("AGENTS.md", "Rules.\n").write(
        "controls/kept.json",
        &control("R.", &json!({"applies_to": ["/AGENTS.md", "/gone.md"]})),
    );
    assert_eq!(applying(&board, "/AGENTS.md"), ["/controls/kept"]);
}
