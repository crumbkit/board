//! `board refs --field`, and the budget of a `refs` run.

use board_model::{Code, Fields, LogicalPath};
use board_ops::{Absent, CheckError, RefsError, RefsReport};
use board_scenario::{Item, Scenario};
use board_tree::{GitError, ResolveError};

use super::{board, board_in, json, text};
use crate::render::{self, Format};
use crate::report::Output;

/// A board whose /hub refers to a project through a link, a markdown file,
/// nothing, and a task its schema refuses.
fn hub() -> Scenario {
    let scenario = board();
    scenario
        .write(
            ".board/types/x/hub/v1.schema.json",
            Item::File(
                r#"{"type": "object", "properties": {"see": {"type": "array", "items": {"type": "string", "format": "board-ref"}}}}"#,
            ),
        )
        .write(
            "links/l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        )
        .write("notes.md", Item::File("# n\n"))
        .write(
            "tasks/bad.json",
            Item::File(r#"{"$type": "lr/task/v1", "status": "done"}"#),
        )
        .write(
            "hub.json",
            Item::File(
                r#"{"$type": "x/hub/v1", "see": ["/links/l", "/notes.md", "/zz", "/tasks/bad"]}"#,
            ),
        );
    scenario
}

#[test]
fn refs_fields_print_each_targets_values_or_its_code_on_every_from_line() {
    let ran = board_in(
        hub().workdir(),
        &["refs", "/hub", "--field", "/status", "--field", "/$type"],
    );
    assert_eq!((ran.code, ran.error), (0, None));
    assert_eq!(
        text(&ran.stdout),
        "from /see/0 /links/l ok target=- \"/status\"=absent \"/$type\"=\"lr/project/v1\"\n\
         from /see/1 /notes.md ok target=board::record::not_a_record\n\
         from /see/2 /zz degraded board::ref::dangling\n\
         from /see/3 /tasks/bad ok target=board::record::invalid \"/status\"=\"done\" \"/$type\"=\"lr/task/v1\"\n"
    );
    let without = board_in(hub().workdir(), &["refs", "/hub"]);
    assert!(
        text(&without.stdout).starts_with("from /see/0 /links/l ok\nfrom /see/1"),
        "no fields, no values"
    );
}

#[test]
fn refs_fields_json_matches_its_golden_bytes() {
    let scenario = hub();
    let ran = board_in(
        scenario.workdir(),
        &[
            "refs", "/hub", "--field", "/status", "--field", "/$type", "--limit", "4", "--json",
        ],
    );
    assert_eq!((ran.code, ran.error), (0, None));
    let root = std::fs::canonicalize(scenario.workdir()).unwrap();
    let printed = text(&ran.stdout).replace(&root.display().to_string(), "<root>");
    assert_eq!(printed, GOLDEN_FIELDS);
}

/// `board refs /hub --field /status --field /$type --limit 4 --json`, with
/// the board's root as `<root>`.
const GOLDEN_FIELDS: &str = r#"{
  "$type": "board/refs-report/v1",
  "answers": [
    {
      "from": [
        {
          "location": {
            "display": "/projects/p",
            "path": "projects/p.json",
            "pointers": [
              {
                "path": "links/l.json",
                "repository": "<root>",
                "source": "worktree"
              }
            ],
            "repository": "<root>",
            "source": "worktree"
          },
          "pointer": "/see/0",
          "status": "ok",
          "target": "/links/l",
          "values": [
            {
              "absent": true,
              "pointer": "/status"
            },
            {
              "pointer": "/$type",
              "value": "lr/project/v1"
            }
          ]
        },
        {
          "location": {
            "display": "/notes.md",
            "path": "notes.md",
            "pointers": [],
            "repository": "<root>",
            "source": "worktree"
          },
          "pointer": "/see/1",
          "status": "ok",
          "target": "/notes.md",
          "target_code": "board::record::not_a_record"
        },
        {
          "code": "board::ref::dangling",
          "location": null,
          "pointer": "/see/2",
          "status": "degraded",
          "target": "/zz"
        },
        {
          "location": {
            "display": "/tasks/bad",
            "path": "tasks/bad.json",
            "pointers": [],
            "repository": "<root>",
            "source": "worktree"
          },
          "pointer": "/see/3",
          "status": "ok",
          "target": "/tasks/bad",
          "target_code": "board::record::invalid",
          "values": [
            {
              "pointer": "/status",
              "value": "done"
            },
            {
              "pointer": "/$type",
              "value": "lr/task/v1"
            }
          ]
        }
      ],
      "location": {
        "display": "/hub",
        "path": "hub.json",
        "pointers": [],
        "repository": "<root>",
        "source": "worktree"
      },
      "missing": null,
      "to": [],
      "total": 4,
      "truncated": false
    }
  ]
}
"#;

#[test]
fn a_field_that_is_not_a_json_pointer_refuses_the_run_and_its_twin_that_is_one_is_admitted() {
    let scenario = hub();
    let run = |field: &str| {
        let arguments = ["refs", "/hub", "--field", field, "--limit", "1"];
        board_in(scenario.workdir(), &arguments)
    };
    for (refused, admitted) in [("status", "/status"), ("/a~2", "/a~1"), ("/a~", "/a~0")] {
        let ran = run(refused);
        assert_eq!(
            (ran.code, ran.error.as_deref()),
            (2, Some("board::usage::invalid")),
            "{refused}"
        );
        assert_eq!(
            text(&ran.stderr),
            format!(
                "error: board::usage::invalid: invalid value '{refused}' for '--field <POINTER>': \
                 must be a JSON pointer, such as /status, or /a~1b for the key a/b: empty or \
                 starting with /, with ~ only as ~0 or ~1\n\
                 hint: run `board refs --help` for its usage and examples\n"
            )
        );
        let ran = run(admitted);
        assert_eq!((ran.code, ran.error), (0, None), "{admitted}");
        assert_eq!(
            text(&ran.stdout),
            format!("from /see/0 /links/l ok target=- \"{admitted}\"=absent\n")
        );
    }
    let ran = run("");
    assert_eq!((ran.code, ran.error), (0, None), "the empty pointer");
    assert!(text(&ran.stdout).contains(r#" target=- ""={"$type":"lr/project/v1""#));
}

/// A `from` line's fields read back: its target's code, then each pointer
/// with its value, or `None` where it is absent.
fn read_fields(line: &str) -> (String, Vec<(String, Option<serde_json::Value>)>) {
    let rest = &line[line.find(" target=").unwrap() + " target=".len()..];
    let (code, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let mut values = Vec::new();
    while !rest.is_empty() {
        let mut pointers = serde_json::Deserializer::from_str(rest).into_iter::<String>();
        let pointer = pointers.next().unwrap().unwrap();
        rest = rest[pointers.byte_offset()..].strip_prefix('=').unwrap();
        let value = if let Some(after) = rest.strip_prefix("absent") {
            rest = after;
            None
        } else {
            let mut found = serde_json::Deserializer::from_str(rest).into_iter();
            let value = found.next().unwrap().unwrap();
            rest = &rest[found.byte_offset()..];
            Some(value)
        };
        rest = rest.strip_prefix(' ').unwrap_or(rest);
        values.push((pointer, value));
    }
    (code.to_owned(), values)
}

#[test]
fn a_from_line_reads_back_when_a_pointer_holds_a_space_and_an_equals_and_a_value_holds_target() {
    let scenario = board();
    scenario
        .write(
            ".board/types/x/note/v1.schema.json",
            Item::File(
                r#"{"type": "object", "properties": {"see": {"type": "string", "format": "board-ref"}}}"#,
            ),
        )
        .write(
            "n.json",
            Item::File(r#"{"$type": "x/note/v1", "a b=c": "x target=y", "see": "/m"}"#),
        )
        .write(
            "m.json",
            Item::File(r#"{"$type": "x/note/v1", "a b=c": "a b target=x", "see": "/n"}"#),
        );
    let ran = board_in(
        scenario.workdir(),
        &["refs", "/n", "--field", "/a b=c", "--field", "/none"],
    );
    let stdout = text(&ran.stdout);
    let from = stdout.lines().next().unwrap();
    assert_eq!(
        from,
        r#"from /see /m ok target=- "/a b=c"="a b target=x" "/none"=absent"#
    );
    let read = (
        "-".to_owned(),
        vec![
            ("/a b=c".to_owned(), Some(serde_json::json!("a b target=x"))),
            ("/none".to_owned(), None),
        ],
    );
    assert_eq!(read_fields(from), read);
}

#[test]
fn refs_help_and_the_over_budget_hint_name_the_budget_and_the_path_limit_the_cli_uses() {
    let budget = format!("{} MiB", render::BUDGET >> 20);
    assert!(render::REFS_HELP.contains(&budget), "{budget}");
    let paths = format!("takes at most {}", board_ops::MAX_LIMIT);
    assert!(render::REFS_HELP.contains(&paths), "{paths}");
    let hint = Code::RefsOverBudget.hint(&Fields::at(&"/a".parse().unwrap()));
    assert!(hint.contains(&format!("up to {budget}")), "{hint}");
    assert!(hint.ends_with("in another `board refs` run"), "{hint}");
}

fn paths(texts: &[&str]) -> Vec<LogicalPath> {
    texts.iter().map(|path| path.parse().unwrap()).collect()
}

/// `paths`, answered from `scenario` and rendered as `board refs` renders
/// them, within `budget` bytes of output.
fn rendered(scenario: &Scenario, texts: &[&str], format: Format, budget: usize) -> Output {
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths = paths(texts);
    let answers = board.refs(&paths, Absent::Refuse, &[], None, None).unwrap();
    render::refs(format, &paths, answers, None, budget)
}

#[test]
fn refs_answers_a_path_while_the_run_has_printed_less_than_its_budget_and_always_the_first() {
    let scenario = hub();
    let asked = ["/projects/p", "/projects/p", "/projects/p"];
    let ran = |budget: usize| {
        let ran = rendered(&scenario, &asked, Format::Text, budget);
        (text(&ran.stdout), ran.error)
    };
    // Each answer is one line of 27 bytes, and no note.
    let line = "/projects/p to /hub /see/0\n";
    assert_eq!(line.len(), 27);
    let over = Some("board::refs::over_budget".to_owned());
    assert_eq!(ran(55), (line.repeat(3), None), "54 bytes are under 55");
    assert_eq!(ran(54), (line.repeat(2), over.clone()), "54 bytes reach 54");
    assert_eq!(
        ran(0),
        (line.to_owned(), over),
        "the first path is always answered"
    );
}

#[test]
fn a_path_after_the_budget_is_spent_is_left_unanswered_with_its_error_in_its_slot() {
    let scenario = hub();
    let asked = ["/projects/p", "/hub", "/projects/p"];
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths = paths(&asked);
    let mut pulled = 0;
    let answers = board.refs(&paths, Absent::Refuse, &[], None, None).unwrap();
    let answers = answers.inspect(|_| pulled += 1);
    let refused = render::refs(Format::Text, &paths, answers, None, 27);
    assert_eq!(pulled, 1, "a path left over budget is never resolved");
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::refs::over_budget"))
    );
    // The index is in memory, as no cache is given, which a note says first.
    assert_eq!(
        text(&refused.stderr),
        "note: the index is in memory for this run: there is no cache directory\n\
         error: /hub: board::refs::over_budget: /hub was not answered: this run already \
         printed its budget of 27 bytes\n\
         hint: /hub: one run prints up to 64 MiB; ask for /hub and the paths \
         after it in another `board refs` run\n\
         error: /projects/p: board::refs::over_budget: /projects/p was not answered: this run \
         already printed its budget of 27 bytes\n\
         hint: /projects/p: one run prints up to 64 MiB; ask for /projects/p and the paths \
         after it in another `board refs` run\n"
    );
    let report = json(&rendered(&scenario, &asked, Format::Json, 1));
    let answers = report["answers"].as_array().unwrap();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0]["to"][0]["path"], "hub.json");
    assert_eq!(answers[1]["$type"], "board/error/v1");
    assert_eq!(answers[1]["code"], "board::refs::over_budget");
    assert_eq!(answers[1]["fields"]["path"], "/hub");
    assert_eq!(answers[2]["code"], "board::refs::over_budget");
    let admitted = rendered(&scenario, &asked, Format::Json, usize::MAX);
    assert_eq!((admitted.code, admitted.error), (0, None));
}

#[test]
fn the_refs_budget_counts_the_bytes_of_the_format_printed() {
    let scenario = hub();
    let asked = ["/projects/p", "/projects/p"];
    // The first answer as text is 27 bytes; as JSON it is many more, so a
    // budget between the two answers both paths as text and one as JSON.
    let one = rendered(&scenario, &asked[..1], Format::Json, usize::MAX);
    let budget = one.stdout.len() - 10;
    assert!(budget > 27);
    let as_text = rendered(&scenario, &asked, Format::Text, budget);
    assert_eq!((as_text.code, as_text.error), (0, None));
    let as_json = rendered(&scenario, &asked, Format::Json, budget);
    assert_eq!(as_json.error.as_deref(), Some("board::refs::over_budget"));
}

#[test]
fn a_check_that_cannot_run_refuses_the_whole_run_and_a_path_refused_alone_does_not() {
    let paths = paths(&["/a", "/b"]);
    let failed = || {
        let source = ResolveError::Git(GitError::NoWorkingTree {
            repository: "/board".into(),
        });
        RefsError::Check {
            source: CheckError::Scope { source },
        }
    };
    let alone = || RefsError::Read {
        source: board_ops::ReadError::NotARecord {
            path: "/b".parse().unwrap(),
        },
    };
    let answers =
        |second: RefsError| -> [Result<RefsReport, RefsError>; 2] { [Err(alone()), Err(second)] };
    let refused = render::refs(
        Format::Json,
        &paths,
        answers(failed()).into_iter(),
        None,
        1 << 20,
    );
    let refused = json(&refused);
    assert_eq!(refused["$type"], "board/error/v1");
    assert_eq!(refused["code"], failed().code().as_str());
    let admitted = render::refs(
        Format::Json,
        &paths,
        answers(alone()).into_iter(),
        None,
        1 << 20,
    );
    let admitted = json(&admitted);
    assert_eq!(admitted["$type"], "board/refs-report/v1");
    assert_eq!(
        admitted["answers"][1]["code"],
        "board::record::not_a_record"
    );
}

/// `board refs /projects/p /hub --field /status` in `format`, where
/// `change`, when set, changes /projects/p, the target of /hub's first
/// reference, after the first answer and so after the check. With the timing
/// record the run would append.
fn changed_between(change: bool, format: Format) -> (Output, serde_json::Value) {
    let scenario = hub();
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths = paths(&["/projects/p", "/hub"]);
    let fields = ["/status".to_owned()];
    let mut answered = 0;
    let answers = board
        .refs(&paths, Absent::Refuse, &fields, Some(1), None)
        .unwrap()
        .inspect(|_| {
            answered += 1;
            if change && answered == 1 {
                scenario.write(
                    "projects/p.json",
                    Item::File(r#"{"$type": "lr/project/v1", "status": "new"}"#),
                );
            }
        });
    let output = render::refs(format, &paths, answers, Some(1), usize::MAX);
    let ran = crate::timing::Ran {
        command: Some("refs".to_owned()),
        trace: board.take_trace(),
        root: None,
    };
    let record = crate::timing::record(std::time::Instant::now(), &ran, &output, 0).unwrap();
    (output, record)
}

#[test]
fn a_target_changed_after_the_check_prints_target_changed_without_values_and_its_twin_its_values() {
    let from = |fields: &str| format!("/hub from /see/0 /links/l ok {fields}\n");
    let (changed, record) = changed_between(true, Format::Text);
    assert_eq!(
        (changed.code, text(&changed.stdout)),
        (
            0,
            format!(
                "/projects/p to /hub /see/0\n{}",
                from("target=board::refs::target_changed")
            )
        )
    );
    assert_eq!(record["work"]["target_changed"], 1);
    let (unchanged, record) = changed_between(false, Format::Text);
    assert_eq!(
        (unchanged.code, text(&unchanged.stdout)),
        (
            0,
            format!(
                "/projects/p to /hub /see/0\n{}",
                from("target=- \"/status\"=absent")
            )
        )
    );
    assert_eq!(record["work"].get("target_changed"), None);
    assert_eq!(
        Code::RefsTargetChanged.hint(&Fields::default()),
        "the target's file changed during this run, so its values were not read; \
         run the same board refs again. If every run refuses it, delete the board's \
         directory under `$XDG_CACHE_HOME/board/` or `~/.cache/board/` so the index \
         is rebuilt from the tree, and if it persists, report it"
    );
}

#[test]
fn a_target_changed_after_the_check_has_its_code_and_no_values_in_json_and_its_twin_has_values() {
    let first = |change| {
        let answers = json(&changed_between(change, Format::Json).0)["answers"].clone();
        answers[1]["from"][0].clone()
    };
    let changed = first(true);
    assert_eq!(changed["target_code"], "board::refs::target_changed");
    assert_eq!(changed.get("values"), None);
    let unchanged = first(false);
    assert_eq!(unchanged.get("target_code"), None);
    assert_eq!(
        unchanged["values"],
        serde_json::json!([{"pointer": "/status", "absent": true}])
    );
}
