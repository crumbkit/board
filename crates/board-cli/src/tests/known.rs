//! Known boards: run from a repository that a known board mounts, Board runs
//! as that board, with the directory's logical path as the prefix, as git
//! runs from a subdirectory (docs/architecture.md, Boards and entries).

use super::*;

/// A repository with a commit and a `docs` directory, to mount.
fn library() -> Scenario {
    let library = Scenario::new();
    library.commit(&[("x.json", "1")]);
    library.write("docs/a.md", Item::File("a"));
    library
}

/// A board that mounts `library` at `/repos/lib`.
fn mounting(library: &Scenario) -> Scenario {
    let scenario = board();
    let commit = library.commit(&[("x.json", "1")]);
    let mount = mount_of(&library.path(), commit);
    scenario.write("repos/lib.json", Item::File(&mount));
    scenario
}

/// An XDG config directory whose known boards file holds `text`, or no
/// file when `text` is `None`.
fn config(text: Option<&str>) -> Scenario {
    let xdg = Scenario::new();
    if let Some(text) = text {
        xdg.write("board/config.json", Item::File(text));
    }
    xdg
}

/// The known boards file listing `boards`.
fn listing(boards: &[&Scenario]) -> Scenario {
    let roots: Vec<String> = boards.iter().map(|board| board.path()).collect();
    config(Some(&json!({"boards": roots}).to_string()))
}

/// The known boards file naming `personal` as the personal board.
fn naming(personal: &Scenario) -> Scenario {
    config(Some(&json!({"personal": personal.path()}).to_string()))
}

/// Runs `board` in `directory` with `xdg`'s known boards file and `cache`,
/// and the logical path it started in, which the timing record holds.
fn known(
    directory: &Path,
    xdg: &Scenario,
    cache: Option<&Path>,
    arguments: &[&str],
) -> (Output, Value) {
    fed(directory, xdg, cache, arguments, b"")
}

/// `known`, with `stdin` as standard input.
fn fed(
    directory: &Path,
    xdg: &Scenario,
    cache: Option<&Path>,
    arguments: &[&str],
    stdin: &[u8],
) -> (Output, Value) {
    inside(None, directory, xdg, cache, arguments, stdin)
}

/// `fed`, inside the plugin `plugin` names when it is set.
fn inside(
    plugin: Option<OsString>,
    directory: &Path,
    xdg: &Scenario,
    cache: Option<&Path>,
    arguments: &[&str],
    stdin: &[u8],
) -> (Output, Value) {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: cache.map(Path::to_path_buf),
        plugins: None,
        boards: Some(xdg.workdir().join("board/config.json")),
        signer: None,
        allowed_signers: None,
        environment: board_plugin::Environment {
            plugin,
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    let mut ran = Ran::default();
    let output = run(arguments, &context, &mut &stdin[..], &mut ran);
    let started = std::time::Instant::now();
    let record = crate::timing::record(started, &ran, &output, 0).unwrap();
    (output, record["prefix"].clone())
}

/// A board whose puts do not commit, so a test needs no git identity.
fn uncommitted() -> Scenario {
    let board = board();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    board
}

fn code(output: &Output) -> String {
    let stderr = text(&output.stderr);
    let code = stderr
        .lines()
        .find_map(|line| line.strip_prefix("  code:  "));
    code.unwrap_or_default().to_owned()
}

#[test]
fn a_mounted_repository_runs_as_its_board_and_an_unmounted_one_finds_no_board() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let (found, prefix) = known(library.workdir(), &xdg, None, &["get", "/projects/p"]);
    assert_eq!(text(&found.stderr), "");
    assert_eq!(text(&found.stdout), r#"{"$type": "lr/project/v1"}"#);
    assert_eq!(prefix, "/repos/lib");
    let unmounted = Scenario::new();
    let (lost, prefix) = known(unmounted.workdir(), &xdg, None, &["get", "/projects/p"]);
    assert_eq!(lost.code, 1);
    assert_eq!(code(&lost), "board::config::not_found");
    assert_eq!(prefix, Value::Null);
    let empty = config(None);
    let (lost, _) = known(unmounted.workdir(), &empty, None, &["get", "/projects/p"]);
    assert_eq!(code(&lost), "board::config::not_found");
    let file = empty.workdir().join("board/config.json");
    assert!(
        hint_of(&lost).ends_with(&format!(
            "; to run from a repository a board mounts, list that board in `{}` as \
             {{\"boards\": [\"<board directory>\"]}}",
            file.display()
        )),
        "{}",
        hint_of(&lost)
    );
}

#[test]
fn a_subdirectory_of_a_mounted_repository_is_the_prefix_beneath_the_mount() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let (found, prefix) = known(&library.workdir().join("docs"), &xdg, None, &["check"]);
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
    assert_eq!(prefix, "/repos/lib/docs");
}

#[test]
fn a_linked_worktree_of_a_mounted_repository_runs_as_its_board() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let pinned = library.commit(&[("x.json", "1")]);
    // A linked worktree as `git worktree add --detach` lays it out.
    let linked = format!("{}/linked", library.path());
    let git_dir = format!("{}/.git/worktrees/linked", library.path());
    library
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{pinned}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write("linked/docs/a.md", Item::File("a"));
    let docs = Path::new(&linked).join("docs");
    let (found, prefix) = known(&docs, &xdg, None, &["get", "/projects/p"]);
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
    assert_eq!(prefix, "/repos/lib/docs");
}

#[test]
fn two_known_boards_mounting_the_repository_are_refused_and_one_is_admitted() {
    let library = library();
    let (one, two) = (mounting(&library), mounting(&library));
    let both = listing(&[&one, &two]);
    let (refused, _) = known(library.workdir(), &both, None, &["check"]);
    assert_eq!(refused.code, 1);
    assert_eq!(code(&refused), "board::config::ambiguous");
    let stderr = text(&refused.stderr);
    for board in [&one, &two] {
        let named = format!("{} at /repos/lib", board.path());
        assert!(stderr.contains(&named), "{stderr}");
    }
    let file = both.workdir().join("board/config.json");
    let named = format!("`{}`", file.display());
    assert!(hint_of(&refused).contains(&named), "{}", hint_of(&refused));
    let alone = listing(&[&one]);
    let (admitted, prefix) = known(library.workdir(), &alone, None, &["check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/repos/lib");
}

#[test]
fn a_missing_or_empty_known_boards_file_finds_no_board_and_one_listing_the_board_finds_it() {
    let library = library();
    let board = mounting(&library);
    for xdg in [config(None), config(Some("")), config(Some("{}"))] {
        let (lost, _) = known(library.workdir(), &xdg, None, &["check"]);
        assert_eq!(code(&lost), "board::config::not_found");
    }
    let (found, _) = known(library.workdir(), &listing(&[&board]), None, &["check"]);
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
}

#[test]
fn a_known_boards_file_that_is_not_a_list_is_refused_and_a_list_is_admitted() {
    let library = library();
    let board = mounting(&library);
    let list = json!({"boards": [board.path()]});
    let field = json!({"board": [board.path()]});
    let (refused, _) = known(
        library.workdir(),
        &config(Some(&field.to_string())),
        None,
        &["check"],
    );
    assert_eq!(code(&refused), "board::config::invalid");
    assert!(
        hint_of(&refused).starts_with("write `"),
        "{}",
        hint_of(&refused)
    );
    let (admitted, _) = known(
        library.workdir(),
        &config(Some(&list.to_string())),
        None,
        &["check"],
    );
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn outside_a_repository_the_known_boards_file_is_not_read_and_inside_one_it_is() {
    let library = library();
    let xdg = config(Some("not json"));
    let outside = tempfile::tempdir().unwrap();
    let (lost, _) = known(outside.path(), &xdg, None, &["check"]);
    assert_eq!(code(&lost), "board::config::not_found");
    let (refused, _) = known(library.workdir(), &xdg, None, &["check"]);
    assert_eq!(code(&refused), "board::config::invalid");
    let board = mounting(&library);
    let (admitted, _) = known(library.workdir(), &listing(&[&board]), None, &["check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn a_known_boards_file_that_cannot_be_read_is_refused_and_a_missing_one_lists_none() {
    let library = library();
    let unreadable = Scenario::new();
    unreadable.write("board/config.json/inner", Item::File("a directory"));
    let (refused, _) = known(library.workdir(), &unreadable, None, &["check"]);
    assert_eq!(code(&refused), "board::config::invalid");
    let (missing, _) = known(library.workdir(), &config(None), None, &["check"]);
    assert_eq!(code(&missing), "board::config::not_found");
    let board = mounting(&library);
    let (admitted, _) = known(library.workdir(), &listing(&[&board]), None, &["check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn one_board_listed_under_several_spellings_is_admitted_and_two_boards_are_ambiguous() {
    let library = library();
    let (one, two) = (mounting(&library), mounting(&library));
    let canonical = one.workdir().canonicalize().unwrap();
    let spellings = json!({"boards": [
        one.path(),
        canonical,
        format!("{}/", one.path()),
        one.workdir().join(".board"),
    ]});
    let xdg = config(Some(&spellings.to_string()));
    let (admitted, prefix) = known(library.workdir(), &xdg, None, &["check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/repos/lib");
    let (refused, _) = known(library.workdir(), &listing(&[&one, &two]), None, &["check"]);
    assert_eq!(code(&refused), "board::config::ambiguous");
}

#[test]
fn known_boards_that_do_not_mount_the_repository_are_named_as_read_and_one_that_does_is_admitted() {
    let library = library();
    let board = mounting(&library);
    let nowhere = library.workdir().join("nowhere");
    let xdg = config(Some(
        &json!({"boards": [nowhere, board.path()]}).to_string(),
    ));
    let file = xdg.workdir().join("board/config.json");
    let unmounted = Scenario::new();
    let (lost, _) = known(unmounted.workdir(), &xdg, None, &["check"]);
    assert_eq!(code(&lost), "board::config::not_found");
    let stderr = text(&lost.stderr);
    let read = format!(
        "the known boards in `{}` were read and none mounts this repository",
        file.display()
    );
    assert!(stderr.contains(&read), "{stderr}");
    let skipped = format!("could not be opened or refreshed: {}", nowhere.display());
    assert!(stderr.contains(&skipped), "{stderr}");
    let (opened, _) = known(unmounted.workdir(), &listing(&[&board]), None, &["check"]);
    let stderr = text(&opened.stderr);
    assert!(stderr.contains("none mounts this repository\n"), "{stderr}");
    assert!(
        hint_of(&lost).starts_with("put a `board/mount/v1` record whose `repo` is this repository"),
        "{}",
        hint_of(&lost)
    );
    let (admitted, prefix) = known(library.workdir(), &xdg, None, &["check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/repos/lib");
}

#[test]
fn a_board_that_cannot_be_opened_is_passed_over() {
    let library = library();
    let board = mounting(&library);
    let nowhere = library.workdir().join("nowhere");
    let xdg = config(Some(
        &json!({"boards": [nowhere, board.path()]}).to_string(),
    ));
    let (found, prefix) = known(library.workdir(), &xdg, None, &["check"]);
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
    assert_eq!(prefix, "/repos/lib");
}

#[test]
fn a_directory_given_with_c_inside_a_mounted_repository_sets_the_prefix_and_an_unmounted_one_finds_no_board()
 {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let elsewhere = Scenario::new();
    let docs = library.workdir().join("docs");
    let arguments = ["-C", docs.to_str().unwrap(), "check"];
    let (found, prefix) = known(elsewhere.workdir(), &xdg, None, &arguments);
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
    assert_eq!(prefix, "/repos/lib/docs");
    let unmounted = Scenario::new();
    let path = unmounted.path();
    let arguments = ["-C", path.as_str(), "check"];
    let (lost, _) = known(elsewhere.workdir(), &xdg, None, &arguments);
    assert_eq!(code(&lost), "board::config::not_found");
}

#[test]
fn discovery_answers_from_a_warm_index_as_from_a_cold_one_after_the_mount_moves() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let cache = tempfile::tempdir().unwrap();
    let warm = |arguments: &[&str]| known(library.workdir(), &xdg, Some(cache.path()), arguments);
    let cold = |arguments: &[&str]| known(library.workdir(), &xdg, None, arguments);
    assert_eq!(warm(&["check"]).1, "/repos/lib");
    assert_eq!(warm(&["check"]).1, cold(&["check"]).1);
    let commit = library.commit(&[("x.json", "1")]);
    board.write("repos/lib.json", Item::Deleted).write(
        "vendor/lib.json",
        Item::Fresh(&mount_of(&library.path(), commit)),
    );
    let (warmed, prefix) = warm(&["check"]);
    assert_eq!(warmed.code, 0, "{}", text(&warmed.stderr));
    assert_eq!(prefix, "/vendor/lib");
    assert_eq!(cold(&["check"]).1, prefix);
}

#[test]
fn personal_puts_to_the_personal_board_from_inside_another_board_an_unmounted_repository_or_no_repository()
 {
    let personal = board();
    let uncommitted = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    personal.write(".board/board.json", Item::File(uncommitted));
    let xdg = naming(&personal);
    let (other, unmounted) = (board(), Scenario::new());
    let outside = tempfile::tempdir().unwrap();
    let record = br#"{"$type": "lr/project/v1"}"#;
    let starts = [other.workdir(), unmounted.workdir(), outside.path()];
    for (name, start) in ["a", "b", "c"].into_iter().zip(starts) {
        let arguments = ["--personal", "put", &format!("projects/{name}")];
        let (put, prefix) = fed(start, &xdg, None, &arguments, record);
        assert_eq!(put.code, 0, "{}", text(&put.stderr));
        assert_eq!(prefix, "/");
        let written = personal.workdir().join(format!("projects/{name}.json"));
        assert!(written.is_file(), "{}", written.display());
    }
    assert!(!other.workdir().join("projects/a.json").exists());
    let arguments = ["--personal", "-C", "projects", "ls", "."];
    let (listed, prefix) = known(other.workdir(), &xdg, None, &arguments);
    assert_eq!(listed.code, 0, "{}", text(&listed.stderr));
    assert_eq!(prefix, "/projects");
}

#[test]
fn personal_is_refused_when_the_known_boards_file_names_no_personal_board_and_admitted_when_it_does()
 {
    let personal = board();
    let other = board();
    for xdg in [config(None), listing(&[&personal])] {
        let (refused, _) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
        assert_eq!(refused.code, 1);
        assert_eq!(code(&refused), "board::config::not_found");
        let file = xdg.workdir().join("board/config.json");
        let hint = hint_of(&refused);
        let named = format!("name the personal board in `{}` as", file.display());
        assert!(hint.starts_with(&named), "{hint}");
    }
    let arguments = ["--personal", "check"];
    let (admitted, prefix) = known(other.workdir(), &naming(&personal), None, &arguments);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/");
}

#[test]
fn personal_naming_anything_but_a_boards_root_is_refused_and_the_root_is_admitted() {
    let library = library();
    let work = mounting(&library);
    let missing = work.workdir().join("missing");
    let below = work.workdir().join("projects");
    let other = board();
    for named in [missing.as_path(), library.workdir(), below.as_path()] {
        let known_boards = json!({"boards": [work.path()], "personal": named});
        let xdg = config(Some(&known_boards.to_string()));
        let (refused, prefix) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
        assert_eq!(refused.code, 1, "{}", text(&refused.stdout));
        assert_eq!(code(&refused), "board::config::not_found");
        assert_eq!(prefix, Value::Null);
        let file = xdg.workdir().join("board/config.json");
        let message = format!(
            "`personal` in `{}` names {}, which is not the root of a board",
            file.display(),
            named.display()
        );
        assert!(
            text(&refused.stderr).contains(&message),
            "{}",
            text(&refused.stderr)
        );
        let hint = hint_of(&refused);
        let named = format!("name the personal board in `{}` as", file.display());
        assert!(hint.starts_with(&named), "{hint}");
    }
    let known_boards = json!({"boards": [work.path()], "personal": work.path()});
    let xdg = config(Some(&known_boards.to_string()));
    let (admitted, prefix) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/");
}

#[test]
fn a_personal_board_whose_board_json_is_invalid_is_refused_as_invalid_and_a_valid_one_is_admitted()
{
    let (personal, other) = (board(), board());
    let xdg = naming(&personal);
    personal.write(".board/board.json", Item::File(r#"{"x": 1}"#));
    let (refused, _) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
    assert_eq!(refused.code, 1);
    assert_eq!(code(&refused), "board::config::invalid");
    personal.write(".board/board.json", Item::File(CONFIG));
    let (admitted, prefix) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/");
}

#[test]
fn a_relative_personal_path_is_read_against_the_known_boards_files_directory() {
    let (personal, other) = (board(), board());
    let name = personal.workdir().file_name().unwrap();
    let relative = Path::new("../..").join(name);
    let xdg = config(Some(&json!({"personal": relative}).to_string()));
    assert_eq!(xdg.workdir().parent(), personal.workdir().parent());
    let (admitted, prefix) = known(other.workdir(), &xdg, None, &["--personal", "check"]);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    assert_eq!(prefix, "/");
}

#[test]
fn a_day_start_that_is_not_hh_mm_is_refused_and_one_that_is_or_is_null_is_admitted() {
    let library = library();
    let board = mounting(&library);
    for start in ["7am", "24:00", "07:60", "7:00"] {
        let file = json!({"boards": [board.path()], "day_start": start});
        let (refused, _) = known(
            library.workdir(),
            &config(Some(&file.to_string())),
            None,
            &["check"],
        );
        assert_eq!(code(&refused), "board::config::invalid", "{start}");
    }
    for start in [json!("07:00"), Value::Null] {
        let file = json!({"boards": [board.path()], "day_start": start});
        let (admitted, _) = known(
            library.workdir(),
            &config(Some(&file.to_string())),
            None,
            &["check"],
        );
        assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    }
}

#[test]
fn a_plugins_decide_is_refused_when_the_known_boards_file_cannot_be_read_and_the_owners_is_admitted()
 {
    let board = uncommitted();
    let broken = config(Some("{"));
    let decide = |plugin: Option<&str>| {
        let (output, _) = inside(
            plugin.map(OsString::from),
            board.workdir(),
            &broken,
            None,
            &["decide", "--op", "review.comment", "--path", "/notes/n"],
            b"",
        );
        output
    };
    assert_eq!(code(&decide(Some("tasks"))), "board::config::invalid");
    let admitted = decide(None);
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn a_plugin_name_that_is_empty_not_utf8_or_not_a_name_is_refused_and_a_name_is_admitted() {
    use std::os::unix::ffi::OsStringExt;
    let board = uncommitted();
    let empty = config(None);
    let put = |plugin: OsString| {
        let record = br#"{"$type": "lr/project/v1"}"#;
        let arguments = ["put", "/notes/n"];
        inside(
            Some(plugin),
            board.workdir(),
            &empty,
            None,
            &arguments,
            record,
        )
        .0
    };
    for refused in [
        OsString::from_vec(b"tas\xffks".to_vec()),
        OsString::new(),
        OsString::from("Tasks"),
        OsString::from("../tasks"),
    ] {
        let output = put(refused.clone());
        assert_eq!(code(&output), "board::config::invalid", "{refused:?}");
        assert!(text(&output.stderr).contains("not a plugin's name"));
    }
    assert!(!board.workdir().join("notes/n.json").exists());
    assert_eq!(text(&put(OsString::from("tasks")).stderr), "");
}

#[test]
fn a_write_command_the_put_check_does_not_cover_is_refused_under_a_plugin_and_admitted_for_the_owner()
 {
    let board = uncommitted();
    let empty = config(None);
    let schema = br#"{"type": "object"}"#;
    let run = |plugin: Option<&str>, arguments: &[&str], stdin: &[u8]| {
        inside(
            plugin.map(OsString::from),
            board.workdir(),
            &empty,
            None,
            arguments,
            stdin,
        )
        .0
    };
    let add = ["type", "add", "lr/kind/v1"];
    let refused = run(Some("tasks"), &add, schema);
    assert_eq!(code(&refused), "board::settings::from_plugin");
    assert!(text(&refused.stderr).contains("run board type add"));
    assert!(!board.workdir().join(".board/types/lr/kind").exists());
    assert_eq!(text(&run(None, &add, schema).stderr), "");
    assert!(
        board
            .workdir()
            .join(".board/types/lr/kind/v1.schema.json")
            .exists()
    );
    // A plugin's sync runs as the owner's does, and this board follows no
    // remote: sync moves commits the put check already saw.
    let sync = ["sync", "--no-push"];
    assert_eq!(
        code(&run(Some("tasks"), &sync, b"")),
        "board::sync::no_remote"
    );
    assert_eq!(code(&run(None, &sync, b"")), "board::sync::no_remote");
}
