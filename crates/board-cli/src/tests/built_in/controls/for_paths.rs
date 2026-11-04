//! `controls.for-paths` through the binary: the controls that apply to any
//! of several files and patterns, each listed once with what it matched.

use super::*;

/// The run of `board --json call controls.for-paths` with `paths` on its
/// standard input, and its one document.
fn for_paths(board: &Fixture, paths: &[&str]) -> (Output, Value) {
    let stdin = json!({ "paths": paths }).to_string();
    let ran = board.run_with(&["--json", "call", "controls.for-paths"], &stdin);
    let document = json(&ran);
    (ran, document)
}

/// Each control `controls.for-paths` returns for `paths`, with the paths it
/// matched.
fn matched(board: &Fixture, paths: &[&str]) -> Vec<(String, Vec<String>)> {
    let (ran, document) = for_paths(board, paths);
    assert_eq!(ran.code, 0, "{document}");
    (document["data"]["output"]["controls"]
        .as_array()
        .unwrap()
        .iter())
    .map(|item| {
        let matched = (item["matched"].as_array().unwrap().iter())
            .map(|path| path.as_str().unwrap().to_owned())
            .collect();
        (item["control"].as_str().unwrap().to_owned(), matched)
    })
    .collect()
}

/// `(control, matched)` pairs as owned strings.
fn pairs(expected: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    (expected.iter())
        .map(|(control, paths)| {
            let paths = paths.iter().map(|path| (*path).to_owned()).collect();
            ((*control).to_owned(), paths)
        })
        .collect()
}

#[test]
fn a_pattern_reaches_the_controls_of_the_files_it_matches_and_each_control_is_listed_once() {
    let board = board();
    board
        .write("crates/store/src/lib.rs", "//! Store.\n")
        .write("crates/store/Cargo.toml", "[package]\n")
        .write("docs/a.md", "A.\n")
        .write(
            "controls/rust.json",
            &control("Typed errors.", &json!({"paths": ["crates/*.rs"]})),
        )
        .write(
            "controls/manifests.json",
            &control("Few features.", &json!({"paths": ["crates/*/Cargo.toml"]})),
        )
        .write(
            "controls/docs.json",
            &control("Say what exists.", &json!({"paths": ["docs/*.md"]})),
        );
    assert_eq!(
        matched(&board, &["/crates/store/**", "/crates/store/src/lib.rs"]),
        pairs(&[
            ("/controls/manifests", &["/crates/store/**"]),
            (
                "/controls/rust",
                &["/crates/store/**", "/crates/store/src/lib.rs"]
            ),
        ])
    );
}

#[test]
fn a_file_path_is_matched_as_for_path_matches_it_through_a_link_and_before_it_exists() {
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
    assert_eq!(
        matched(&board, &["/alias/a.md", "/docs/new.md", "/docs/new.txt"]),
        pairs(&[("/controls/docs", &["/alias/a.md", "/docs/new.md"])])
    );
}

#[test]
fn a_pattern_that_matches_no_file_yet_is_matched_as_a_path_and_one_that_cannot_be_is_not() {
    let board = board();
    board.write(
        "controls/rust.json",
        &control("R.", &json!({"paths": ["crates/*.rs"]})),
    );
    assert_eq!(
        matched(&board, &["/crates/new/src/*.rs", "/crates/new/**"]),
        pairs(&[("/controls/rust", &["/crates/new/src/*.rs"])])
    );
}

#[test]
fn a_pattern_reaches_a_control_whose_applies_to_it_matches_and_not_one_it_does_not() {
    let board = board();
    board
        .write("AGENTS.md", "Rules.\n")
        .write("docs/a.md", "A.\n")
        .write(
            "controls/agents.json",
            &control("Stay small.", &json!({"applies_to": ["/AGENTS.md"]})),
        )
        .write(
            "controls/docs.json",
            &control("R.", &json!({"applies_to": ["/docs/a.md"]})),
        );
    assert_eq!(
        matched(&board, &["/docs/**"]),
        pairs(&[("/controls/docs", &["/docs/**"])])
    );
}

#[test]
fn a_pattern_into_a_mount_reaches_the_controls_of_the_mounted_files() {
    let board = board();
    let _library = library(
        &board,
        "repos/lib.json",
        &[("src/lib.rs", "//! L.\n"), ("README.md", "L.\n")],
    );
    board
        .write(
            "controls/rust.json",
            &control("R.", &json!({"paths": ["repos/lib/*.rs"]})),
        )
        .write(
            "controls/docs.json",
            &control("R.", &json!({"paths": ["repos/lib/*.md"]})),
        );
    assert_eq!(
        matched(&board, &["/repos/lib/src/**"]),
        pairs(&[("/controls/rust", &["/repos/lib/src/**"])])
    );
}

#[test]
fn each_control_carries_its_summary_or_else_its_requirements_first_sentence() {
    let board = board();
    board
        .write("docs/a.md", "A.\n")
        .write(
            "controls/long.json",
            &control(
                "Say what exists, and say it once.",
                &json!({"paths": ["docs/*"], "summary": "Say what exists."}),
            ),
        )
        .write(
            "controls/short.json",
            &control("Be kind. Always.", &json!({"paths": ["docs/*"]})),
        );
    let (_, document) = for_paths(&board, &["/docs/a.md"]);
    assert_eq!(
        document["data"]["output"]["controls"],
        json!([
            {
                "control": "/controls/long",
                "summary": "Say what exists.",
                "matched": ["/docs/a.md"],
            },
            {
                "control": "/controls/short",
                "summary": "Be kind.",
                "matched": ["/docs/a.md"],
            },
        ])
    );
}

#[test]
fn no_paths_are_refused_and_one_path_is_answered() {
    let board = board();
    let (refused, document) = for_paths(&board, &[]);
    assert_ne!(refused.code, 0, "{document}");
    assert_eq!(
        document["code"],
        json!("board::plugin::bad_input"),
        "{document}"
    );
    let (admitted, document) = for_paths(&board, &["/docs/a.md"]);
    assert_eq!(admitted.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["controls"], json!([]));
}
