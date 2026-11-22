//! `board init --personal`: it makes a git repository a board, enables the
//! plugins a colleague needs and names the board as the personal one in the
//! known boards file (docs/architecture/paths/personal.md).

use super::*;

/// The plugins `init --personal` enables, in order.
const SIX: [&str; 6] = ["tasks", "runs", "evals", "rules", "plans", "share"];

/// Runs `board` in `directory` with the built-in plugins and the known
/// boards file in `xdg`.
fn personal(directory: &Path, xdg: &Path, arguments: &[&str]) -> Output {
    let context = Context {
        start: directory.to_path_buf(),
        home: None,
        cache: None,
        plugins: None,
        boards: Some(file(xdg)),
        signer: None,
        allowed_signers: None,
        attention_allowed_signers: None,
        environment: board_plugin::Environment {
            path: Some(PATH.into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_plugins::BUILT_IN,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut &b""[..], &mut Ran::default())
}

/// The known boards file in `xdg`.
fn file(xdg: &Path) -> std::path::PathBuf {
    xdg.join("board/config.json")
}

/// The subjects of the commits that changed `.board/board.json`, oldest first.
fn enables(board: &Scenario, xdg: &Path) -> Vec<String> {
    let log = personal(board.workdir(), xdg, &["log", "/.board/board", "--json"]);
    let rows = json(&log)["data"].as_array().unwrap().clone();
    let mut subjects: Vec<String> = (rows.iter())
        .map(|row| row["subject"].as_str().unwrap().to_owned())
        .collect();
    subjects.reverse();
    subjects
}

/// The plugins `plugin list` shows enabled.
fn enabled(board: &Scenario, xdg: &Path) -> Vec<String> {
    let list = personal(board.workdir(), xdg, &["plugin", "list", "--json"]);
    let rows = json(&list)["data"].as_array().unwrap().clone();
    (rows.iter())
        .filter(|row| row["enabled"] == true)
        .map(|row| row["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn init_personal_enables_six_plugins_names_the_board_and_a_second_run_changes_nothing() {
    let board = Scenario::new();
    board.identified();
    let xdg = tempfile::tempdir().unwrap();
    let report = personal(
        board.workdir(),
        xdg.path(),
        &["init", "--personal", "--json"],
    );
    assert_eq!(report.code, 0, "{}", text(&report.stdout));
    let data = &json(&report)["data"];
    let root = board.workdir().canonicalize().unwrap();
    let shown = root.display().to_string();
    assert_eq!(data["root"], json!(shown));
    assert_eq!(data["created"], true);
    assert_eq!(data["enabled"], json!(SIX));
    let config = file(xdg.path());
    assert_eq!(
        data["personal"],
        json!({"file": config.display().to_string(), "named": true})
    );
    let mut expected: Vec<String> = SIX.map(str::to_owned).to_vec();
    expected.sort();
    let mut listed = enabled(&board, xdg.path());
    listed.sort();
    assert_eq!(listed, expected);
    let subjects = SIX.map(|name| format!("Enable the {name} plugin"));
    assert_eq!(enables(&board, xdg.path()), subjects);
    let written = std::fs::read(&config).unwrap();
    let known: Value = serde_json::from_slice(&written).unwrap();
    assert_eq!(known, json!({"boards": [shown], "personal": shown}));
    let elsewhere = tempfile::tempdir().unwrap();
    let check = personal(elsewhere.path(), xdg.path(), &["--personal", "check"]);
    assert_eq!(check.code, 0, "{}", text(&check.stderr));

    let head = board.head().unwrap().id;
    let again = personal(
        board.workdir(),
        xdg.path(),
        &["init", "--personal", "--json"],
    );
    assert_eq!(again.code, 0, "{}", text(&again.stdout));
    let data = &json(&again)["data"];
    assert_eq!(data["created"], false);
    assert_eq!(data["enabled"], json!([]));
    assert_eq!(data["personal"]["named"], false);
    assert_eq!(board.head().unwrap().id, head, "no new commit");
    assert_eq!(std::fs::read(&config).unwrap(), written);
}

#[test]
fn init_personal_says_what_it_enabled_and_named_and_what_to_run_next() {
    let board = Scenario::new();
    board.identified();
    let xdg = tempfile::tempdir().unwrap();
    let output = personal(board.workdir(), xdg.path(), &["init", "--personal"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let root = board.workdir().canonicalize().unwrap();
    let config = file(xdg.path());
    let expected = format!(
        "created .board/board.json in {root}\nenabled tasks, runs, evals, rules, plans and share\nnamed {root} as the personal board in {config}\n",
        root = root.display(),
        config = config.display()
    );
    assert_eq!(text(&output.stdout), expected);
    assert_eq!(
        text(&output.stderr),
        "next: run `board --personal tasks add --kind project --title '<title>'`, or ask Claude Code to add a project with the tasks skill\n"
    );
    let again = personal(board.workdir(), xdg.path(), &["init", "--personal"]);
    let expected = format!(
        "{root} is already a board\nthe six plugins were already enabled\n{config} already names {root}\n",
        root = root.display(),
        config = config.display()
    );
    assert_eq!(text(&again.stdout), expected);
    assert_eq!(text(&again.stderr), "");
}

#[test]
fn init_personal_on_a_board_with_two_of_the_six_enabled_enables_only_the_other_four() {
    let board = Scenario::new();
    board.identified();
    let elsewhere = "/elsewhere/board";
    let known = json!({"boards": [elsewhere], "day_start": "07:00"});
    let before = Scenario::new();
    before.write("board/config.json", Item::File(&known.to_string()));
    let xdg = before.workdir();
    assert_eq!(personal(board.workdir(), xdg, &["init"]).code, 0);
    for name in ["runs", "tasks"] {
        let enabled = personal(board.workdir(), xdg, &["plugin", "enable", name]);
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    }
    let output = personal(board.workdir(), xdg, &["init", "--personal"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    let root = board.workdir().canonicalize().unwrap();
    let file = file(xdg);
    let expected = format!(
        "{root} is already a board\nenabled evals, rules, plans and share\nnamed {root} as the personal board in {file}\n",
        root = root.display(),
        file = file.display()
    );
    assert_eq!(text(&output.stdout), expected);
    assert!(text(&output.stderr).starts_with("next: run `board --personal tasks add"));
    let mut listed = enabled(&board, xdg);
    listed.sort();
    assert_eq!(
        listed,
        ["evals", "plans", "rules", "runs", "share", "tasks"]
    );
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let shown = root.display().to_string();
    let expected = json!({"boards": [elsewhere, shown], "personal": shown, "day_start": "07:00"});
    assert_eq!(written, expected);
}

#[test]
fn init_personal_where_the_file_names_another_board_writes_nothing_and_its_twin_naming_this_one_succeeds()
 {
    let other = Scenario::board();
    for admitted in [false, true] {
        let board = Scenario::new();
        board.identified();
        let root = if admitted { board.path() } else { other.path() };
        let known = json!({"personal": root}).to_string();
        // The known boards file sits in a directory of its own, as XDG's does.
        let config = Scenario::new();
        config.write("board/config.json", Item::File(&known));
        let xdg = config.workdir();
        let output = personal(board.workdir(), xdg, &["init", "--personal", "--json"]);
        let has_board = board.workdir().join(".board/board.json").exists();
        if admitted {
            assert_eq!(output.code, 0, "{}", text(&output.stdout));
            assert!(has_board);
            continue;
        }
        assert_eq!(output.code, 1);
        assert_eq!(json(&output)["code"], "board::config::personal_elsewhere");
        assert!(!has_board, "no .board/board.json is written");
        assert!(board.head().is_none(), "nothing is committed");
        assert_eq!(std::fs::read_to_string(file(xdg)).unwrap(), known);
    }
}

#[test]
fn init_without_personal_writes_no_known_boards_file() {
    let board = Scenario::new();
    let xdg = tempfile::tempdir().unwrap();
    let output = personal(board.workdir(), xdg.path(), &["init", "--json"]);
    assert_eq!(output.code, 0);
    let data = &json(&output)["data"];
    assert_eq!((data.get("enabled"), data.get("personal")), (None, None));
    assert!(!file(xdg.path()).exists());
}

#[expect(
    clippy::disallowed_methods,
    reason = "only a child process can be given an environment without a git identity"
)]
fn without_identity(board: &Scenario, home: &Path) -> std::process::Output {
    std::process::Command::new(super::super::help::binary())
        .args(["init", "--personal", "--json"])
        .current_dir(board.workdir())
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_CACHE_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

#[test]
fn init_personal_without_a_git_identity_leaves_the_file_unchanged_and_with_one_finishes() {
    let board = Scenario::new();
    let home = tempfile::tempdir().unwrap();
    let refused = without_identity(&board, home.path());
    assert_eq!(refused.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(error["code"], "board::put::no_identity");
    assert!(
        !file(home.path()).exists(),
        "the known boards file is not written"
    );
    board.identified();
    let admitted = without_identity(&board, home.path());
    assert_eq!(
        admitted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&admitted.stdout)
    );
    let report: Value = serde_json::from_slice(&admitted.stdout).unwrap();
    assert_eq!(report["data"]["created"], false);
    assert_eq!(report["data"]["enabled"], json!(SIX));
    assert_eq!(report["data"]["personal"]["named"], true);
}
