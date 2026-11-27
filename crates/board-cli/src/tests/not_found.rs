//! A refusal for a path that names nothing lists the nearest entries in its
//! deepest ancestor that resolves.

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{answers, board, board_in, hint_of, text};

const DECISIONS: &str = "projects/board/decisions";

/// A board whose decisions directory holds two stamped records, a slugged
/// record and a Markdown file.
fn decisions() -> Scenario {
    let scenario = board();
    for name in [
        "20261127T1947Z-95a5.json",
        "20261127T2010Z-0c1d.json",
        "a-refusal-for-a-path-that-holds-nothing.json",
    ] {
        scenario.write(
            &format!("{DECISIONS}/{name}"),
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    }
    scenario.write(&format!("{DECISIONS}/notes.md"), Item::File("# n\n"));
    scenario
}

/// The one failure of `board get <path> --json` in `scenario`.
fn refused(scenario: &Scenario, path: &str) -> Value {
    let ran = board_in(scenario.workdir(), &["get", path, "--json"]);
    assert_eq!(
        (ran.code, ran.error.as_deref()),
        (1, Some("board::path::not_found")),
        "{}",
        text(&ran.stdout)
    );
    answers(&ran)[0].clone()
}

#[test]
fn a_missing_path_a_few_characters_off_lists_the_entry_it_meant_first() {
    let scenario = decisions();
    let cases = [
        (
            "a-refusal-for-a-path",
            "a-refusal-for-a-path-that-holds-nothing",
        ),
        (
            "a-refusal-for-a-path-that-holds-nothing-names",
            "a-refusal-for-a-path-that-holds-nothing",
        ),
        ("20261127T1948Z-95a5", "20261127T1947Z-95a5"),
        ("notes", "notes.md"),
    ];
    for (missed, meant) in cases {
        let failure = refused(&scenario, &format!("/{DECISIONS}/{missed}"));
        let nearest = failure["fields"]["nearest"].as_array().unwrap();
        assert_eq!(nearest[0], json!(meant), "{missed}: {failure}");
        assert!(nearest.len() <= 5, "{failure}");
        assert_eq!(
            failure["fields"]["ancestor"],
            json!(format!("/{DECISIONS}"))
        );
        let twin = board_in(
            scenario.workdir(),
            &["get", &format!("/{DECISIONS}/{meant}")],
        );
        assert_eq!(twin.code, 0, "{}", text(&twin.stderr));
    }
}

#[test]
fn the_hint_names_the_nearest_entries_and_the_listing_of_all() {
    let scenario = decisions();
    let missed = board_in(
        scenario.workdir(),
        &["get", &format!("/{DECISIONS}/20261127T1948Z-95a5")],
    );
    assert_eq!(missed.error.as_deref(), Some("board::path::not_found"));
    assert_eq!(
        hint_of(&missed),
        format!(
            "nearest entries in /{DECISIONS}: 20261127T1947Z-95a5, 20261127T2010Z-0c1d; run `board ls /{DECISIONS}` for all"
        )
    );
    let listed = board_in(scenario.workdir(), &["ls", &format!("/{DECISIONS}/note")]);
    assert_eq!(listed.error.as_deref(), Some("board::path::not_found"));
    assert_eq!(
        hint_of(&listed),
        format!("nearest entries in /{DECISIONS}: notes.md; run `board ls /{DECISIONS}` for all")
    );
}

#[test]
fn a_missing_path_with_no_close_name_offers_none() {
    let scenario = decisions();
    let failure = refused(&scenario, &format!("/{DECISIONS}/qqqq"));
    assert_eq!(failure["fields"].get("nearest"), None, "{failure}");
    assert_eq!(
        failure["recovery"]["text"],
        json!(format!(
            "run `board ls /{DECISIONS}` to see what is there; paths leave out `.json`"
        ))
    );
}

#[test]
fn a_directory_over_the_entry_cap_offers_no_names() {
    let scenario = board();
    for number in 0..=board_tree::MAX_NEAREST_LISTED {
        scenario.write(&format!("crowd/entry-{number}.md"), Item::File(""));
    }
    let failure = refused(&scenario, "/crowd/entry-1");
    assert_eq!(failure["fields"].get("nearest"), None, "{failure}");
    assert_eq!(
        failure["recovery"]["text"],
        json!("run `board ls /crowd` to see what is there; paths leave out `.json`")
    );
}
