//! `board get` of one path or several.

use board_model::{Code, Fields, LogicalPath};
use board_ops::ReadError;
use board_scenario::{Item, Scenario};
use board_tree::GitError;
use serde_json::Value;

use super::{board, board_cached, json, text};
use crate::render::{self, Format};
use crate::report::Output;

/// The one answer in a `board get --json` run of one path.
pub(super) fn got(output: &Output) -> Value {
    let report = json(output);
    assert_eq!(report["$type"], "board/get-report/v1");
    assert_eq!(report["answers"].as_array().map(Vec::len), Some(1));
    report["answers"][0].clone()
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
const GOLDEN_GET: &str = r##"{
  "$type": "board/get-report/v1",
  "answers": [
    {
      "blob": "350ab0b8b289baeaf217fc2914e660f59ee25faf",
      "location": {
        "display": "/projects/p",
        "path": "projects/p.json",
        "pointers": [],
        "repository": "<root>",
        "source": "worktree"
      },
      "record": {
        "$type": "lr/project/v1"
      },
      "text": "{\"$type\": \"lr/project/v1\"}"
    },
    {
      "blob": "fa736015ccf0a4d977752508b6e716d8bbaa028b",
      "code": "board::record::not_a_record",
      "location": {
        "display": "/notes.md",
        "path": "notes.md",
        "pointers": [],
        "repository": "<root>",
        "source": "worktree"
      },
      "text": "# n\n"
    },
    {
      "base64": "/wBh",
      "blob": "0a853c868c31f8e9991aa72662ad9eab6eabbc6b",
      "code": "board::record::not_a_record",
      "location": {
        "display": "/raw.bin",
        "path": "raw.bin",
        "pointers": [],
        "repository": "<root>",
        "source": "worktree"
      }
    },
    {
      "blob": "49da92e55c925be83ecfab7287828b6eb13dddf2",
      "code": "board::record::bad_type",
      "location": {
        "display": "/bad",
        "path": "bad.json",
        "pointers": [],
        "repository": "<root>",
        "source": "worktree"
      },
      "text": "{\"$type\": \"task\"}"
    },
    {
      "$type": "board/error/v1",
      "code": "board::path::not_found",
      "fields": {
        "ancestor": "/",
        "path": "/zz"
      },
      "hint": "run `board ls /` to see what is there; paths leave out `.json`",
      "message": "nothing is at /zz"
    }
  ]
}
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
    assert!(
        alone.starts_with("error: board::path::not_found: "),
        "{alone}"
    );
    assert!(alone.contains("\nhint: "), "{alone}");
    assert_eq!(
        text(&refused.stderr),
        alone
            .replacen("error: ", "error: /zz: ", 1)
            .replacen("hint: ", "hint: /zz: ", 1),
        "the refusal is printed as it is alone, each line naming the path asked"
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
    let report = json(&several);
    assert_eq!(report["$type"], "board/get-report/v1");
    let alone: Vec<Value> = paths
        .iter()
        .map(|path| got(&run(&["get", path, "--json"])))
        .collect();
    assert_eq!(report["answers"], Value::Array(alone));
}

#[test]
fn too_many_admits_as_many_paths_as_the_most_and_refuses_one_more() {
    assert!(render::too_many(Format::Json, "get", 3, 3).is_none());
    let refused = render::too_many(Format::Json, "get", 4, 3).unwrap();
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (2, Some("board::usage::invalid"))
    );
    let error = json(&refused);
    assert_eq!(error["$type"], "board/error/v1");
    assert_eq!(
        error["message"],
        "get was given 4 paths, and one run answers at most 3"
    );
    assert_eq!(
        error["hint"],
        "split the paths over several runs of at most 3 each"
    );
}

#[test]
fn get_help_and_the_over_budget_hint_name_the_budget_and_the_path_limit_the_cli_uses() {
    let budget = format!("{} MiB", render::GET_BUDGET >> 20);
    assert_eq!(render::GET_BUDGET % (1 << 20), 0);
    assert!(render::GET_HELP.contains(&budget), "{budget}");
    let paths = format!("takes at most {}", board_ops::MAX_LIMIT);
    assert!(render::GET_HELP.contains(&paths), "{paths}");
    let hint = Code::GetOverBudget.hint(&Fields::at(&"/a".parse().unwrap()));
    assert!(hint.contains(&format!("up to {budget}")), "{hint}");
}

/// `paths`, each read from `scenario` and rendered as `board get` renders
/// them, within `budget` bytes of output.
fn rendered(scenario: &Scenario, paths: &[&str], format: Format, budget: usize) -> Output {
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths: Vec<LogicalPath> = paths.iter().map(|path| path.parse().unwrap()).collect();
    render::get(format, &paths, board.get_each(&paths), budget)
}

/// A board holding /a.md, /b.md and /c.md, each the 6 bytes `12345\n`.
fn three_files() -> Scenario {
    let scenario = board();
    for name in ["a", "b", "c"] {
        scenario.write(&format!("{name}.md"), Item::File("12345\n"));
    }
    scenario
}

#[test]
fn get_reads_a_path_while_the_run_has_printed_less_than_its_budget_and_always_the_first() {
    let scenario = three_files();
    let codes = |budget: usize| -> (Vec<u8>, Option<String>) {
        let ran = rendered(
            &scenario,
            &["/a.md", "/b.md", "/c.md"],
            Format::Text,
            budget,
        );
        (ran.stdout, ran.error)
    };
    // `==> /a.md <==\n12345\n` is 20 bytes, and each later file 21 more.
    let two = b"==> /a.md <==\n12345\n\n==> /b.md <==\n12345\n".to_vec();
    let three = [two.as_slice(), b"\n==> /c.md <==\n12345\n"].concat();
    assert_eq!(codes(42), (three, None), "41 bytes are under 42");
    let over = Some("board::get::over_budget".to_owned());
    assert_eq!(codes(41), (two, over.clone()), "41 bytes reach 41");
    let first = b"==> /a.md <==\n12345\n".to_vec();
    assert_eq!(codes(0), (first, over), "the first path is always read");
}

#[test]
fn a_path_after_the_budget_is_spent_is_answered_with_its_error_in_its_slot_and_fails_the_run() {
    let scenario = three_files();
    let paths = ["/a.md", "/b.md", "/c.md"];
    let refused = rendered(&scenario, &paths, Format::Text, 41);
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::get::over_budget"))
    );
    assert_eq!(
        text(&refused.stderr),
        "error: /c.md: board::get::over_budget: /c.md was not read: this run already \
         printed its budget of 41 bytes\n\
         hint: /c.md: one run prints up to 64 MiB; ask for /c.md and the paths \
         after it in another `board get` run\n"
    );
    let report = json(&rendered(&scenario, &paths, Format::Json, 1));
    let answers = report["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0]["text"], "12345\n");
    assert_eq!(answers[1]["$type"], "board/error/v1");
    assert_eq!(answers[1]["code"], "board::get::over_budget");
    assert_eq!(answers[1]["fields"]["path"], "/b.md");
    assert_eq!(answers[2]["code"], "board::get::over_budget");
}

#[test]
fn the_budget_counts_the_json_a_file_prints_so_escaped_bytes_spend_more_of_it() {
    let scenario = board();
    scenario
        .write("p.md", Item::File("abcdef"))
        .write("c.md", Item::Bytes(b"\x01\x01\x01\x01\x01\x01"))
        .write("z.md", Item::File("z"));
    // The plain file alone, as JSON, is the budget: it printed less than
    // that before /z.md, so /z.md is read. The control bytes are as many,
    // but each prints as the six bytes `\u0001`, which spend the budget.
    let budget = rendered(&scenario, &["/p.md"], Format::Json, usize::MAX)
        .stdout
        .len();
    let plain = rendered(&scenario, &["/p.md", "/z.md"], Format::Json, budget);
    assert_eq!((plain.code, plain.error), (0, None));
    let escaped = rendered(&scenario, &["/c.md", "/z.md"], Format::Json, budget);
    assert_eq!(
        (escaped.code, escaped.error.as_deref()),
        (1, Some("board::get::over_budget"))
    );
    assert_eq!(json(&escaped)["answers"][0]["text"], "\u{1}".repeat(6));
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
    let report = json(&render::get(Format::Json, &paths, failed(), usize::MAX));
    let answers = report["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0]["$type"], "board/error/v1");
    assert_eq!(answers[0], answers[1]);
    let ran = render::get(Format::Text, &paths, failed(), usize::MAX);
    assert_eq!(ran.code, 1);
    let lines = text(&ran.stderr);
    assert!(lines.starts_with("error: /a: "), "{lines}");
    assert!(lines.contains("\nerror: /b: "), "{lines}");
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
