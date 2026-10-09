//! Shell completion: `BOARD_COMPLETE=<shell> board` and the calls its script
//! makes.

use super::*;

/// Runs `board` from `directory` with `BOARD_COMPLETE` set to `shell`, with
/// `home` as the home directory and `stdin` as standard input.
fn with_shell(
    directory: &Path,
    home: Option<&Path>,
    shell: &str,
    arguments: &[&str],
    stdin: &[u8],
) -> Output {
    let context = Context {
        start: directory.to_path_buf(),
        home: home.map(Path::to_path_buf),
        cache: None,
        plugins: None,
        environment: board_plugin::Environment::default(),
        complete: Some(shell.into()),
        cursor: None,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut &stdin[..])
}

fn completing(directory: &Path, shell: &str, arguments: &[&str]) -> Output {
    with_shell(directory, None, shell, arguments, b"")
}

/// The values fish is offered for the command line `words`, the last being
/// the word under the cursor, with `home` as the home directory.
fn offered_at(directory: &Path, home: Option<&Path>, words: &[&str]) -> Vec<String> {
    let mut arguments = vec!["--", BINARY_NAME];
    arguments.extend_from_slice(words);
    let output = with_shell(directory, home, "fish", &arguments, b"");
    assert_eq!(output.code, 0);
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
    text(&output.stdout)
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default().to_owned())
        .collect()
}

fn offered(directory: &Path, words: &[&str]) -> Vec<String> {
    offered_at(directory, None, words)
}

fn tasks() -> Scenario {
    let scenario = board();
    scenario
        .write(
            "tasks/t1.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "a"}"#),
        )
        .write(
            "tasks/t2.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "b"}"#),
        );
    scenario
}

#[test]
fn each_supported_shell_gets_a_script_that_calls_board_back() {
    let scenario = board();
    for (shell, call) in [
        ("bash", r#"BOARD_COMPLETE="bash""#),
        ("zsh", r#"BOARD_COMPLETE="zsh""#),
        ("fish", "BOARD_COMPLETE=fish"),
    ] {
        let output = completing(scenario.workdir(), shell, &[]);
        let script = text(&output.stdout);
        assert_eq!(output.code, 0, "{shell}");
        assert!(output.stderr.is_empty(), "{shell}");
        assert!(script.contains(call), "{shell}: {script}");
        assert!(script.contains(BINARY_NAME), "{shell}: {script}");
    }
}

#[test]
fn an_unknown_shell_asking_for_a_script_is_refused_with_the_shells_it_could_be() {
    let scenario = board();
    let refused = completing(scenario.workdir(), "tcsh", &[]);
    assert_eq!(refused.code, 2);
    assert!(refused.stdout.is_empty());
    assert!(
        text(&refused.stderr).starts_with("error: board::usage::invalid: "),
        "{}",
        text(&refused.stderr)
    );
    assert!(hint_of(&refused).contains("BOARD_COMPLETE=fish board | source"));
    let admitted = completing(scenario.workdir(), "fish", &[]);
    assert_eq!(admitted.code, 0);
    assert!(!admitted.stdout.is_empty());
}

#[test]
fn an_unknown_shell_while_completing_prints_nothing() {
    let output = completing(board().workdir(), "tcsh", &["--", BINARY_NAME, "g"]);
    assert_eq!(output, Output::default());
}

#[test]
fn a_real_command_runs_whatever_board_complete_holds() {
    let record = br#"{"$type": "lr/task/v1", "title": "a"}"#;
    for shell in ["fish", "1", "true", "", "0"] {
        // Without a commit, the put needs no git identity, which the
        // machine running the test may lack.
        let scenario = board();
        scenario.write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        );
        let put = with_shell(
            scenario.workdir(),
            None,
            shell,
            &["put", "/tasks/new"],
            record,
        );
        assert_eq!(put.code, 0, "{shell:?}: {}", text(&put.stderr));
        assert!(
            scenario.workdir().join("tasks/new.json").exists(),
            "{shell:?}"
        );
        let listed = with_shell(scenario.workdir(), None, shell, &["ls", "--json"], b"");
        assert_eq!(listed.code, 0, "{shell:?}");
        assert_eq!(json(&listed)["$type"], "board/ls-report/v1", "{shell:?}");
    }
}

#[test]
fn board_complete_empty_or_zero_asks_for_no_script() {
    let scenario = board();
    for shell in ["", "0"] {
        let output = completing(scenario.workdir(), shell, &[]);
        assert_eq!(output.code, 2, "{shell:?}");
        assert!(
            text(&output.stderr).starts_with("error: board::usage::invalid: board needs a command"),
            "{shell:?}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn commands_subcommands_and_flags_complete() {
    let scenario = board();
    assert_eq!(offered(scenario.workdir(), &["g"]), ["get"]);
    assert_eq!(offered(scenario.workdir(), &["plugin", "en"]), ["enable"]);
    assert!(offered(scenario.workdir(), &["ls", "--"]).contains(&"--limit".to_owned()));
}

#[test]
fn only_the_words_after_a_leading_double_dash_are_completed() {
    let scenario = board();
    let output = completing(scenario.workdir(), "fish", &["--", BINARY_NAME, "g"]);
    assert_eq!(text(&output.stdout).split('\t').next(), Some("get"));
}

#[test]
fn every_path_argument_completes_from_the_board() {
    let scenario = tasks();
    for words in [
        &["get"][..],
        &["ls"],
        &["check"],
        &["refs", "--missing"],
        &["put", "--expect", "none"],
        &["mount", "update"],
    ] {
        let mut line = words.to_vec();
        line.push("/tasks/t");
        assert_eq!(
            offered(scenario.workdir(), &line),
            ["/tasks/t1", "/tasks/t2"],
            "{words:?}"
        );
    }
    assert_eq!(offered(scenario.workdir(), &["get", "/ta"]), ["/tasks/"]);
    assert!(offered(scenario.workdir(), &["get", "/zz"]).is_empty());
}

#[test]
fn a_path_typed_with_shell_escapes_completes_as_the_name_it_spells() {
    let scenario = board();
    scenario.write(
        "with space/a.json",
        Item::File(r#"{"$type": "lr/project/v1"}"#),
    );
    for typed in [
        r"/with\ space/",
        "'/with space/'",
        r#""/with space/""#,
        "/with' 'space/",
    ] {
        assert_eq!(
            offered(scenario.workdir(), &["get", typed]),
            ["/with space/a"],
            "{typed}"
        );
    }
    assert!(offered(scenario.workdir(), &["get", r"/with\ spade/"]).is_empty());
}

#[test]
fn bash_is_offered_a_name_escaped_since_it_inserts_it_as_it_is() {
    use crate::complete::{escaped, unescape};
    assert_eq!(escaped("/with space/", "bash"), r"/with\ space/");
    assert_eq!(escaped("/it's$/a", "bash"), r"/it\'s\$/a");
    assert_eq!(escaped("/tasks/t-1_2.md", "bash"), "/tasks/t-1_2.md");
    // fish and zsh quote what they insert themselves.
    assert_eq!(escaped("/with space/", "fish"), "/with space/");
    assert_eq!(escaped("/with space/", "zsh"), "/with space/");
    for name in ["/with space/", "/it's$/a", "/a\"b/"] {
        assert_eq!(unescape(&escaped(name, "bash")), name);
    }
}

#[test]
fn outside_a_board_a_path_offers_nothing_and_says_nothing() {
    let outside = tempfile::tempdir().unwrap();
    let output = completing(outside.path(), "fish", &["--", BINARY_NAME, "get", "/"]);
    assert_eq!(output, Output::default());
    assert_eq!(
        offered(tasks().workdir(), &["get", "/"]),
        ["/projects/", "/tasks/"]
    );
}

#[test]
fn a_path_completes_in_the_board_that_dash_c_names() {
    let scenario = tasks();
    let outside = tempfile::tempdir().unwrap();
    let board = scenario.workdir().to_str().unwrap();
    assert_eq!(
        offered(outside.path(), &["-C", board, "get", "/ta"]),
        ["/tasks/"]
    );
    assert!(offered(outside.path(), &["-C", "/does/not/exist", "get", "/ta"]).is_empty());
}

#[test]
fn a_dash_c_joined_by_an_equals_sign_names_the_board() {
    let scenario = tasks();
    let outside = tempfile::tempdir().unwrap();
    let joined = format!("-C={}", scenario.workdir().to_str().unwrap());
    assert_eq!(
        offered(outside.path(), &[&joined, "get", "/ta"]),
        ["/tasks/"]
    );
    assert!(offered(outside.path(), &["-C=/does/not/exist", "get", "/ta"]).is_empty());
}

#[test]
fn a_dash_c_fish_has_already_unquoted_is_taken_as_it_is() {
    let scenario = tasks();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(scenario.workdir(), outside.path().join("it's")).unwrap();
    assert_eq!(
        offered(outside.path(), &["-C", "it's", "get", "/ta"]),
        ["/tasks/"]
    );
    assert!(offered(outside.path(), &["-C", "its", "get", "/ta"]).is_empty());
}

#[test]
fn a_dash_c_under_the_home_directory_completes_through_a_tilde() {
    let scenario = tasks();
    let outside = tempfile::tempdir().unwrap();
    let home = scenario.workdir().parent().unwrap();
    let name = scenario.workdir().file_name().unwrap().to_str().unwrap();
    let tilde = format!("~/{name}");
    assert_eq!(
        offered_at(outside.path(), Some(home), &["-C", &tilde, "get", "/ta"]),
        ["/tasks/"]
    );
    assert!(offered_at(outside.path(), None, &["-C", &tilde, "get", "/ta"]).is_empty());
    // `~name` is another user's home, which completion does not look up.
    let user = format!("~{name}");
    assert!(offered_at(outside.path(), Some(home), &["-C", &user, "get", "/ta"]).is_empty());
}

#[test]
fn the_root_help_says_how_to_install_completion() {
    let help = text(&board_in(board().workdir(), &["--help"]).stdout);
    assert!(
        help.contains("BOARD_COMPLETE=fish board | source"),
        "{help}"
    );
    assert!(
        help.contains(r#"eval "$(BOARD_COMPLETE=bash board)""#),
        "{help}"
    );
}
