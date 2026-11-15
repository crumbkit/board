//! `board get --at` and `board log`: a board read at one commit, and the
//! commits that changed a path.

use std::ffi::OsString;

use board_model::{BINARY_NAME, ObjectId};
use board_scenario::seed::mount_of;
use board_scenario::{Item, Scenario};
use serde_json::Value;

use super::{answers, board_cached, board_with, json, text};
use crate::config::Context;
use crate::report::Output;
use crate::run::run;
use crate::timing::Ran;

/// The first commit's time; each later commit is a minute after the one
/// before, so a walk's order is fixed.
const T: i64 = 1_700_000_000;

const CONFIG: (&str, &str) = (".board/board.json", r#"{"$type": "board/config/v1"}"#);

/// A board whose history puts `/tasks/a`, puts `/tasks/b`, edits `/tasks/a`,
/// removes it and puts it again, with each commit's id in order.
fn history() -> (Scenario, Vec<ObjectId>) {
    let scenario = Scenario::board();
    let b = ("tasks/b.json", "{}");
    let trees: [(&[(&str, &str)], &str); 5] = [
        (
            &[CONFIG, ("tasks/a.json", r#"{"n": 1}"#)],
            "board put /tasks/a",
        ),
        (
            &[CONFIG, ("tasks/a.json", r#"{"n": 1}"#), b],
            "board put /tasks/b",
        ),
        (
            &[CONFIG, ("tasks/a.json", r#"{"n": 2}"#), b],
            "board put /tasks/a",
        ),
        (&[CONFIG, b], "board rm /tasks/a"),
        (
            &[CONFIG, ("tasks/a.json", r#"{"n": 3}"#), b],
            "board put /tasks/a",
        ),
    ];
    let mut commits: Vec<ObjectId> = Vec::new();
    for (at, (files, message)) in trees.into_iter().enumerate() {
        let parents: Vec<ObjectId> = commits.last().copied().into_iter().collect();
        let time = T + 60 * i64::try_from(at).unwrap();
        commits.push(scenario.commit_at(files, &parents, time, message));
    }
    (scenario, commits)
}

/// Runs `board` in `scenario` with a cache of its own.
fn run_in(scenario: &Scenario, arguments: &[&str]) -> Output {
    let cache = tempfile::tempdir().unwrap();
    board_cached(scenario.workdir(), Some(cache.path()), arguments)
}

/// The code and error of a run.
fn ended(output: &Output) -> (u8, Option<&str>) {
    (output.code, output.error.as_deref())
}

#[test]
fn get_at_the_first_commit_prints_the_bytes_the_record_had_then_and_plain_get_the_newer_bytes() {
    let (scenario, commits) = history();
    scenario.write("tasks/a.json", Item::File(r#"{"n": 4}"#));
    let first = commits[0].to_string();
    let old = run_in(&scenario, &["get", "--at", &first, "/tasks/a"]);
    assert_eq!(
        (ended(&old), text(&old.stdout)),
        ((0, None), r#"{"n": 1}"#.to_owned())
    );
    let now = run_in(&scenario, &["get", "/tasks/a"]);
    assert_eq!(text(&now.stdout), r#"{"n": 4}"#);
    let report = run_in(&scenario, &["get", "--at", &first, "/tasks/a", "--json"]);
    let answer = &answers(&report)[0];
    assert_eq!(answer["location"]["source"], json!(first));
    assert_eq!(answer["text"], json!(r#"{"n": 1}"#));
}

#[test]
fn get_at_one_commit_answers_each_path_and_refuses_only_the_one_absent_there() {
    let (scenario, commits) = history();
    let at = commits[2].to_string();
    let refused = run_in(
        &scenario,
        &[
            "get",
            "--at",
            &at,
            "/tasks/a",
            "/tasks/zz",
            "/tasks/b",
            "--json",
        ],
    );
    assert_eq!(ended(&refused), (1, Some("board::path::not_found")));
    let answered = answers(&refused);
    assert_eq!(answered[0]["text"], json!(r#"{"n": 2}"#));
    assert_eq!(
        (answered[1]["index"].clone(), answered[1]["code"].clone()),
        (json!(1), json!("board::path::not_found"))
    );
    assert_eq!(answered[2]["text"], json!("{}"));
    let admitted = run_in(
        &scenario,
        &["get", "--at", &at, "/tasks/a", "/tasks/b", "--json"],
    );
    assert_eq!(ended(&admitted), (0, None));
    assert_eq!(answers(&admitted).len(), 2);
}

#[test]
fn get_at_an_id_that_names_no_commit_refuses_every_path_and_a_commits_id_reads() {
    let (scenario, commits) = history();
    let blob = scenario.blob(r#"{"n": 1}"#).to_string();
    let absent = "1".repeat(40);
    let long = "2".repeat(64);
    for refused in [absent.as_str(), blob.as_str(), long.as_str()] {
        let output = run_in(
            &scenario,
            &["get", "--at", refused, "/tasks/a", "/tasks/b", "--json"],
        );
        assert_eq!(
            ended(&output),
            (1, Some("board::commit::not_found")),
            "{refused}"
        );
        let codes: Vec<Value> = answers(&output)
            .iter()
            .map(|answer| answer["code"].clone())
            .collect();
        assert_eq!(
            codes,
            vec![json!("board::commit::not_found"); 2],
            "{refused}"
        );
    }
    let admitted = run_in(
        &scenario,
        &["get", "--at", &commits[1].to_string(), "/tasks/a"],
    );
    assert_eq!(ended(&admitted), (0, None));
}

/// A board that mounts `library` at `/repos/lib`, named in a known boards
/// file, so a run from the library's own checkout may take `--checkout`.
struct Mounted {
    /// The board, kept for its directory's lifetime.
    _board: Scenario,
    library: Scenario,
    xdg: Scenario,
}

impl Mounted {
    fn new() -> Self {
        let library = Scenario::new();
        let pinned = library.commit(&[("docs/usage.md", "one")]);
        let board = Scenario::board();
        board.write(
            "repos/lib.json",
            Item::File(&mount_of(&library.path(), pinned)),
        );
        board.commit_at(
            &[
                CONFIG,
                ("repos/lib.json", &mount_of(&library.path(), pinned)),
            ],
            &[],
            T,
            "mount",
        );
        let xdg = Scenario::new();
        let known = json!({"boards": [board.path()]}).to_string();
        xdg.write("board/config.json", Item::File(&known));
        Self {
            _board: board,
            library,
            xdg,
        }
    }

    /// Runs `board` from the library's checkout with the known boards file.
    fn run(&self, arguments: &[&str]) -> Output {
        let context = Context {
            start: self.library.workdir().to_path_buf(),
            home: None,
            cache: None,
            plugins: None,
            boards: Some(self.xdg.workdir().join("board/config.json")),
            signer: None,
            allowed_signers: None,
            attention_allowed_signers: None,
            environment: board_plugin::Environment::default(),
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
        run(arguments, &context, &mut &b""[..], &mut Ran::default())
    }
}

#[test]
fn an_at_that_is_not_head_or_a_full_id_and_checkout_with_a_read_at_a_commit_are_usage() {
    let (scenario, _) = history();
    for at in ["abc123", "main"] {
        let refused = run_in(&scenario, &["get", "--at", at, "/tasks/a"]);
        assert_eq!(ended(&refused), (2, Some("board::cli::usage")), "{at}");
    }
    let admitted = run_in(&scenario, &["get", "--at", "HEAD", "/tasks/a"]);
    assert_eq!(ended(&admitted), (0, None));
    let mounted = Mounted::new();
    let usage = "/repos/lib/docs/usage.md";
    for refused in [
        mounted.run(&["--checkout", "get", "--at", "HEAD", usage]),
        mounted.run(&["--checkout", "log", usage]),
    ] {
        assert_eq!(
            ended(&refused),
            (2, Some("board::cli::usage")),
            "{}",
            text(&refused.stderr)
        );
        assert!(
            text(&refused.stderr).contains("pins in the commit read"),
            "{}",
            text(&refused.stderr)
        );
    }
    let checkout = mounted.run(&["--checkout", "get", usage]);
    assert_eq!(ended(&checkout), (0, None), "{}", text(&checkout.stderr));
    assert_eq!(text(&checkout.stdout), "one");
}

#[test]
fn get_at_head_ignores_an_uncommitted_edit_that_plain_get_shows() {
    let (scenario, _) = history();
    scenario.write("tasks/a.json", Item::File(r#"{"n": 9}"#));
    let committed = run_in(&scenario, &["get", "--at", "HEAD", "/tasks/a"]);
    assert_eq!(text(&committed.stdout), r#"{"n": 3}"#);
    let edited = run_in(&scenario, &["get", "/tasks/a"]);
    assert_eq!(text(&edited.stdout), r#"{"n": 9}"#);
}

#[test]
fn get_at_reads_a_commit_head_cannot_reach() {
    let (scenario, commits) = history();
    scenario.commit_at(&[CONFIG], &[], T + 1000, "a new root");
    let unreached = run_in(
        &scenario,
        &["get", "--at", &commits[2].to_string(), "/tasks/a"],
    );
    assert_eq!(
        (ended(&unreached), text(&unreached.stdout)),
        ((0, None), r#"{"n": 2}"#.to_owned())
    );
}

/// A library with two commits of `docs/a.md`, and a board whose first
/// commit pins a commit the library lacks, whose second pins the first
/// library commit and links `/links/to` to the file, and whose third pins
/// the second library commit.
fn moving_pin() -> (Scenario, Scenario, Vec<ObjectId>, Vec<ObjectId>) {
    let library = Scenario::new();
    let one = library.commit_at(&[("docs/a.md", "one")], &[], T, "one");
    let pins = vec![
        one,
        library.commit_at(&[("docs/a.md", "two")], &[one], T + 1, "two"),
    ];
    let board = Scenario::board();
    let mount = |commit| mount_of(&library.path(), commit);
    let link = (
        "links/to.json",
        r#"{"$type": "board/mount/v1", "path": "/repos/lib/docs/a.md"}"#,
    );
    let (missing, first, second) = (
        mount(ObjectId::Sha1([9; 20])),
        mount(pins[0]),
        mount(pins[1]),
    );
    let c0 = board.commit_at(&[CONFIG, ("repos/lib.json", &missing)], &[], T, "missing");
    let c1 = board.commit_at(
        &[CONFIG, ("repos/lib.json", &first), link],
        &[c0],
        T + 1,
        "first",
    );
    let c2 = board.commit_at(
        &[CONFIG, ("repos/lib.json", &second), link],
        &[c1],
        T + 2,
        "second",
    );
    (board, library, vec![c0, c1, c2], pins)
}

#[test]
fn a_path_through_a_link_and_a_mount_reads_at_an_old_commit_with_the_pin_that_commit_held() {
    let (board, _library, commits, _) = moving_pin();
    let read = |at: ObjectId, path: &str| {
        text(&run_in(&board, &["get", "--at", &at.to_string(), path]).stdout)
    };
    assert_eq!(read(commits[1], "/repos/lib/docs/a.md"), "one");
    assert_eq!(read(commits[1], "/links/to"), "one");
    assert_eq!(read(commits[2], "/repos/lib/docs/a.md"), "two");
    assert_eq!(read(commits[2], "/links/to"), "two");
}

#[test]
fn where_the_pin_is_missing_at_an_old_commit_the_mount_returns_its_record_and_a_path_beneath_is_refused()
 {
    let (board, _library, commits, _) = moving_pin();
    let at = commits[0].to_string();
    let own = run_in(&board, &["get", "--at", &at, "/repos/lib", "--json"]);
    assert_eq!(ended(&own), (0, None));
    assert_eq!(answers(&own)[0]["record"]["$type"], json!("board/mount/v1"));
    let beneath = run_in(&board, &["get", "--at", &at, "/repos/lib/docs/a.md"]);
    assert_eq!(ended(&beneath), (1, Some("board::mount::commit_missing")));
}

#[test]
fn log_json_and_text_match_their_golden_bytes() {
    let (scenario, _) = history();
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    let anonymous =
        |output: Output| text(&output.stdout).replace(&root.display().to_string(), "<root>");
    let report = run_in(&scenario, &["log", "/tasks/a", "--json"]);
    assert_eq!(ended(&report), (0, None));
    assert_eq!(anonymous(report), GOLDEN_LOG);
    let lines = run_in(&scenario, &["log", "/tasks/a"]);
    assert_eq!(text(&lines.stderr), "");
    assert_eq!(anonymous(lines), GOLDEN_LOG_TEXT);
}

/// `board log /tasks/a --json`, with the board's root as `<root>`.
const GOLDEN_LOG: &str = r#"{"schema":"board/log-report/v2","data":[{"commit":"412232fe6eef800f6878e3226356bf5c5728430a","time":"2023-11-14T22:17:20Z","subject":"board put /tasks/a","blob":"0810038cc65bb67e2d79da9f155689cc41682414"},{"commit":"300acd801ca9ebb1fdd45d96ccbb15112dabcb98","time":"2023-11-14T22:16:20Z","subject":"board rm /tasks/a","blob":null},{"commit":"419a46e014173625a52d494252454f4e6dfbc9db","time":"2023-11-14T22:15:20Z","subject":"board put /tasks/a","blob":"7dcae0bf83ff2086923358064cef18872f338b62"},{"commit":"7a98ad52855d32b234d9d23a304867b6a169fa9a","time":"2023-11-14T22:13:20Z","subject":"board put /tasks/a","blob":"e1ffef216a338ef12e5600097bc9b158f8fa58c4"}],"truncated":false,"summary":{"at":"412232fe6eef800f6878e3226356bf5c5728430a","walked":5,"complete":true,"location":{"display":"/tasks/a","path":"tasks/a.json","pointers":[],"repository":"<root>","source":"412232fe6eef800f6878e3226356bf5c5728430a"}}}
"#;

/// `board log /tasks/a`.
const GOLDEN_LOG_TEXT: &str = "\
412232fe6eef800f6878e3226356bf5c5728430a 2023-11-14T22:17:20Z board put /tasks/a
300acd801ca9ebb1fdd45d96ccbb15112dabcb98 2023-11-14T22:16:20Z board rm /tasks/a
419a46e014173625a52d494252454f4e6dfbc9db 2023-11-14T22:15:20Z board put /tasks/a
7a98ad52855d32b234d9d23a304867b6a169fa9a 2023-11-14T22:13:20Z board put /tasks/a
";

#[test]
fn log_limit_one_gives_the_newest_change_truncated_and_all_gives_every_change_complete() {
    let (scenario, commits) = history();
    let newest = json(&run_in(
        &scenario,
        &["log", "/tasks/a", "--limit", "1", "--json"],
    ));
    assert_eq!(newest["data"][0]["commit"], json!(commits[4].to_string()));
    assert_eq!(
        (
            newest["data"].as_array().unwrap().len(),
            newest["truncated"].clone()
        ),
        (1, json!(true))
    );
    assert_eq!(newest["summary"]["walked"], json!(1));
    let every = json(&run_in(&scenario, &["log", "/tasks/a", "--all", "--json"]));
    let listed: Vec<Value> = every["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["commit"].clone())
        .collect();
    let expected: Vec<Value> = [4, 3, 2, 0]
        .iter()
        .map(|at: &usize| json!(commits[*at].to_string()))
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(
        (
            every["truncated"].clone(),
            every["summary"]["complete"].clone()
        ),
        (json!(false), json!(true))
    );
    assert_eq!(
        every["data"][1]["blob"],
        Value::Null,
        "the removal has no blob"
    );
}

#[test]
fn log_at_a_shallow_boundary_is_truncated_and_not_complete_and_its_text_notes_why() {
    let (scenario, commits) = history();
    scenario.write(".git/shallow", Item::File(&format!("{}\n", commits[2])));
    let report = json(&run_in(&scenario, &["log", "/tasks/a", "--json"]));
    let summary = &report["summary"];
    assert_eq!(
        (
            report["truncated"].clone(),
            summary["complete"].clone(),
            summary["walked"].clone()
        ),
        (json!(true), json!(false), json!(3))
    );
    let listed = run_in(&scenario, &["log", "/tasks/a"]);
    let stderr = text(&listed.stderr);
    assert!(
        stderr.contains("note: the walk stopped after 3 commits"),
        "{stderr}"
    );
}

#[test]
fn log_of_a_symlinked_record_lists_each_edit_of_its_target_with_the_blob_get_reports() {
    let scenario = Scenario::board();
    let target = |n: &'static str| (b"tasks/a.json".as_slice(), Item::File(n));
    let config = (CONFIG.0.as_bytes(), Item::File(CONFIG.1));
    let link = (b"tasks/c.json".as_slice(), Item::Symlink("a.json"));
    let first = scenario.commit_items_at(&[config, target(r#"{"n": 1}"#)], &[], T, "put a");
    let linked = scenario.commit_items_at(
        &[config, target(r#"{"n": 1}"#), link],
        &[first],
        T + 60,
        "link c",
    );
    let edited = scenario.commit_items_at(
        &[config, target(r#"{"n": 2}"#), link],
        &[linked],
        T + 120,
        "edit a",
    );
    let log = json(&run_in(&scenario, &["log", "/tasks/c", "--json"]));
    let listed: Vec<Value> = (log["data"].as_array().unwrap().iter())
        .map(|row| row["commit"].clone())
        .collect();
    let expected = [edited, first].map(|commit| json!(commit.to_string()));
    let got = run_in(&scenario, &["get", "--at", "HEAD", "/tasks/c", "--json"]);
    assert_eq!(
        (listed, log["data"][0]["blob"].clone()),
        (expected.to_vec(), answers(&got)[0]["blob"].clone()),
        "the target's edits and get's blob, not the link's"
    );
}

#[test]
fn log_of_a_path_absent_at_head_is_not_found_naming_at_and_at_the_commit_before_the_removal_lists_it()
 {
    let (scenario, commits) = history();
    scenario.commit_at(
        &[CONFIG, ("tasks/b.json", "{}")],
        &[commits[4]],
        T + 600,
        "board rm /tasks/a",
    );
    let absent = run_in(&scenario, &["log", "/tasks/a"]);
    assert_eq!(ended(&absent), (1, Some("board::path::not_found")));
    let stderr = text(&absent.stderr);
    assert!(
        stderr.contains("`board log /tasks` lists the commits") && stderr.contains("--at"),
        "{stderr}"
    );
    let before = run_in(
        &scenario,
        &["log", "/tasks/a", "--at", &commits[4].to_string(), "--json"],
    );
    assert_eq!(ended(&before), (0, None));
    assert_eq!(json(&before)["data"].as_array().unwrap().len(), 4);
}

#[test]
fn the_hint_for_a_removed_path_says_log_of_its_ancestor_lists_the_commit_that_removed_it() {
    let (scenario, commits) = history();
    let removal = scenario.commit_at(
        &[CONFIG, ("tasks/b.json", "{}")],
        &[commits[4]],
        T + 600,
        "board rm /tasks/a",
    );
    let stderr = text(&run_in(&scenario, &["log", "/tasks/a"]).stderr);
    assert!(
        stderr.contains("`board log /tasks` lists the commits that changed /tasks")
            && stderr.contains("the commit that removed the path"),
        "{stderr}"
    );
    let ancestor = json(&run_in(&scenario, &["log", "/tasks", "--json"]));
    assert_eq!(ancestor["data"][0]["commit"], json!(removal.to_string()));
}

#[test]
fn in_a_board_with_no_commit_log_is_not_found_and_get_at_head_names_no_commit_until_the_first() {
    let scenario = Scenario::board();
    scenario.write("tasks/a.json", Item::File("{}"));
    let log = run_in(&scenario, &["log"]);
    assert_eq!(ended(&log), (1, Some("board::path::not_found")));
    assert!(
        text(&log.stderr).contains("the board has no commit yet"),
        "{}",
        text(&log.stderr)
    );
    let get = run_in(&scenario, &["get", "--at", "HEAD", "/tasks/a"]);
    assert_eq!(ended(&get), (1, Some("board::commit::not_found")));
    scenario.commit_at(&[CONFIG, ("tasks/a.json", "{}")], &[], T, "first");
    assert_eq!(ended(&run_in(&scenario, &["log"])), (0, None));
    assert_eq!(
        ended(&run_in(&scenario, &["get", "--at", "HEAD", "/tasks/a"])),
        (0, None)
    );
}

#[test]
fn log_through_a_mount_lists_the_mounted_repositorys_commits_and_names_that_repository() {
    let (board, library, commits, pins) = moving_pin();
    let at = commits[2].to_string();
    let report = json(&run_in(
        &board,
        &["log", "/repos/lib/docs/a.md", "--at", &at, "--json"],
    ));
    let listed: Vec<Value> = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["commit"].clone())
        .collect();
    assert_eq!(
        listed,
        [json!(pins[1].to_string()), json!(pins[0].to_string())]
    );
    let repository = std::fs::canonicalize(library.workdir()).unwrap();
    assert_eq!(
        report["summary"]["location"]["repository"],
        json!(repository.display().to_string())
    );
    assert_eq!(report["summary"]["at"], json!(at));
}

#[test]
fn get_at_a_row_id_from_a_log_through_a_mount_is_not_found_and_the_hint_says_why_log_gave_it() {
    let (board, _library, _commits, pins) = moving_pin();
    let refused = run_in(
        &board,
        &["get", "--at", &pins[1].to_string(), "/repos/lib/docs/a.md"],
    );
    assert_eq!(ended(&refused), (1, Some("board::commit::not_found")));
    let stderr = text(&refused.stderr);
    assert!(
        stderr.contains("through a mount lists the mounted repository's commits")
            && stderr.contains("`get --at` on this board cannot read"),
        "{stderr}"
    );
    assert!(!stderr.contains("lists the board's commits"), "{stderr}");
}

#[test]
fn both_reads_answer_while_another_process_holds_the_write_lock_and_a_put_then_is_locked() {
    let (scenario, _) = history();
    scenario.identified();
    let held = scenario.flocked(".git/board.lock");
    let cache = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str], stdin: &[u8]| {
        board_with(scenario.workdir(), Some(cache.path()), arguments, stdin)
    };
    assert_eq!(
        ended(&run(&["get", "--at", "HEAD", "/tasks/a"], b"")),
        (0, None)
    );
    assert_eq!(ended(&run(&["log", "/tasks/a"], b"")), (0, None));
    assert_eq!(
        std::fs::read_dir(cache.path()).unwrap().count(),
        0,
        "neither read writes the index"
    );
    let put = run(&["put", "/tasks/c", "--wait", "0s"], br#"{"n": 1}"#);
    assert_eq!(ended(&put), (1, Some("board::put::locked")));
    drop(held);
}
