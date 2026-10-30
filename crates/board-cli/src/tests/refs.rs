//! `board refs --field`, and the budget of a `refs` run.

use board_model::{Code, Fields, LogicalPath};
use board_ops::{Absent, CheckError, RefsError, RefsReport};
use board_scenario::{Item, Scenario};
use board_tree::{GitError, ResolveError};

use super::{answers, board, board_in, json, text};
use crate::render::{self, Printer};
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
const GOLDEN_FIELDS: &str = r#"{"schema":"board/refs-report/v2","data":[{"from":[{"link":false,"location":{"display":"/projects/p","path":"projects/p.json","pointers":[{"path":"links/l.json","repository":"<root>","source":"worktree"}],"repository":"<root>","source":"worktree"},"pointer":"/see/0","status":"ok","target":"/links/l","values":[{"absent":true,"pointer":"/status"},{"pointer":"/$type","value":"lr/project/v1"}]},{"link":false,"location":{"display":"/notes.md","path":"notes.md","pointers":[],"repository":"<root>","source":"worktree"},"pointer":"/see/1","status":"ok","target":"/notes.md","target_code":"board::record::not_a_record"},{"code":"board::ref::dangling","link":false,"location":null,"pointer":"/see/2","status":"degraded","target":"/zz"},{"link":false,"location":{"display":"/tasks/bad","path":"tasks/bad.json","pointers":[],"repository":"<root>","source":"worktree"},"pointer":"/see/3","status":"ok","target":"/tasks/bad","target_code":"board::record::invalid","values":[{"pointer":"/status","value":"done"},{"pointer":"/$type","value":"lr/task/v1"}]}],"location":{"display":"/hub","path":"hub.json","pointers":[],"repository":"<root>","source":"worktree"},"missing":null,"to":[],"total":4,"truncated":false}],"failures":[]}
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
            (2, Some("board::cli::usage")),
            "{refused}"
        );
        assert_eq!(
            text(&ran.stderr),
            format!(
                "error: invalid value '{refused}' for '--field <POINTER>': \
                 must be a JSON pointer, such as /status, or /a~1b for the key a/b: empty or \
                 starting with /, with ~ only as ~0 or ~1\n\
                 \x20 help:  run `board refs -h`\n\
                 \x20 code:  board::cli::usage\n"
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
fn refs_help_names_the_budget_and_the_path_limit_the_cli_uses() {
    let budget = format!("{} MiB", render::BUDGET >> 20);
    assert!(render::REFS_HELP.contains(&budget), "{budget}");
    let paths = format!("At most {} paths", board_ops::MAX_LIMIT);
    assert!(render::REFS_HELP.contains(&paths), "{paths}");
}

fn paths(texts: &[&str]) -> Vec<LogicalPath> {
    texts.iter().map(|path| path.parse().unwrap()).collect()
}

/// `paths`, answered from `scenario` and rendered as `board refs` renders
/// them, within `budget` bytes of output.
fn rendered(scenario: &Scenario, texts: &[&str], format: bool, budget: usize) -> Output {
    let board = board_ops::Board::open(scenario.workdir(), None).unwrap();
    let paths = paths(texts);
    let answers = board.refs(&paths, Absent::Refuse, &[], None, None).unwrap();
    render::refs(
        Printer::of(format, budget, &["refs"]),
        &paths,
        answers,
        None,
    )
}

#[test]
fn refs_answers_a_path_while_the_run_has_printed_less_than_its_budget_and_always_the_first() {
    let scenario = hub();
    let asked = ["/projects/p", "/projects/p", "/projects/p"];
    let ran = |budget: usize| {
        let ran = rendered(&scenario, &asked, false, budget);
        (text(&ran.stdout), ran.error)
    };
    // The first answer is a header and one line, 35 bytes, and each after
    // it 36, with the blank line before its header.
    let first = "==> /projects/p <==\nto /hub /see/0\n";
    assert_eq!(first.len(), 35);
    let next = format!("\n{first}");
    let over = Some("board::output::over_budget".to_owned());
    let three = format!("{first}{next}{next}");
    assert_eq!(ran(107), (three, None), "107 bytes fit in 107");
    let two = format!("{first}{next}");
    assert_eq!(ran(106), (two, over.clone()), "the third passes 106 by one");
    assert_eq!(
        ran(1),
        (first.to_owned(), over),
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
    let refused = render::refs(Printer::of(false, 27, &["refs"]), &paths, answers, None);
    assert_eq!(pulled, 1, "a path left over budget is never resolved");
    assert_eq!(
        (refused.code, refused.error.as_deref()),
        (1, Some("board::output::over_budget"))
    );
    // The index is in memory, as no cache is given, which crumbkit prints as
    // the batch's note, before the failure.
    assert_eq!(
        text(&refused.stderr),
        "note: the index is in memory for this run: there is no cache directory\n\
         error: 2 more answers did not fit in the output budget of 27 bytes\n\
         \x20 help:  run `board refs /hub /projects/p`\n\
         \x20 code:  board::output::over_budget\n"
    );
    // Room for the first answer and the failure that leaves the other two
    // out, but not for /hub's references.
    let budget = rendered(&scenario, &asked[..1], true, usize::MAX)
        .stdout
        .len()
        + 300;
    let report = json(&rendered(&scenario, &asked, true, budget));
    assert_eq!(report["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["data"][0]["to"][0]["path"], "hub.json");
    let over = &report["failures"][0];
    assert_eq!(over["code"], "board::output::over_budget");
    assert_eq!(over["fields"]["budget"], budget);
    assert_eq!(
        over["fields"]["left_out"],
        serde_json::json!(["/hub", "/projects/p"])
    );
    assert_eq!(over["fields"]["left_out_from"], 1);
    let admitted = rendered(&scenario, &asked, true, usize::MAX);
    assert_eq!((admitted.code, admitted.error), (0, None));
}

#[test]
fn the_refs_budget_counts_the_bytes_of_the_format_printed() {
    let scenario = hub();
    let asked = ["/projects/p", "/projects/p"];
    // Both answers as text are shorter than the first as JSON, which names
    // the board's directory, so a budget of the text's length answers both
    // paths as text and one as JSON. Each is measured, because the
    // directory's length differs by platform.
    let text_both = rendered(&scenario, &asked, false, usize::MAX);
    let json_one = rendered(&scenario, &asked[..1], true, usize::MAX);
    let budget = text_both.stdout.len();
    assert!(budget < json_one.stdout.len());
    let as_text = rendered(&scenario, &asked, false, budget);
    assert_eq!((as_text.code, as_text.error), (0, None));
    let as_json = rendered(&scenario, &asked, true, budget);
    assert_eq!(as_json.error.as_deref(), Some("board::output::over_budget"));
}

#[test]
fn refs_over_budget_leaves_out_the_later_paths_in_one_failure_with_a_run_recovery_in_text_and_json()
{
    let scenario = hub();
    let asked = ["/projects/p", "/projects/p", "/projects/p"];
    let over = "  code:  board::output::over_budget\n";
    // The first answer is 35 bytes as text and several hundred as JSON, so
    // 1 is under it in both formats and 50 only in JSON; refs prints its
    // first answer either way.
    for budget in [1, 50] {
        let as_text = rendered(&scenario, &asked, false, budget);
        let stderr = text(&as_text.stderr);
        assert_eq!(as_text.error.as_deref(), Some("board::output::over_budget"));
        assert_eq!(stderr.matches(over).count(), 1, "{stderr}");
        assert!(
            stderr.contains("  help:  run `board refs /projects/p /projects/p`\n"),
            "{stderr}"
        );
        let report = json(&rendered(&scenario, &asked, true, budget));
        assert_eq!(report["data"].as_array().map(Vec::len), Some(1));
        let failures = report["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1, "{report}");
        assert_eq!(failures[0]["code"], "board::output::over_budget");
        assert_eq!(failures[0]["fields"]["left_out_from"], 1);
        assert_eq!(failures[0]["recovery"]["kind"], "run");
        assert_eq!(failures[0]["recovery"]["all"], "left_out");
    }
    for format in [false, true] {
        let admitted = rendered(&scenario, &asked, format, usize::MAX);
        assert_eq!((admitted.code, admitted.error), (0, None));
    }
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
        Printer::of(true, 1 << 20, &[]),
        &paths,
        answers(failed()).into_iter(),
        None,
    );
    let refused = json(&refused);
    assert_eq!(refused["schema"], "board/failure/v1");
    assert_eq!(refused["code"], failed().code().as_str());
    let admitted = render::refs(
        Printer::of(true, 1 << 20, &[]),
        &paths,
        answers(alone()).into_iter(),
        None,
    );
    assert_eq!(json(&admitted)["schema"], "board/refs-report/v2");
    assert_eq!(
        super::answers(&admitted)[1]["code"],
        "board::record::not_a_record"
    );
}

/// `board refs /projects/p /hub --field /status` in `format`, where
/// `change`, when set, changes /projects/p, the target of /hub's first
/// reference, after the first answer and so after the check. With the timing
/// record the run would append.
fn changed_between(change: bool, format: bool) -> (Output, serde_json::Value) {
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
    let output = render::refs(
        Printer::of(format, usize::MAX, &["refs"]),
        &paths,
        answers,
        Some(1),
    );
    let ran = crate::timing::Ran {
        command: Some("refs".to_owned()),
        trace: board.take_trace(),
        root: None,
        prefix: None,
    };
    let record = crate::timing::record(std::time::Instant::now(), &ran, &output, 0).unwrap();
    (output, record)
}

#[test]
fn a_target_changed_after_the_check_prints_target_changed_without_values_and_its_twin_its_values() {
    let from = |fields: &str| format!("\n==> /hub <==\nfrom /see/0 /links/l ok {fields}\n");
    let (changed, record) = changed_between(true, false);
    assert_eq!(
        (changed.code, text(&changed.stdout)),
        (
            0,
            format!(
                "==> /projects/p <==\nto /hub /see/0\n{}",
                from("target=board::refs::target_changed")
            )
        )
    );
    assert_eq!(record["work"]["target_changed"], 1);
    let (unchanged, record) = changed_between(false, false);
    assert_eq!(
        (unchanged.code, text(&unchanged.stdout)),
        (
            0,
            format!(
                "==> /projects/p <==\nto /hub /see/0\n{}",
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
        let answers = answers(&changed_between(change, true).0);
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
