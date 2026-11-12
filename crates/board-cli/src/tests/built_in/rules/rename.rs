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

const ORDER_CONTEXT: &str = "# Why\n\nSee /controls/x/a.\n";
const NOTE: &str = "# A note\n\nRun `board call controls.check`, then read /controls/x/a and /repos/board/controls/rules/agents.\n";

/// A board enabling the rules plugin under its former name, holding a rule
/// and a source under `/controls/`, a boundary and a crossing that name the
/// rule under their old fields, a note, and a signed order.
fn old_board() -> Fixture {
    let board = Fixture::enabling(&["controls", "boundary", "attention"]);
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
        .write("orders/o.md", ORDER_CONTEXT);
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
    assert_eq!(listed["signed"], json!(["/orders/o"]));
    assert_eq!(listed["kept"][0]["path"], "/.board/board");
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

    let whole: BTreeMap<String, Option<String>> = (board.checked("/").into_iter())
        .filter(|(path, code)| code.is_some() && !path.starts_with("/orders/"))
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
