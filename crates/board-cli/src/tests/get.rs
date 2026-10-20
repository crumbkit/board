//! `board get` of one path or several.

use board_model::LogicalPath;
use board_ops::ReadError;
use board_scenario::{Item, Scenario};
use board_tree::GitError;
use serde_json::Value;

use super::{answers, board, board_cached, board_with, json, text};
use crate::render::{self, Printer};
use crate::report::Output;

/// The one answer in a `board get --json` run of one path.
pub(super) fn got(output: &Output) -> Value {
    assert_eq!(json(output)["schema"], "board/get-report/v2");
    let answers = answers(output);
    assert_eq!(answers.len(), 1);
    answers[0].clone()
}

/// A board holding a record, a text file, a file that is not UTF-8 and a
/// JSON file that is not a record.
fn files() -> Scenario {
    let scenario = board();
    scenario
        .write("notes.md", Item::File("# n\n"))
        .write("raw.bin", Item::Bytes(b"\xff\x00a"))
        .write("bad.json", Item::File(r#"{"$type": "task"}"#));
    scenario
}

#[test]
fn get_json_is_one_report_whose_answers_match_their_golden_bytes() {
    let scenario = files();
    let ran = board_cached(
        scenario.workdir(),
        None,
        &[
            "get",
            "/projects/p",
            "/notes.md",
            "/raw.bin",
            "/bad",
            "/zz",
            "--json",
        ],
    );
    assert_eq!(
        (ran.code, ran.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    let printed = text(&ran.stdout).replace(&root.display().to_string(), "<root>");
    assert_eq!(printed, GOLDEN_GET);
}

/// `board get /projects/p /notes.md /raw.bin /bad /zz --json`, with the
/// board's root as `<root>`.
const GOLDEN_GET: &str = r##"{"schema":"board/get-report/v2","data":[{"location":{"display":"/projects/p","path":"projects/p.json","pointers":[],"repository":"<root>","source":"worktree"},"blob":"350ab0b8b289baeaf217fc2914e660f59ee25faf","text":"{\"$type\": \"lr/project/v1\"}","record":{"$type":"lr/project/v1"}},{"location":{"display":"/notes.md","path":"notes.md","pointers":[],"repository":"<root>","source":"worktree"},"blob":"fa736015ccf0a4d977752508b6e716d8bbaa028b","text":"# n\n","code":"board::record::not_a_record"},{"location":{"display":"/raw.bin","path":"raw.bin","pointers":[],"repository":"<root>","source":"worktree"},"blob":"0a853c868c31f8e9991aa72662ad9eab6eabbc6b","base64":"/wBh","code":"board::record::not_a_record"},{"location":{"display":"/bad","path":"bad.json","pointers":[],"repository":"<root>","source":"worktree"},"blob":"49da92e55c925be83ecfab7287828b6eb13dddf2","text":"{\"$type\": \"task\"}","code":"board::record::bad_type"}],"failures":[{"key":"/zz","index":4,"code":"board::path::not_found","message":"nothing is at /zz","fields":{"ancestor":"/","path":"/zz"},"recovery":{"kind":"hint","text":"run `board ls /` to see what is there; paths leave out `.json`"}}]}
"##;

#[test]
fn get_of_several_paths_answers_each_as_alone_and_one_that_names_nothing_fails_the_run() {
    let scenario = files();
    let cache = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| board_cached(scenario.workdir(), Some(cache.path()), arguments);
    let admitted = run(&["get", "/notes.md", "/projects/p"]);
    assert_eq!((admitted.code, admitted.error.as_deref()), (0, None));
    assert_eq!(
        text(&admitted.stdout),
        "==> /notes.md <==\n# n\n\n==> /projects/p <==\n{\"$type\": \"lr/project/v1\"}"
    );
    assert_eq!(text(&admitted.stderr), "");
    let refused = run(&["get", "/notes.md", "/zz", "/projects/p"]);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    assert_eq!(refused.stdout, admitted.stdout);
    let alone = text(&run(&["get", "/zz"]).stderr);
    assert!(alone.starts_with("error: nothing is at /zz\n"), "{alone}");
    assert!(alone.contains("\n  help:  "), "{alone}");
    assert_eq!(
        text(&refused.stderr),
        alone.replacen("(index 0)", "(index 1)", 1),
        "the refusal is printed as it is alone, but for the index of its path"
    );
    let paths = ["/notes.md", "/zz", "/raw.bin", "/bad", "/projects/p"];
    let mut arguments = vec!["get"];
    arguments.extend(paths);
    arguments.push("--json");
    let several = run(&arguments);
    assert_eq!(
        (several.code, several.error.as_deref()),
        (1, Some("board::path::not_found"))
    );
    assert_eq!(json(&several)["schema"], "board/get-report/v2");
    let alone: Vec<Value> = paths
        .iter()
        .map(|path| got(&run(&["get", path, "--json"])))
        .collect();
    let mut merged = answers(&several);
    // A failure names its key's index in the run, which alone is 0.
    merged[1]["index"] = 0.into();
    assert_eq!(merged, alone);
}

#[test]
fn get_answers_at_most_max_limit_paths_and_refuses_one_more() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let run = |count: usize| {
        let mut arguments = vec!["get"];
        arguments.extend(std::iter::repeat_n("/projects/p", count));
        board_cached(scenario.workdir(), Some(cache.path()), &arguments)
    };
    assert_eq!(run(board_ops::MAX_LIMIT).code, 0);
    let refused = run(board_ops::MAX_LIMIT + 1);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (2, Some("board::cli::usage"))
    );
    assert!(refused.stdout.is_empty());
    let stderr = String::from_utf8(refused.stderr).unwrap();
    let split = format!(
        "help:  split the paths over several runs of at most {} each",
        board_ops::MAX_LIMIT
    );
    assert!(stderr.contains(&split), "{stderr}");
}

#[test]
fn get_help_names_the_budget_and_the_path_limit_the_cli_uses() {
    let budget = format!("{} MiB", render::BUDGET >> 20);
    assert_eq!(render::BUDGET % (1 << 20), 0);
    assert!(render::GET_HELP.contains(&budget), "{budget}");
    let paths = format!("At most {} paths", board_ops::MAX_LIMIT);
    assert!(render::GET_HELP.contains(&paths), "{paths}");
}

/// `paths`, each read from `scenario` and rendered as `board get` renders
/// them, within `budget` bytes of output.
fn rendered(scenario: &Scenario, paths: &[&str], format: bool, budget: usize) -> Output {
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths: Vec<LogicalPath> = paths.iter().map(|path| path.parse().unwrap()).collect();
    render::get(
        &mut Printer::of(format, budget, &["get"]),
        &paths,
        board.get_each(&paths),
    )
}

/// The 1,200 bytes of each file `three_files` writes: crumbkit refuses a
/// budget under 1,024 bytes, so each file takes more than that.
fn file() -> String {
    format!("{}\n", "x".repeat(1199))
}

/// A board holding /a.md, /b.md and /c.md, each [`file`].
fn three_files() -> Scenario {
    let scenario = board();
    for name in ["a", "b", "c"] {
        scenario.write(&format!("{name}.md"), Item::File(&file()));
    }
    scenario
}

#[test]
fn get_prints_each_file_that_fits_in_its_budget_and_always_the_first() {
    let scenario = three_files();
    let codes = |budget: usize| -> (Vec<u8>, Option<String>) {
        let ran = rendered(&scenario, &["/a.md", "/b.md", "/c.md"], false, budget);
        (ran.stdout, ran.error)
    };
    // `==> /a.md <==\n` and the file are 1,214 bytes, and each later file
    // 1,215 more, with the blank line before its header.
    let headed = |name: &str| format!("\n==> /{name}.md <==\n{}", file());
    let first = headed("a")[1..].to_owned();
    let two = first.clone() + &headed("b");
    let three = two.clone() + &headed("c");
    let over = Some("board::output::over_budget".to_owned());
    assert_eq!(
        codes(3644),
        (three.into_bytes(), None),
        "3,644 bytes fit in 3,644"
    );
    assert_eq!(
        codes(3643),
        (two.clone().into_bytes(), over.clone()),
        "the third file passes 3,643 by one byte"
    );
    assert_eq!(
        codes(2429),
        (two.into_bytes(), over.clone()),
        "two files reach 2,429, so the third is not read"
    );
    assert_eq!(
        codes(1024),
        (first.into_bytes(), over),
        "the first path is always read, even past the budget"
    );
}

#[test]
fn a_path_after_the_budget_is_spent_is_answered_with_its_error_in_its_slot_and_fails_the_run() {
    let scenario = three_files();
    let paths = ["/a.md", "/b.md", "/c.md"];
    let refused = rendered(&scenario, &paths, false, 2429);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::output::over_budget"))
    );
    assert_eq!(
        text(&refused.stderr),
        "error: 1 more answer did not fit in the output budget of 2429 bytes\n\
         \x20 help:  run `board get /c.md`\n\
         \x20 code:  board::output::over_budget\n"
    );
    // Room for the first answer and the failure that leaves the other two
    // out, but not for /b.md's file.
    let budget = rendered(&scenario, &paths[..1], true, usize::MAX)
        .stdout
        .len()
        + 300;
    let report = json(&rendered(&scenario, &paths, true, budget));
    assert_eq!(report["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["data"][0]["text"], file());
    let over = &report["failures"][0];
    assert_eq!(over["code"], "board::output::over_budget");
    assert_eq!(over["fields"]["budget"], budget);
    assert_eq!(
        over["fields"]["left_out"],
        serde_json::json!(["/b.md", "/c.md"])
    );
    assert_eq!(over["fields"]["left_out_from"], 1);
    assert_eq!(over["recovery"]["call"], "board get <PATH>...");
}

/// crumbkit reruns the caller's own words with the paths left out, so the
/// recovery keeps `-C` and `--json`, and the line text prints runs. Words
/// that do not hold the paths fall back to the command alone.
#[test]
fn an_over_budget_get_recovers_with_the_callers_flags_and_without_its_words_names_the_command() {
    let scenario = three_files();
    scenario.write("sub/keep.md", Item::File("k"));
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths = ["/a.md", "/b.md", "/c.md"];
    let asked: Vec<LogicalPath> = paths.iter().map(|path| path.parse().unwrap()).collect();
    let ran = |json: bool, budget: usize, words: &[&str]| {
        let printer = &mut Printer::of(json, budget, words);
        render::get(printer, &asked, board.get_each(&asked))
    };
    let budget = rendered(&scenario, &paths[..1], true, usize::MAX)
        .stdout
        .len()
        + 300;
    let typed = ["-C", "sub", "get", "/a.md", "/b.md", "/c.md", "--json"];
    assert_eq!(
        json(&ran(true, budget, &typed))["failures"][0]["recovery"],
        serde_json::json!({
            "kind": "run",
            "call": "board -C sub get <PATH>... --json",
            "all": "left_out",
        })
    );
    let bare = json(&ran(true, budget, &["get"]));
    assert_eq!(
        bare["failures"][0]["recovery"]["call"],
        "board get <PATH>..."
    );
    let typed = ["-C", "sub", "get", "/a.md", "/b.md", "/c.md"];
    assert_eq!(
        super::hint_of(&ran(false, 2429, &typed)),
        "run `board -C sub get /c.md`"
    );
    let rerun = super::board_in(scenario.workdir(), &["-C", "sub", "get", "/c.md"]);
    assert_eq!((rerun.code, rerun.stdout), (0, file().into_bytes()));
}

#[test]
fn the_budget_counts_the_json_a_file_prints_so_escaped_bytes_spend_more_of_it() {
    let scenario = board();
    scenario
        .write("p.md", Item::File(&"a".repeat(800)))
        .write("c.md", Item::Bytes(&[1; 800]))
        .write("z.md", Item::File("z"));
    // The plain file and /z.md, as JSON, are the budget, so both print. The
    // control bytes are as many, but each prints as the six bytes
    // `\u0001`, which spend the budget.
    let budget = rendered(&scenario, &["/p.md", "/z.md"], true, usize::MAX)
        .stdout
        .len();
    let plain = rendered(&scenario, &["/p.md", "/z.md"], true, budget);
    assert_eq!((plain.code, plain.error), (0, None));
    let escaped = rendered(&scenario, &["/c.md", "/z.md"], true, budget);
    assert_eq!(
        (escaped.code, escaped.error.as_deref()),
        (1, Some("board::output::over_budget"))
    );
    assert_eq!(answers(&escaped)[0]["text"], "\u{1}".repeat(800));
}

#[test]
fn a_board_that_cannot_be_read_answers_every_path_with_its_one_error() {
    let paths: Vec<LogicalPath> = ["/a", "/b"]
        .iter()
        .map(|path| path.parse().unwrap())
        .collect();
    let failed = || -> Result<std::iter::Empty<_>, ReadError> {
        let source = GitError::NoWorkingTree {
            repository: "/board".into(),
        };
        Err(ReadError::Git { source })
    };
    let ran = render::get(&mut Printer::of(true, 1 << 20, &["get"]), &paths, failed());
    let failures = json(&ran)["failures"].as_array().cloned().unwrap();
    assert_eq!(failures.len(), 2);
    assert_eq!(
        (&failures[0]["key"], &failures[1]["key"]),
        (&"/a".into(), &"/b".into())
    );
    assert_eq!(failures[0]["code"], failures[1]["code"]);
    let ran = render::get(&mut Printer::of(false, 1 << 20, &["get"]), &paths, failed());
    assert_eq!(ran.code, 1);
    let lines = text(&ran.stderr);
    assert!(lines.contains("  key:   /a (index 0)\n"), "{lines}");
    assert!(lines.contains("  key:   /b (index 1)\n"), "{lines}");
}

#[test]
fn get_of_one_path_prints_its_bytes_alone_and_of_two_heads_each_with_its_path() {
    let scenario = files();
    let one = board_cached(scenario.workdir(), None, &["get", "/raw.bin"]);
    assert_eq!((one.code, one.stdout.as_slice()), (0, &b"\xff\x00a"[..]));
    let two = board_cached(scenario.workdir(), None, &["get", "/raw.bin", "/raw.bin"]);
    assert_eq!(
        (two.code, two.stdout.as_slice()),
        (
            0,
            &b"==> /raw.bin <==\n\xff\x00a\n==> /raw.bin <==\n\xff\x00a"[..]
        )
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test removes what the store holds"
)]
fn remove(location: &str) {
    std::fs::remove_file(location).unwrap();
}

#[test]
fn get_fetch_prints_an_attachments_bytes_from_the_store_and_without_fetch_its_record() {
    let store = tempfile::tempdir().unwrap();
    let scenario = board();
    let config = serde_json::json!({
        "$type": "board/config/v1",
        "commit_on_put": false,
        "store": {"backend": "directory", "location": store.path().display().to_string()},
    });
    scenario.write(".board/board.json", Item::File(&config.to_string()));
    let bytes: Vec<u8> = (0..3000u32)
        .map(|i| u8::try_from(i % 253).unwrap_or_default())
        .collect();
    let run =
        |arguments: &[&str], stdin: &[u8]| board_with(scenario.workdir(), None, arguments, stdin);
    let put = run(&["put", "/files/blob.bin", "--file", "--json"], &bytes);
    assert_eq!((put.code, put.error.as_deref()), (0, None));
    assert_eq!(
        json(&put)["data"]["writes"][0]["location"]["path"],
        "files/blob.bin.json"
    );
    let shown = run(&["get", "/files/blob.bin"], b"");
    assert_eq!((shown.code, shown.error.as_deref()), (0, None));
    assert!(text(&shown.stdout).contains("\"board/attachment/v1\""));
    let fetched = run(&["get", "/files/blob.bin", "--fetch"], b"");
    assert_eq!((fetched.code, fetched.error.as_deref()), (0, None));
    assert_eq!(fetched.stdout, bytes);
    let answer = got(&run(&["get", "/files/blob.bin", "--fetch", "--json"], b""));
    assert_eq!(answer["record"]["$type"], "board/attachment/v1");
    assert_eq!(answer["record"]["size"], 3000);
    assert!(answer["base64"].is_string());
    assert_eq!(
        run(&["get", "/projects/p", "--fetch"], b"").stdout,
        run(&["get", "/projects/p"], b"").stdout
    );
    remove(answer["record"]["location"].as_str().unwrap());
    let gone = run(&["get", "/files/blob.bin", "--fetch"], b"");
    assert_eq!(
        (gone.code, gone.error.as_deref()),
        (1, Some("board::attachment::missing"))
    );
    assert!(
        text(&gone.stderr).starts_with("error: /files/blob.bin names "),
        "{}",
        text(&gone.stderr)
    );
    assert_eq!(run(&["get", "/files/blob.bin"], b"").code, 0);
}
