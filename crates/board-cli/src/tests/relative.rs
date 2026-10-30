//! A path argument that does not start with `/` is read from the run's
//! prefix: the directory the run starts in, as a logical path in the board
//! it opens (docs/architecture.md, Boards and entries). So is a `board call` flag whose property is
//! `"format": "board-path"`.

use super::plugins::{RESULT, installed};
use super::*;

/// A manifest whose capability takes a board-path flag and a plain string.
const PATHED: &str = r#"{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks.",
  "run": ["./run"],
  "capabilities": [{
    "name": "due",
    "description": "Open tasks due under a path.",
    "inputSchema": {"type": "object", "properties": {
      "path": {"type": "string", "format": "board-path"},
      "note": {"type": "string"}
    }},
    "outputSchema": {"type": "object"}
  }]
}"#;

/// A repository whose commit holds README.md and docs/guide.md, and whose
/// working tree has a `docs` directory to run from.
fn library() -> Scenario {
    let library = Scenario::new();
    library.write("docs/guide.md", Item::File("guide"));
    library
}

/// A board enabling tasks that mounts `library`'s commit at `/repos/lib`
/// and does not commit a put, which would need a git identity the machine
/// running the test may lack,
/// and a plugins directory whose tasks offers `due` with `PATHED`'s input
/// and saves its request beside its manifest.
fn mounting(library: &Scenario) -> (Scenario, Scenario) {
    let (_, data) = installed();
    let board = board();
    let commit = library.commit(&[("README.md", "read me"), ("docs/guide.md", "guide")]);
    board
        .write(
            "repos/lib.json",
            Item::File(&mount_of(&library.path(), commit)),
        )
        .write(
            ".board/board.json",
            Item::File(
                r#"{"$type": "board/config/v1", "plugins": ["tasks"], "commit_on_put": false}"#,
            ),
        );
    let saved = data.workdir().join("plugins/tasks/request");
    let script = format!(
        "#!/bin/sh\ncat > '{}'\nprintf '%s' '{RESULT}'\n",
        saved.display()
    );
    data.write("plugins/tasks/plugin.json", Item::File(PATHED))
        .write("plugins/tasks/run", Item::Executable(&script));
    (board, data)
}

/// Runs `board` in `directory`, with the known boards file listing `board`,
/// tasks installed from `data`, `cache` and `stdin`.
fn relative(
    directory: &Path,
    fixture: &(Scenario, Scenario),
    cache: Option<&Path>,
    arguments: &[&str],
    stdin: &str,
) -> Output {
    relative_into(
        directory,
        fixture,
        cache,
        arguments,
        stdin,
        &mut Ran::default(),
    )
}

/// `relative`, recording the run in `ran`.
fn relative_into(
    directory: &Path,
    (board, data): &(Scenario, Scenario),
    cache: Option<&Path>,
    arguments: &[&str],
    stdin: &str,
    ran: &mut Ran,
) -> Output {
    let xdg = Scenario::new();
    let listed = json!({"boards": [board.path()]}).to_string();
    xdg.write("board/config.json", Item::File(&listed));
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: cache.map(Path::to_path_buf),
        plugins: Some(data.workdir().join("plugins")),
        boards: Some(xdg.workdir().join("board/config.json")),
        signer: None,
        allowed_signers: None,
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_ops::BuiltIn::default(),
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut stdin.as_bytes(), ran)
}

/// Runs `arguments` as `relative` does, without stdin or a cache, and
/// returns what the run recorded for the timing log.
fn recorded(directory: &Path, fixture: &(Scenario, Scenario), arguments: &[&str]) -> (Output, Ran) {
    let mut ran = Ran::default();
    let output = relative_into(directory, fixture, None, arguments, "", &mut ran);
    (output, ran)
}

/// The input tasks was sent.
fn sent(data: &Scenario) -> Value {
    let request = std::fs::read(data.workdir().join("plugins/tasks/request")).unwrap();
    serde_json::from_slice::<Value>(&request).unwrap()["input"].clone()
}

fn code(output: &Output) -> String {
    let stderr = text(&output.stderr);
    let code = stderr
        .lines()
        .find_map(|line| line.strip_prefix("  code:  "));
    code.unwrap_or_default().to_owned()
}

/// Asserts that `given` and `absolute`, run in `directory`, both succeed and
/// print the same.
fn same(directory: &Path, fixture: &(Scenario, Scenario), given: &[&str], absolute: &[&str]) {
    let read = relative(directory, fixture, None, given, "");
    assert_eq!(read.code, 0, "{given:?}: {}", text(&read.stderr));
    let expected = relative(directory, fixture, None, absolute, "");
    assert_eq!(expected.code, 0, "{absolute:?}: {}", text(&expected.stderr));
    assert_eq!(text(&read.stdout), text(&expected.stdout), "{given:?}");
}

#[test]
fn ls_and_get_read_a_relative_path_from_the_mounted_repository_and_its_subdirectory() {
    let library = library();
    let fixture = mounting(&library);
    let root = library.workdir();
    same(root, &fixture, &["ls", "."], &["ls", "/repos/lib"]);
    let get = relative(root, &fixture, None, &["get", "./README.md"], "");
    assert_eq!(text(&get.stdout), "read me");
    let docs = root.join("docs");
    same(&docs, &fixture, &["ls", "."], &["ls", "/repos/lib/docs"]);
    for given in ["./guide.md", "guide.md"] {
        let get = relative(&docs, &fixture, None, &["get", given], "");
        assert_eq!(text(&get.stdout), "guide", "{given}");
    }
    let listed = relative(&docs, &fixture, None, &["ls", "."], "");
    assert!(
        text(&listed.stdout).contains("guide.md"),
        "{}",
        text(&listed.stdout)
    );
}

#[test]
fn inside_a_board_a_subdirectory_is_the_prefix_and_the_root_reads_from_slash() {
    let fixture = mounting(&library());
    let root = fixture.0.workdir();
    same(root, &fixture, &["ls", "."], &["ls", "/"]);
    same(
        root,
        &fixture,
        &["get", "./projects/p"],
        &["get", "/projects/p"],
    );
    same(
        root,
        &fixture,
        &["check", "projects"],
        &["check", "/projects"],
    );
    same(
        root,
        &fixture,
        &["refs", "projects/p"],
        &["refs", "/projects/p"],
    );
    let task = r#"{"$type": "lr/task/v1", "title": "t"}"#;
    let put = relative(root, &fixture, None, &["put", "tasks/new"], task);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    let got = relative(root, &fixture, None, &["get", "/tasks/new"], "");
    assert_eq!(got.code, 0, "{}", text(&got.stderr));
    let projects = root.join("projects");
    same(&projects, &fixture, &["ls", "."], &["ls", "/projects"]);
    same(&projects, &fixture, &["get", "p"], &["get", "/projects/p"]);
    same(
        &projects,
        &fixture,
        &["check", "."],
        &["check", "/projects"],
    );
    let (_, ran) = recorded(&projects, &fixture, &["ls", "."]);
    assert_eq!(ran.prefix.as_deref(), Some("/projects"));
    let (_, ran) = recorded(root, &fixture, &["ls", "."]);
    assert_eq!(ran.prefix.as_deref(), Some("/"));
}

#[test]
fn from_a_board_directory_whose_name_is_no_logical_path_a_relative_path_is_refused() {
    let fixture = mounting(&library());
    let board = &fixture.0;
    board.write("a\tb/x.md", Item::File("x"));
    board.write("ab/x.md", Item::File("x"));
    let task = r#"{"$type": "lr/task/v1", "title": "t"}"#;
    let refused = board.workdir().join("a\tb");
    for (arguments, stdin) in [(&["ls", "."][..], ""), (&["put", "new"][..], task)] {
        let output = relative(&refused, &fixture, None, arguments, stdin);
        assert_eq!(code(&output), "board::path::malformed", "{arguments:?}");
    }
    same(&refused, &fixture, &["ls", "/ab"], &["ls", "/ab"]);
    let (_, ran) = recorded(&refused, &fixture, &["ls", "/"]);
    assert_eq!(ran.prefix.as_deref(), Some("/a\tb"));
    let admitted = board.workdir().join("ab");
    same(&admitted, &fixture, &["ls", "."], &["ls", "/ab"]);
    let put = relative(&admitted, &fixture, None, &["put", "new"], task);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
}

#[test]
fn with_c_the_prefix_is_the_directory_c_enters_as_git_reads_it() {
    let fixture = mounting(&library());
    let outside = Scenario::new();
    let elsewhere = outside.workdir();
    let entered = fixture.0.workdir().join("projects");
    let entered = entered.to_str().unwrap();
    same(
        elsewhere,
        &fixture,
        &["-C", entered, "ls", "."],
        &["-C", entered, "ls", "/projects"],
    );
    let root = fixture.0.workdir();
    let root = root.to_str().unwrap();
    same(
        elsewhere,
        &fixture,
        &["-C", root, "ls", "."],
        &["-C", root, "ls", "/"],
    );
}

#[test]
fn a_relative_path_that_climbs_above_the_root_is_refused_and_one_that_stays_is_admitted() {
    let library = library();
    let fixture = mounting(&library);
    let root = fixture.0.workdir();
    let refused = relative(root, &fixture, None, &["ls", ".."], "");
    assert_eq!(refused.code, 2);
    assert_eq!(
        text(&refused.stderr),
        "error: path \"..\", read from /, is refused: it has a . or .. segment, \
         which Board does not support\n  help:  run `board ls -h`\n  code:  board::cli::usage\n"
    );
    assert_eq!(relative(root, &fixture, None, &["ls", "."], "").code, 0);
}

#[test]
fn a_dot_dot_below_the_root_suggests_the_absolute_path_it_meant_and_that_path_runs() {
    let library = library();
    let fixture = mounting(&library);
    let docs = library.workdir().join("docs");
    let refused = relative(&docs, &fixture, None, &["get", "../README.md"], "");
    assert_eq!(refused.code, 2);
    assert_eq!(code(&refused), "board::cli::usage");
    assert_eq!(
        text(&refused.stderr),
        "error: path \"../README.md\", read from /repos/lib/docs, is refused: it has a . or .. \
         segment, which Board does not support\n  help:  use /repos/lib/README.md for ../README.md\n  \
         code:  board::cli::usage\n"
    );
    let admitted = relative(&docs, &fixture, None, &["get", "/repos/lib/README.md"], "");
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
    let admitted = relative(&docs, &fixture, None, &["get", "./guide.md"], "");
    assert_eq!(admitted.code, 0, "{}", text(&admitted.stderr));
}

#[test]
fn one_trailing_slash_on_a_relative_path_is_dropped_and_two_are_refused() {
    let library = library();
    let fixture = mounting(&library);
    let root = library.workdir();
    same(root, &fixture, &["ls", "docs/"], &["ls", "/repos/lib/docs"]);
    let refused = relative(root, &fixture, None, &["ls", "docs//"], "");
    assert_eq!(refused.code, 2);
    assert_eq!(code(&refused), "board::cli::usage");
    let said = text(&refused.stderr);
    assert!(
        said.contains("path \"docs//\", read from /repos/lib, is refused: it has an empty segment"),
        "{said}"
    );
    let flags = ["call", "tasks.due", "--path", "docs/"];
    let called = relative(root, &fixture, None, &flags, "");
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    assert_eq!(sent(&fixture.1), json!({"path": "/repos/lib/docs"}));
}

#[test]
fn a_board_path_flags_help_says_a_relative_path_is_read_and_a_plain_flags_does_not() {
    let fixture = mounting(&library());
    let help = relative(
        fixture.0.workdir(),
        &fixture,
        None,
        &["call", "tasks.due", "-h"],
        "",
    );
    assert_eq!(help.code, 0, "{}", text(&help.stderr));
    let said = text(&help.stdout);
    let line = |flag: &str| {
        said.lines()
            .find(|line| line.contains(flag))
            .unwrap_or_default()
            .to_owned()
    };
    let relative = "string; a path without a leading / is read from the current directory.";
    assert!(line("--path").contains(relative), "{said}");
    assert!(!line("--note").contains(relative), "{said}");
}

#[test]
fn a_board_path_flag_is_read_from_the_prefix_and_a_plain_string_flag_stays_literal() {
    let library = library();
    let fixture = mounting(&library);
    let docs = library.workdir().join("docs");
    let flags = [
        "call",
        "tasks.due",
        "--path",
        "guide.md",
        "--note",
        "guide.md",
    ];
    let called = relative(&docs, &fixture, None, &flags, "");
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    let expected = json!({"path": "/repos/lib/docs/guide.md", "note": "guide.md"});
    assert_eq!(sent(&fixture.1), expected);
    let called = relative(
        &docs,
        &fixture,
        None,
        &["call", "tasks.due", "--path", "."],
        "",
    );
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    assert_eq!(sent(&fixture.1), json!({"path": "/repos/lib/docs"}));
    let root = fixture.0.workdir();
    let called = relative(
        root,
        &fixture,
        None,
        &["call", "tasks.due", "--path", "docs"],
        "",
    );
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    assert_eq!(sent(&fixture.1), json!({"path": "/docs"}));
}

#[test]
fn a_board_path_flag_that_climbs_above_the_root_stays_literal_for_the_schema_to_judge() {
    let fixture = mounting(&library());
    let root = fixture.0.workdir();
    let called = relative(
        root,
        &fixture,
        None,
        &["call", "tasks.due", "--path", ".."],
        "",
    );
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    assert_eq!(sent(&fixture.1), json!({"path": ".."}));
}

#[test]
fn json_on_stdin_is_sent_as_given_and_the_same_path_as_a_flag_is_resolved() {
    let library = library();
    let fixture = mounting(&library);
    let docs = library.workdir().join("docs");
    let stdin = r#"{"path": "guide.md"}"#;
    let called = relative(&docs, &fixture, None, &["call", "tasks.due"], stdin);
    assert_eq!(called.code, 0, "{}", text(&called.stderr));
    assert_eq!(sent(&fixture.1), json!({"path": "guide.md"}));
    let flags = ["call", "tasks.due", "--path", "guide.md"];
    relative(&docs, &fixture, None, &flags, "");
    assert_eq!(
        sent(&fixture.1),
        json!({"path": "/repos/lib/docs/guide.md"})
    );
}

#[test]
fn a_relative_path_answers_from_a_warm_index_as_from_a_cold_one() {
    let library = library();
    let fixture = mounting(&library);
    let docs = library.workdir().join("docs");
    let cache = tempfile::tempdir().unwrap();
    for arguments in [["ls", "."], ["check", "."], ["refs", "guide.md"]] {
        let cold = relative(&docs, &fixture, None, &arguments, "");
        relative(&docs, &fixture, Some(cache.path()), &arguments, "");
        let warm = relative(&docs, &fixture, Some(cache.path()), &arguments, "");
        assert_eq!(warm.code, cold.code, "{arguments:?}");
        assert_eq!(text(&warm.stdout), text(&cold.stdout), "{arguments:?}");
    }
}
