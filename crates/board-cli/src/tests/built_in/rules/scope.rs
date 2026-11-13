//! `controls.check` under a path measures only the files under it: a
//! control spanning several mounts runs on the files of the mount asked,
//! and a breach or an unreadable file in another mount is not that mount's.

use super::*;

/// A board whose two controls span every mount under /repos: a version
/// rule on each `rust-toolchain.toml` and a budget of three words on each
/// README. /repos/new is within both and /repos/old breaches both. The
/// repositories live as long as the value returned.
fn spanning() -> (Fixture, [(Scenario, ObjectId); 2]) {
    let board = board();
    board
        .write(
            "controls/rust.json",
            &control(
                "Pin the latest Rust.",
                &json!({
                    "paths": ["repos/*/rust-toolchain.toml"],
                    "rule": {"check": "version", "at_least": "1.99.1", "capture": r#"channel = "([0-9.]+)""#},
                }),
            ),
        )
        .write(
            "controls/readme.json",
            &control(
                "Short READMEs.",
                &json!({
                    "paths": ["repos/*/README.md"],
                    "rule": {"check": "budget", "unit": "words", "max": 3},
                }),
            ),
        );
    let new = library(
        &board,
        "repos/new.json",
        &[
            ("README.md", "one two"),
            ("rust-toolchain.toml", "channel = \"1.99.1\"\n"),
        ],
    );
    let old = library(
        &board,
        "repos/old.json",
        &[
            ("README.md", "one two three four five"),
            ("rust-toolchain.toml", "channel = \"1.98.0\"\n"),
        ],
    );
    (board, [new, old])
}

#[test]
fn a_control_spanning_two_mounts_passes_the_mount_within_it_and_reports_the_other_mounts_breach() {
    let (board, _mounts) = spanning();
    let new = board.call("rules.check", &["--path", "/repos/new"]);
    assert_eq!(new["passed"], true, "{new}");
    assert_eq!(
        new["measured"],
        json!([{
            "rule": "/controls/readme",
            "requirement": "Short READMEs.",
            "budget": "words",
            "max": 3,
            "used": 2,
            "files": ["/repos/new/README.md"],
            "unreadable": [],
            "kind": "standing",
            "show": "failing",
        }])
    );
    assert_eq!(
        (&new["versions"][0]["files"], &new["versions"][0]["behind"]),
        (&json!(["/repos/new/rust-toolchain.toml"]), &json!([]))
    );
    let old = board.call("rules.check", &["--path", "/repos/old"]);
    assert_eq!(old["passed"], false, "{old}");
    assert_eq!(
        (&old["measured"][0]["files"], &old["measured"][0]["used"]),
        (&json!(["/repos/old/README.md"]), &json!(5))
    );
    assert_eq!(
        old["versions"][0]["behind"],
        json!([{"file": "/repos/old/rust-toolchain.toml", "line": 1, "found": "1.98.0"}])
    );
}

#[test]
fn a_breach_is_reported_as_data_and_the_call_exits_zero_as_a_passing_one_does() {
    let (board, _mounts) = spanning();
    for (path, passed) in [("/repos/new", true), ("/repos/old", false), ("/", false)] {
        let ran = board.run(&["--json", "call", "rules.check", "--path", path]);
        let report = json(&ran);
        assert_eq!(ran.code, 0, "{path}: {report}");
        assert_eq!(
            report["data"]["output"]["passed"], passed,
            "{path}: {report}"
        );
    }
}

/// Takes the blob holding `text` out of `repository`'s object store, as a
/// partial clone leaves a file whose blob it never fetched.
#[expect(
    clippy::disallowed_methods,
    reason = "the test removes a loose object to stand in for a partial clone"
)]
fn forget(repository: &Scenario, text: &str) {
    // Writing a blob that is already stored changes nothing, and gives its id.
    let id = repository.blob(text);
    let hex = id.to_string();
    let object = (repository.workdir())
        .join(".git/objects")
        .join(&hex[..2])
        .join(&hex[2..]);
    std::fs::remove_file(&object).unwrap();
    assert_eq!(repository.stored(id), None);
}

#[test]
fn a_file_missing_from_another_mount_leaves_the_first_mount_passing_and_the_root_reports_it() {
    let (board, mounts) = spanning();
    let [_, (old, _)] = &mounts;
    board.write(
        "controls/rust.json",
        &control("Plain.", &json!({"paths": ["repos/*/rust-toolchain.toml"]})),
    );
    board.write(
        "controls/readme.json",
        &control(
            "Short READMEs.",
            &json!({"paths": ["repos/*/README.md"], "rule": {"check": "budget", "unit": "words", "max": 9}}),
        ),
    );
    assert_eq!(
        board.call("rules.check", &[])["passed"],
        true,
        "both READMEs are readable and within nine words"
    );
    forget(old, "one two three four five");
    let new = board.call("rules.check", &["--path", "/repos/new"]);
    assert_eq!(new["passed"], true, "{new}");
    assert_eq!(new["measured"][0]["unreadable"], json!([]), "{new}");
    let root = board.call("rules.check", &[]);
    assert_eq!(root["passed"], false, "{root}");
    let unreadable = &root["measured"][0]["unreadable"];
    assert_eq!(
        unreadable[0]["file"], "/repos/old/README.md",
        "{unreadable}"
    );
    assert!(unreadable[0]["code"].is_string(), "{unreadable}");
    assert_eq!(root["measured"][0]["used"], 2, "{root}");
}

#[test]
fn a_degraded_file_in_another_mount_leaves_the_first_mount_passing_and_the_root_lists_it() {
    let board = board();
    board.write(
        "controls/docs.json",
        &control(
            "Short docs.",
            &json!({"paths": ["repos/*/docs/*"], "rule": {"check": "budget", "unit": "words", "max": 9}}),
        ),
    );
    let _new = library(&board, "repos/new.json", &[("docs/a", "one")]);
    let _old = library(
        &board,
        "repos/old.json",
        &[("docs/a", "one"), ("docs/a.json", "{}")],
    );
    let new = board.call("rules.check", &["--path", "/repos/new"]);
    assert_eq!(new["passed"], true, "{new}");
    assert_eq!(new["measured"][0]["files"], json!(["/repos/new/docs/a"]));
    let root = board.call("rules.check", &[]);
    assert_eq!(root["passed"], false, "{root}");
    assert_eq!(
        root["measured"][0]["unreadable"][0]["file"], "/repos/old/docs/a",
        "{root}"
    );
}

#[test]
fn a_path_measures_its_own_mount_and_not_a_sibling_whose_name_it_begins() {
    let board = board();
    board.write(
        "controls/readme.json",
        &control(
            "Short READMEs.",
            &json!({"paths": ["repos/*/README.md"], "rule": {"check": "budget", "unit": "words", "max": 3}}),
        ),
    );
    let _new = library(&board, "repos/new.json", &[("README.md", "one two")]);
    let _newer = library(
        &board,
        "repos/newer.json",
        &[("README.md", "one two three four five")],
    );
    let new = board.call("rules.check", &["--path", "/repos/new"]);
    assert_eq!(new["passed"], true, "{new}");
    assert_eq!(new["measured"][0]["files"], json!(["/repos/new/README.md"]));
    let newer = board.call("rules.check", &["--path", "/repos/newer"]);
    assert_eq!(newer["passed"], false, "{newer}");
    assert_eq!(
        newer["measured"][0]["files"],
        json!(["/repos/newer/README.md"])
    );
}
