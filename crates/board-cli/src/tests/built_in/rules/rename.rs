//! `rules.migrate --rename true`: moves each rule and source from
//! `/controls/` to `/rules/` under the new types, renames the fields
//! boundaries and crossings held, rewrites every mention of an old path or
//! capability in records and Markdown, and leaves signed records alone.

use super::*;

/// A signed order that names an old capability and path; its signature
/// is not checked here.
fn order() -> String {
    json!({
        "$type": board_model::ORDER_TYPE,
        "order": "Run `board call controls.check` on /controls/x/a.",
        "context": "/orders/o.md", "expires": "2099-01-01", "pinned": true, "signature": "AAAA",
    })
    .to_string()
}

/// A pending effect on the crossing, holding an old capability name; it is
/// signed when answered, so the rename leaves it as it is.
fn effect() -> String {
    json!({
        "$type": board_model::EFFECT_TYPE, "board": "/b", "plugin": "boundary",
        "op": "controls.evaluate", "path": format!("/crossings/{}", "a".repeat(64)),
    })
    .to_string()
}

const ORDER_CONTEXT: &str = "# Why\n\nSee /controls/x/a.\n";
const NOTE: &str = "# A note\n\nRun `board call controls.check`, then read /controls/x/a and /repos/board/controls/rules/agents.\n";

/// A board enabling the rules plugin under its former name, holding a rule
/// and a source under `/controls/`, a boundary and a crossing that name the
/// rule under their old fields, a note, a signed order, and an effect on
/// the crossing.
fn old_board() -> Fixture {
    let board = Fixture::enabling(&["controls", "boundary", "attention"]);
    config_naming(&board, &["rules", "boundary", "attention"]);
    let rule = json!({
        "$type": "controls/control/v1",
        "requirement": "Name no one; see /controls/sources/s.",
        "purpose": "A test.",
        "rule": {"check": "pattern", "file": "/private/names.txt", "quote": "match-only"},
    });
    let source = json!({
        "$type": "controls/source/v1", "url": "https://example.com/s", "title": "S",
        "read_on": "2026-11-01", "why": "It says why.",
    });
    let boundary = json!({
        "$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": ["github.pr-*"],
        "checks": [{"control": "/controls/x/a", "parts": ["*"]}],
    });
    let crossing = json!({
        "$type": "boundary/crossing/v1",
        "digest": format!("sha256:{}", "a".repeat(64)),
        "action": "github.pr-create", "to": "github:octo/board",
        "boundaries": ["/boundaries/octo"],
        "verdicts": [{"control": "/controls/x/a", "rule": "pattern", "part": "pr.body", "status": "passed", "findings": []}],
        "result": "passed",
    });
    board
        .write("private/names.txt", "acme\n")
        .write("controls/x/a.json", &rule.to_string())
        .write("controls/sources/s.json", &source.to_string())
        .write("boundaries/octo.json", &boundary.to_string())
        .write(
            &format!("crossings/{}.json", "a".repeat(64)),
            &crossing.to_string(),
        )
        .write("notes/n.md", NOTE)
        .write("orders/o.json", &order())
        .write("orders/o.md", ORDER_CONTEXT)
        .write("effects/pending/e.json", &effect());
    board
}

fn rename(board: &Fixture, flags: &[&str]) -> Output {
    let arguments: Vec<&str> = ["--json", "call", "rules.migrate", "--rename", "true"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    board.run(&arguments)
}

/// The dry run's output.
fn dry_run(board: &Fixture) -> Value {
    let ran = rename(board, &[]);
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    json(&ran)["data"]["output"].clone()
}

fn applied(board: &Fixture, digest: &str) -> Output {
    rename(board, &["--apply", "true", "--digest", digest])
}

#[test]
fn renaming_moves_rules_renames_fields_rewrites_mentions_and_leaves_signed_records() {
    let board = old_board();
    assert_eq!(
        board.checked("/controls/"),
        codes(&[("/controls/x/a", None), ("/controls/sources/s", None)])
    );
    let listed = dry_run(&board);
    assert_eq!(listed["applied"], false);
    assert!(
        board
            .read("controls/x/a.json")
            .contains("controls/control/v1")
    );
    let moved: Vec<(&str, &str)> = (listed["writes"].as_array().unwrap().iter())
        .filter_map(|each| Some((each["path"].as_str()?, each["to"].as_str()?)))
        .collect();
    assert_eq!(
        moved,
        [
            ("/controls/sources/s", "/rules/sources/s"),
            ("/controls/x/a", "/rules/x/a")
        ]
    );
    assert_eq!(listed["signed"], json!(["/effects/pending/e", "/orders/o"]));
    assert_eq!(listed["kept"], json!([]));
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));

    let rule: Value = serde_json::from_str(&board.read("rules/x/a.json")).unwrap();
    assert_eq!(rule["$type"], "rules/rule/v1");
    assert_eq!(rule["requirement"], "Name no one; see /rules/sources/s.");
    assert_eq!(rule["checker"]["check"], "pattern");
    assert_eq!(rule.get("rule"), None);
    let source: Value = serde_json::from_str(&board.read("rules/sources/s.json")).unwrap();
    assert_eq!(source["$type"], "rules/source/v1");
    let boundary: Value = serde_json::from_str(&board.read("boundaries/octo.json")).unwrap();
    assert_eq!(
        boundary["checks"],
        json!([{"rule": "/rules/x/a", "parts": ["*"]}])
    );
    let crossing: Value =
        serde_json::from_str(&board.read(&format!("crossings/{}.json", "a".repeat(64)))).unwrap();
    assert_eq!(
        (
            &crossing["verdicts"][0]["rule"],
            &crossing["verdicts"][0]["checker"]
        ),
        (&json!("/rules/x/a"), &json!("pattern"))
    );
    assert_eq!(
        board.read("notes/n.md"),
        "# A note\n\nRun `board call rules.check`, then read /rules/x/a and /repos/board/rules/agents.\n"
    );
    assert_eq!(board.read("orders/o.json"), order());
    assert_eq!(board.read("orders/o.md"), ORDER_CONTEXT);
    assert_eq!(board.read("effects/pending/e.json"), effect());

    let whole: BTreeMap<String, Option<String>> = (board.checked("/").into_iter())
        .filter(|(path, code)| {
            code.is_some() && !path.starts_with("/orders/") && !path.starts_with("/effects/")
        })
        .collect();
    assert_eq!(whole, BTreeMap::new());
    let again = dry_run(&board);
    assert_eq!(
        (&again["writes"], &again["total"]),
        (&json!([]), &json!(0)),
        "{again}"
    );
}

#[test]
fn a_reference_renamed_into_a_mounted_board_not_yet_migrated_resolves_through_the_moved_prefix() {
    let board = old_board();
    let rule = control("Elsewhere.", &json!({}));
    let (other, commit) = repository(&["rules"], &[("controls/x/b.json", rule)]);
    mount(&board, "repos/other.json", &other, commit);
    let boundary = json!({
        "$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": ["github.pr-*"],
        "checks": [{"control": "/repos/other/controls/x/b", "parts": ["*"]}],
    });
    board.write("boundaries/other.json", &boundary.to_string());
    let ran = applied(&board, dry_run(&board)["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let renamed: Value = serde_json::from_str(&board.read("boundaries/other.json")).unwrap();
    assert_eq!(renamed["checks"][0]["rule"], "/repos/other/rules/x/b");
    assert_eq!(
        board.checked("/boundaries/"),
        codes(&[("/boundaries/octo", None), ("/boundaries/other", None)])
    );
}

#[test]
fn a_mount_pointer_under_controls_is_kept_and_a_reference_renamed_through_it_still_resolves() {
    let board = old_board();
    let (other, commit) = repository(&["rules"], &[("x.json", control("There.", &json!({})))]);
    mount(&board, "controls/auth.json", &other, commit);
    let boundary = json!({
        "$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": ["github.pr-*"],
        "checks": [{"control": "/controls/auth/x", "parts": ["*"]}],
    });
    board.write("boundaries/auth.json", &boundary.to_string());
    let listed = dry_run(&board);
    let kept: Vec<&str> = (listed["kept"].as_array().unwrap().iter())
        .filter_map(|each| each["path"].as_str())
        .collect();
    assert_eq!(kept, ["/controls/auth"], "{listed}");
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let renamed: Value = serde_json::from_str(&board.read("boundaries/auth.json")).unwrap();
    assert_eq!(renamed["checks"][0]["rule"], "/rules/auth/x");
    assert!(board.read("controls/auth.json").contains("board/mount/v1"));
    assert_eq!(
        board.checked("/boundaries/auth"),
        codes(&[("/boundaries/auth", None)])
    );
}

/// The bytes of a PNG's signature and more, which are not UTF-8 and hold no
/// NUL.
const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0xff, 0xfe, b'\n'];

#[test]
fn a_file_that_cannot_move_as_text_and_the_renaming_change_are_kept_as_they_are() {
    let board = old_board();
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture holds a file that is not UTF-8, which Fixture::write cannot write"
    )]
    std::fs::write(board.board.workdir().join("controls/x/img.png"), PNG).unwrap();
    board
        .write(
            "controls/x/broken.json",
            "{\"$type\": \"controls/control/v1\",",
        )
        .write("changes/control-becomes-rule/details.md", NOTE);
    let listed = dry_run(&board);
    let kept: Vec<&str> = (listed["kept"].as_array().unwrap().iter())
        .filter_map(|each| each["path"].as_str())
        .collect();
    assert_eq!(
        kept,
        [
            "/changes/control-becomes-rule/details.md",
            "/controls/x/broken",
            "/controls/x/img.png"
        ],
        "{listed}"
    );
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let bytes = std::fs::read(board.board.workdir().join("controls/x/img.png")).unwrap();
    assert_eq!(bytes, PNG);
    assert!(!board.board.workdir().join("rules/x/img.png").exists());
    assert_eq!(
        board.read("controls/x/broken.json"),
        "{\"$type\": \"controls/control/v1\","
    );
    assert_eq!(board.read("changes/control-becomes-rule/details.md"), NOTE);
    assert!(board.read("rules/x/a.json").contains("rules/rule/v1"));
}

#[test]
fn the_records_and_files_of_the_continuing_change_keep_the_names_they_were_written_in() {
    let board = old_board();
    let record = json!({
        "$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": ["github.pr-*"],
        "checks": [{"control": "/controls/x/a", "parts": ["*"]}],
    })
    .to_string();
    board
        .write("changes/rule-rename-rest/procedure.json", &record)
        .write("changes/rule-rename-rest/procedure.md", NOTE);
    let listed = dry_run(&board);
    let kept: Vec<&str> = (listed["kept"].as_array().unwrap().iter())
        .filter_map(|each| each["path"].as_str())
        .collect();
    assert!(
        kept.contains(&"/changes/rule-rename-rest/procedure"),
        "{listed}"
    );
    assert!(
        kept.contains(&"/changes/rule-rename-rest/procedure.md"),
        "{listed}"
    );
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    assert_eq!(board.read("changes/rule-rename-rest/procedure.md"), NOTE);
    assert_eq!(
        board.read("changes/rule-rename-rest/procedure.json"),
        record
    );
    let again = dry_run(&board);
    assert_eq!(
        (&again["writes"], &again["total"]),
        (&json!([]), &json!(0)),
        "{again}"
    );
}

#[test]
fn applying_after_a_record_changed_since_the_dry_run_refuses_and_writes_nothing() {
    let board = old_board();
    let listed = dry_run(&board);
    board.write("notes/n.md", &format!("{NOTE}More about /controls/x/a.\n"));
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_ne!(ran.code, 0, "{}", text(&ran.stdout));
    assert_eq!(json(&ran)["code"], "rules::migrate::stale");
    assert!(
        board
            .read("controls/x/a.json")
            .contains("controls/control/v1")
    );
    assert!(board.read("notes/n.md").contains("/controls/x/a"));
}

#[test]
fn the_dry_run_refuses_a_record_whose_type_has_no_schema_as_the_apply_would() {
    let board = old_board();
    let page = json!({"$type": "sites/page/v1", "title": "See /controls/x/a."}).to_string();
    board.write("sites/p.json", &page);
    let dry = rename(&board, &[]);
    assert_ne!(dry.code, 0, "{}", text(&dry.stdout));
    let shown = text(&dry.stdout);
    assert!(
        shown.contains("/sites/p") && shown.contains("sites/page/v1"),
        "{shown}"
    );
    assert_eq!(json(&dry)["code"], "rules::board::refused", "{shown}");
    assert_eq!(
        json(&dry)["fields"]["code"],
        "board::put::invalid",
        "{shown}"
    );
    assert!(
        board
            .read("controls/x/a.json")
            .contains("controls/control/v1")
    );
    assert_eq!(board.read("sites/p.json"), page);
}

/// A source that mentions an old path, so the rename rewrites it, with
/// `extra` fields.
fn source_with(extra: &Value) -> String {
    let mut source = json!({
        "$type": "controls/source/v1", "url": "https://example.com/t", "title": "T",
        "read_on": "2026-11-01", "why": "See /controls/x/a.",
    });
    source
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    source.to_string()
}

#[test]
fn the_dry_run_refuses_a_rewritten_record_its_schema_does_not_admit_and_passes_one_it_does() {
    let board = old_board();
    board.write(
        "controls/sources/bad.json",
        &source_with(&json!({"extra": 1})),
    );
    let dry = rename(&board, &[]);
    assert_ne!(dry.code, 0, "{}", text(&dry.stdout));
    assert!(
        text(&dry.stdout).contains("/rules/sources/bad"),
        "{}",
        text(&dry.stdout)
    );
    board.write("controls/sources/bad.json", &source_with(&json!({})));
    let listed = dry_run(&board);
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
}

#[test]
fn the_dry_run_refuses_a_move_whose_target_file_is_taken_as_the_apply_would() {
    let board = old_board();
    board.write("controls/x/notes.txt", "see /controls/x/a\n");
    board.write("rules/x/notes.txt", "already here\n");
    let dry = rename(&board, &[]);
    assert_ne!(dry.code, 0, "{}", text(&dry.stdout));
    assert!(
        text(&dry.stdout).contains("/rules/x/notes.txt"),
        "{}",
        text(&dry.stdout)
    );
}

/// The config of `board`, naming `plugins`.
fn config_naming(board: &Fixture, plugins: &[&str]) {
    let mut config: Value = serde_json::from_str(&board.read(".board/board.json")).unwrap();
    config["plugins"] = json!(plugins);
    board.write(".board/board.json", &config.to_string());
}

#[test]
fn the_rename_is_refused_until_the_config_names_rules_and_then_applies() {
    let board = old_board();
    config_naming(&board, &["controls", "boundary", "attention"]);
    let before = board.read("controls/x/a.json");
    for flags in [
        &[][..],
        &["--apply", "true", "--digest", &"0".repeat(64)][..],
    ] {
        let ran = rename(&board, flags);
        assert_ne!(ran.code, 0, "{}", text(&ran.stdout));
        assert_eq!(json(&ran)["code"], "rules::migrate::config_first");
    }
    assert_eq!(board.read("controls/x/a.json"), before);
    config_naming(&board, &["rules", "boundary", "attention"]);
    let listed = dry_run(&board);
    let ran = applied(&board, listed["digest"].as_str().unwrap());
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    assert!(board.read("rules/x/a.json").contains("rules/rule/v1"));
}
