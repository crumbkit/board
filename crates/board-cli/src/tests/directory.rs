//! `-C <dir>`: every command runs as if Board were started in `<dir>`, as
//! `git -C` does.

use super::*;

const HINT: &str = "give -C a directory that exists, such as the board's root or a directory \
                    inside it; a relative -C is taken from the one before it";

/// A git repository that is not a board, to start from.
fn elsewhere() -> Scenario {
    Scenario::new()
}

fn path(directory: &Path) -> &str {
    directory.to_str().unwrap()
}

#[test]
fn a_command_runs_in_the_directory_named_and_without_it_finds_no_board() {
    let scenario = board();
    let outside = elsewhere();
    let lost = board_in(outside.workdir(), &["get", "/projects/p"]);
    assert_eq!(lost.code, 1);
    assert!(
        text(&lost.stderr).ends_with("  code:  board::config::not_found\n"),
        "{}",
        text(&lost.stderr)
    );
    assert_eq!(
        hint_of(&lost),
        "a board needs `.board/board.json` at its repository root, spelled exactly so; \
         in a git repository, `board init` makes it a board; outside one, run `git init` \
         first; for a board elsewhere, run `board -C <dir> check`"
    );
    let found = board_in(
        outside.workdir(),
        &["-C", path(scenario.workdir()), "get", "/projects/p"],
    );
    assert_eq!(text(&found.stderr), "");
    assert_eq!(found.code, 0);
    assert_eq!(text(&found.stdout), r#"{"$type": "lr/project/v1"}"#);
}

#[test]
fn several_directories_combine_and_each_relative_one_is_taken_from_the_one_before() {
    let scenario = board();
    scenario.write(
        "projects/sub/q.json",
        Item::File(r#"{"$type": "lr/project/v1"}"#),
    );
    let outside = elsewhere();
    let parent = scenario.workdir().parent().unwrap();
    let name = scenario.workdir().file_name().unwrap().to_str().unwrap();
    for directories in [
        vec![path(parent), name],
        vec![path(scenario.workdir()), "projects", "sub", "../.."],
        vec![path(outside.workdir()), path(scenario.workdir())],
        vec![path(scenario.workdir()), ""],
    ] {
        let mut arguments: Vec<&str> = directories.iter().flat_map(|d| ["-C", d]).collect();
        arguments.extend(["ls", "/projects"]);
        let listed = board_in(outside.workdir(), &arguments);
        assert_eq!(listed.code, 0, "{directories:?}: {}", text(&listed.stderr));
    }
    let relative = board_in(
        scenario.workdir(),
        &["-C", "projects", "-C", "sub", "check"],
    );
    assert_eq!(relative.code, 0, "{}", text(&relative.stderr));
}

#[test]
fn a_directory_that_does_not_exist_is_refused_and_one_that_exists_is_admitted() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["-C", "nowhere", "check"]);
    assert_eq!(refused.code, 2);
    assert!(refused.stdout.is_empty());
    assert_eq!(
        text(&refused.stderr),
        format!(
            "error: cannot change to 'nowhere': {} does not exist\n  help:  {HINT}\n\
             \x20 code:  board::cli::usage\n",
            scenario.workdir().join("nowhere").display()
        )
    );
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn a_file_named_as_the_directory_is_refused_and_its_directory_is_admitted() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["-C", "projects/p.json", "check"]);
    assert_eq!(refused.code, 2);
    let stderr = text(&refused.stderr);
    assert!(
        stderr.starts_with("error: cannot change to 'projects/p.json': "),
        "{stderr}"
    );
    assert!(
        stderr.ends_with(&format!(
            "p.json is not a directory\n  help:  {HINT}\n  code:  board::cli::usage\n"
        )),
        "{stderr}"
    );
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "check"]);
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_refused_directory_is_an_error_document_with_json_and_an_admitted_one_is_a_report() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["-C", "nowhere", "--json", "check"]);
    assert_eq!(refused.code, 2);
    assert!(refused.stderr.is_empty());
    let error = json(&refused);
    assert_eq!(error["schema"], "board/failure/v1");
    assert_eq!(error["code"], "board::cli::usage");
    assert_eq!(
        error["message"],
        format!(
            "cannot change to 'nowhere': {} does not exist",
            scenario.workdir().join("nowhere").display()
        )
    );
    assert_eq!(error["recovery"]["text"], HINT);
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "--json", "check"]);
    assert_eq!(admitted.code, 0);
    assert_eq!(json(&admitted)["schema"], "board/check-report/v2");
}

#[test]
fn init_makes_the_named_directory_a_board() {
    let outside = elsewhere();
    let start = elsewhere();
    let made = board_in(start.workdir(), &["-C", path(outside.workdir()), "init"]);
    assert_eq!(made.code, 0, "{}", text(&made.stderr));
    assert!(outside.workdir().join(".board/board.json").is_file());
    assert!(!start.workdir().join(".board").exists());
}

#[test]
fn a_directory_after_the_command_is_refused_and_before_it_is_admitted() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["check", "-C", "projects"]);
    assert_eq!(refused.code, 2);
    assert!(
        text(&refused.stderr).starts_with("error: unexpected argument '-C' found"),
        "{}",
        text(&refused.stderr)
    );
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "check"]);
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_group_without_its_command_after_a_directory_points_at_the_groups_help() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["-C", "projects", "plugin"]);
    assert_eq!(refused.code, 2);
    assert!(
        text(&refused.stderr).contains("\n  help:  run `board plugin -h`\n"),
        "{}",
        text(&refused.stderr)
    );
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "plugin", "list"]);
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_path_through_a_file_or_a_symlink_loop_is_refused_and_a_directory_is_admitted() {
    let scenario = board();
    let through = board_in(scenario.workdir(), &["-C", "projects/p.json/sub", "check"]);
    assert_eq!(through.code, 2);
    assert_eq!(
        text(&through.stderr),
        format!(
            "error: cannot change to 'projects/p.json/sub': {} is not a \
             directory\n  help:  {HINT}\n  code:  board::cli::usage\n",
            scenario.workdir().join("projects/p.json/sub").display()
        )
    );
    std::os::unix::fs::symlink("loop", scenario.workdir().join("loop")).unwrap();
    let looped = board_in(scenario.workdir(), &["-C", "loop", "check"]);
    assert_eq!(looped.code, 2);
    let stderr = text(&looped.stderr);
    assert!(
        stderr.starts_with(&format!(
            "error: cannot change to 'loop': {} cannot be read: ",
            scenario.workdir().join("loop").display()
        )),
        "{stderr}"
    );
    let admitted = board_in(scenario.workdir(), &["-C", "projects", "check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn a_directory_that_is_not_utf8_still_points_a_groups_hint_at_its_help() {
    use std::os::unix::ffi::OsStringExt;
    let scenario = board();
    let context = Context {
        start: scenario.workdir().to_path_buf(),
        home: None,
        cache: None,
        plugins: None,
        environment: board_plugin::Environment::default(),
        complete: None,
        cursor: None,
    };
    let arguments = [
        OsString::from(BINARY_NAME),
        OsString::from("-C"),
        OsString::from_vec(vec![0xff]),
        OsString::from("plugin"),
    ];
    let refused = run(arguments, &context, &mut &b""[..], &mut Ran::default());
    assert_eq!(refused.code, 2);
    assert!(
        text(&refused.stderr).contains("\n  help:  run `board plugin -h`\n"),
        "{}",
        text(&refused.stderr)
    );
    let utf8 = board_in(scenario.workdir(), &["-C", "projects", "plugin"]);
    assert!(text(&utf8.stderr).contains("\n  help:  run `board plugin -h`\n"));
}

#[test]
fn a_long_listing_outside_a_board_fails_as_not_found_and_inside_one_lists() {
    let scenario = board();
    let outside = elsewhere();
    let arguments = ["--json", "--long", "plugin", "list"];
    let lost = board_in(outside.workdir(), &arguments);
    assert_eq!(lost.code, 1, "{}", text(&lost.stdout));
    assert!(lost.stderr.is_empty(), "{}", text(&lost.stderr));
    let error = json(&lost);
    assert_eq!(error["schema"], "board/failure/v1");
    assert_eq!(error["code"], "board::config::not_found");
    let found = board_in(scenario.workdir(), &arguments);
    assert_eq!(found.code, 0, "{}", text(&found.stdout));
    assert_eq!(json(&found)["schema"], "board/plugin-list-report/v2");
}
