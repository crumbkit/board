//! One built-in plugin call takes at most one View snapshot and one Report
//! snapshot, and drops them when HEAD moves, when it writes, runs a process
//! or calls another plugin, and when another process prunes the index.

use std::path::PathBuf;

use board_index::Index;

use super::run::shell;
use super::*;

/// The logical path the cases write, and query for.
const NOTE: &str = "/notes/n";

/// What happens between a call's two queries.
#[derive(Clone, Copy, Debug)]
enum Between {
    Nothing,
    /// The call puts the note.
    Put,
    /// The call runs a process that writes the note's file.
    Run,
    /// The call calls `writer.due`, which puts the note.
    Call,
    /// A second board commits the note.
    Commit,
    /// A second handle on the index prunes it.
    Prune,
}

/// A board with `fixture` and `writer` enabled, committing as an identity
/// of its own, and its index directory.
fn twin_board() -> (Scenario, tempfile::TempDir) {
    let scenario = board(OTHER_WRITER, &["fixture", "writer"], Holds::Same);
    scenario.identified();
    scenario.commit(&[("README.md", "a board\n")]);
    (scenario, tempfile::tempdir().unwrap())
}

const OTHER_WRITER: &[(&str, bool)] = &[("fixture", true), ("writer", true)];

/// The put line that writes the note as `plugin`'s item.
fn note_line(plugin: &str) -> Value {
    json!({"path": NOTE, "record": {"$type": item(plugin).as_str()}})
}

/// How many entries the board holds at the note's path.
fn notes(host: &dyn Host) -> usize {
    let sql = format!(
        "SELECT logical FROM entries WHERE logical = {}",
        literal(NOTE)
    );
    host.query(&sql, Limit::ALL).unwrap().rows.len()
}

/// `fixture`, answering with the notes it finds before and after `between`.
fn querying(between: Between, root: PathBuf, cache: PathBuf) -> Fixture {
    answering(
        "fixture",
        Box::new(move |host| {
            let first = notes(host);
            match between {
                Between::Nothing => {}
                Between::Put => {
                    host.put(&[note_line("fixture")]).unwrap();
                }
                Between::Run => {
                    let arguments = [
                        "-c".to_owned(),
                        "mkdir -p notes && printf '{\"$type\": \"fixture/item/v1\"}' > notes/n.json"
                            .to_owned(),
                    ];
                    let run = Run {
                        directory: Some(&root),
                        ..shell(&arguments)
                    };
                    assert_eq!(host.run(&run).unwrap().code, Some(0));
                }
                Between::Call => {
                    host.call(&target("writer.due"), &json!({"n": 1})).unwrap();
                }
                Between::Commit => {
                    let mut other = Board::open(&root, None).unwrap();
                    other.ask_as(crate::Asker::default());
                    let line = note_line("fixture").to_string();
                    other.put_batch(&mut line.as_bytes()).unwrap();
                }
                Between::Prune => {
                    let other = Board::open(&root, None).unwrap();
                    let index = Index::open(Some(&cache), other.repository().path()).unwrap();
                    let writer = index.write().unwrap();
                    writer.prune().unwrap();
                    writer.decide(Vec::new()).unwrap();
                }
            }
            Ok(json!({"count": 0, "first": first, "second": notes(host)}))
        }),
    )
}

/// `writer`, putting the note as its own item.
fn writer() -> Fixture {
    answering(
        "writer",
        Box::new(|host| {
            host.put(&[note_line("writer")]).unwrap();
            Ok(json!({"count": 1}))
        }),
    )
}

/// The `snapshot` phase's count in `board`'s trace.
fn snapshots(board: &Board) -> usize {
    (board.take_trace().phases.iter())
        .find(|phase| phase.name == "snapshot")
        .map_or(0, |phase| phase.entered)
}

/// Calls `fixture.due` with `fixtures` built in, through the index under
/// `cache`, and returns its output and how many snapshots it took.
fn called(scenario: &Scenario, cache: &Path, fixtures: Vec<Fixture>) -> (Value, usize) {
    let mut board = open(scenario);
    board.build_in(built(fixtures));
    let plugins = Plugins {
        cache: Some(cache),
        ..Plugins::default()
    };
    let output = call_base(&board, &plugins)
        .unwrap_or_else(|error| panic!("{}", error.refusal().message))
        .output;
    (output, snapshots(&board))
}

/// What a call answers, and how many snapshots it took, with `between`
/// between its two queries.
fn around(between: Between) -> (Value, usize) {
    let (scenario, cache) = twin_board();
    let root = scenario.workdir().to_path_buf();
    let fixtures = vec![
        querying(between, root, cache.path().to_path_buf()),
        writer(),
    ];
    called(&scenario, cache.path(), fixtures)
}

#[test]
fn three_queries_in_one_call_take_one_snapshot_and_three_calls_take_three() {
    let (scenario, cache) = twin_board();
    let thrice = answering(
        "fixture",
        Box::new(|host| {
            let found: Vec<usize> = (0..3).map(|_| notes(host)).collect();
            Ok(json!({"count": found.iter().sum::<usize>()}))
        }),
    );
    assert_eq!(called(&scenario, cache.path(), vec![thrice]).1, 1);
    let once = || {
        answering(
            "fixture",
            Box::new(|host| Ok(json!({"count": notes(host)}))),
        )
    };
    let mut board = open(&scenario);
    board.build_in(built(vec![once()]));
    let plugins = Plugins {
        cache: Some(cache.path()),
        ..Plugins::default()
    };
    for _ in 0..3 {
        call_base(&board, &plugins).unwrap();
    }
    assert_eq!(snapshots(&board), 3);
}

#[test]
fn refs_then_queries_take_one_report_and_one_view_snapshot() {
    let (scenario, cache) = twin_board();
    let reading = answering(
        "fixture",
        Box::new(|host| {
            let root: LogicalPath = "/".parse().unwrap();
            for _ in 0..2 {
                let answers = host
                    .refs(std::slice::from_ref(&root), Absent::Refuse, &[])
                    .unwrap();
                assert!(answers.into_iter().all(|answer| answer.is_ok()));
                notes(host);
            }
            Ok(json!({"count": 0}))
        }),
    );
    assert_eq!(called(&scenario, cache.path(), vec![reading]).1, 2);
}

#[test]
fn a_second_query_with_nothing_between_reuses_the_first_snapshot() {
    let (output, taken) = around(Between::Nothing);
    assert_eq!(
        (output["first"].clone(), output["second"].clone()),
        (json!(0), json!(0))
    );
    assert_eq!(taken, 1);
}

#[test]
fn a_put_a_process_a_nested_call_or_a_commit_between_queries_shows_the_note() {
    for between in [Between::Put, Between::Run, Between::Call, Between::Commit] {
        let (output, taken) = around(between);
        assert_eq!(output["first"], 0, "{between:?}");
        assert_eq!(output["second"], 1, "{between:?}");
        assert_eq!(taken, 2, "{between:?}");
    }
}

#[test]
fn a_prune_between_queries_makes_the_second_take_a_fresh_snapshot() {
    let (output, taken) = around(Between::Prune);
    assert_eq!(
        (output["first"].clone(), output["second"].clone()),
        (json!(0), json!(0))
    );
    assert_eq!(taken, 2);
}
