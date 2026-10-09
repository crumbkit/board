//! bash and zsh completion run as each shell's script runs it: the binary
//! with `BOARD_COMPLETE`, `_CLAP_COMPLETE_INDEX` and `_CLAP_IFS` set.
//! `clap_complete` reads the last two from the environment itself, so only
//! a child process proves what these shells are offered.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "only a child process can be given clap_complete's environment"
)]

use std::path::Path;
use std::process::Command;

use board_scenario::{Item, Scenario};

/// What `shell` is offered, from `directory`, for the command line `words`
/// with its cursor on the word at `cursor`, counting `board` as 0, as the
/// bytes its script splits on `separator`.
fn offered(
    directory: &Path,
    shell: &str,
    separator: &str,
    cursor: usize,
    words: &[&str],
) -> String {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_board"))
        .args(["--", "board"])
        .args(words)
        .current_dir(directory)
        .env_clear()
        .env("HOME", home.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("BOARD_COMPLETE", shell)
        .env("_CLAP_COMPLETE_INDEX", cursor.to_string())
        .env("_CLAP_IFS", separator)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

fn bash(directory: &Path, cursor: usize, words: &[&str]) -> String {
    offered(directory, "bash", "\u{b}", cursor, words)
}

fn board(directory: &str) -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(&format!("{directory}/a.json"), Item::File("{}"));
    scenario
}

#[test]
fn bash_is_offered_a_path_with_its_space_escaped() {
    let scenario = board("with space");
    let directory = scenario.workdir();
    assert_eq!(bash(directory, 2, &["get", r"/with\ sp"]), r"/with\ space/");
    assert_eq!(bash(directory, 2, &["get", r"/with\ zz"]), "");
}

#[test]
fn zsh_is_offered_a_path_with_its_colon_escaped() {
    let scenario = board("a:b");
    let directory = scenario.workdir();
    assert_eq!(
        offered(directory, "zsh", "\n", 2, &["get", "/a:"]),
        r"/a\:b/"
    );
    assert_eq!(offered(directory, "zsh", "\n", 2, &["get", "/a;"]), "");
}

#[test]
fn bash_applies_only_the_dash_c_before_its_cursor() {
    let scenario = board("tasks");
    let directory = scenario.workdir();
    // A word follows each `-C` value, so the value is not the last word,
    // which is all that would keep it out without the cursor.
    let after = ["get", "/ta", "-C", "/does/not/exist", "--json"];
    assert_eq!(bash(directory, 2, &after), "/tasks/");
    let before = ["-C", "/does/not/exist", "get", "/ta", "--json"];
    assert_eq!(bash(directory, 4, &before), "");
}

#[test]
fn bash_unquotes_a_dash_c_it_sends_as_typed() {
    let scenario = board("tasks");
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(scenario.workdir(), outside.path().join("it's")).unwrap();
    let typed = |directory| ["-C", directory, "get", "/ta"];
    assert_eq!(bash(outside.path(), 4, &typed(r"it\'s")), "/tasks/");
    assert_eq!(bash(outside.path(), 4, &typed(r"it\'z")), "");
}
