//! One built-in plugin call takes at most one View snapshot and one Report
//! snapshot and keeps them while other sessions commit. It drops them when
//! it writes, runs a process in the board's own tree or calls a plugin that
//! may write, when a process or a read-only process plugin it ran moved
//! HEAD, and when another process prunes the index. A read-only capability
//! it calls, and a host it opens on the same board, share them.

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
    /// The call runs a process in a temporary directory outside the board.
    RunOutside,
    /// The call calls `reader.due`, read-only, which queries the note.
    ReadOnlyCall,
    /// The call calls `reader.due`, which queries the note but does not
    /// say it is read-only.
    UnmarkedCall,
    /// The call calls `liar.due`, which says it is read-only and writes
    /// by `Route`.
    LyingCall(Route),
    /// The call opens a second board with `-C` and calls `reader.due`
    /// there.
    Elsewhere,
    /// The call opens its own board again with `-C` and puts the note
    /// through that host.
    SameBoard,
    /// A second board commits the note.
    Commit,
    /// The call runs a process in a temporary directory outside the board,
    /// which commits the note with git.
    RunOutsideCommitting,
    /// The call calls `pliar.due`, a process plugin that says it is
    /// read-only and commits the note with git.
    ProcessLiar,
    /// The call calls `preader.due`, a process plugin that says it is
    /// read-only and writes nothing.
    ProcessReader,
    /// The call opens its own board again with `-C` and queries the note
    /// through that host.
    SameBoardRead,
    /// A second board commits the note, then the call runs a process
    /// outside the board that commits nothing.
    CommitThenRunOutside,
    /// A second handle on the index prunes it.
    Prune,
}

/// How `liar` writes though it says it is read-only.
#[derive(Clone, Copy, Debug)]
enum Route {
    /// It puts the note.
    Put,
    /// It syncs the board, which has no remote.
    Sync,
    /// It updates a mount the board does not hold.
    MountUpdate,
    /// It asks the owner to decide a capability.
    Decide,
}

/// A board with `fixture`, `writer`, `reader` and `liar` enabled,
/// committing as an identity of its own, and its index directory.
fn twin_board() -> (Scenario, tempfile::TempDir) {
    let scenario = board(OTHER_WRITER, ENABLED, Holds::Same);
    scenario.identified();
    scenario.commit(&[("README.md", "a board\n")]);
    (scenario, tempfile::tempdir().unwrap())
}

const OTHER_WRITER: &[(&str, bool)] = &[
    ("fixture", true),
    ("writer", true),
    ("reader", true),
    ("liar", true),
    ("pliar", true),
    ("preader", true),
];

const ENABLED: &[&str] = &["fixture", "writer", "reader", "liar", "pliar", "preader"];

/// A shell script that writes the note in the board at `root` and commits
/// it with git, as another session's `board put` would.
fn committing(root: &Path) -> String {
    format!(
        "PATH=/usr/bin:/bin:$PATH; export PATH; cd '{}' && mkdir -p notes \
         && printf '{{\"$type\": \"fixture/item/v1\"}}' > notes/n.json \
         && git add notes/n.json && git commit -qm note > /dev/null",
        root.display()
    )
}

/// Installs `pliar` in `data`'s plugins directory: a process plugin whose
/// `due` says it is read-only and commits the note in the board at `root`.
fn install_liar(data: &Scenario, root: &Path) {
    install(data, "pliar", true, Reply::Output(r#"{"count": 1}"#));
    let mut declared = manifest("pliar");
    declared["run"] = json!(["sh", "./run"]);
    declared["capabilities"][0]["annotations"] = json!({"readOnlyHint": true});
    data.write(
        "plugins/pliar/plugin.json",
        Item::File(&declared.to_string()),
    );
    let script = format!(
        "#!/bin/sh\ncat > /dev/null\n{} || exit 1\nprintf '%s' '{{\"$type\": \"board/call-result/v1\", \"output\": {{\"count\": 1}}}}'\n",
        committing(root)
    );
    data.write("plugins/pliar/run", Item::File(&script));
    install(data, "preader", true, Reply::Output(r#"{"count": 0}"#));
    let mut reading = manifest("preader");
    reading["run"] = json!(["sh", "./run"]);
    reading["capabilities"][0]["annotations"] = json!({"readOnlyHint": true});
    data.write(
        "plugins/preader/plugin.json",
        Item::File(&reading.to_string()),
    );
}

/// A second board's put of the note, which commits it.
fn commit_elsewhere(root: &Path) {
    let mut other = Board::open(root, None).unwrap();
    other.ask_as(crate::Asker::default());
    let line = note_line("fixture").to_string();
    other.put_batch(&mut line.as_bytes()).unwrap();
}

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
/// `elsewhere` is the second board [`Between::Elsewhere`] opens.
fn querying(between: Between, root: PathBuf, cache: PathBuf, elsewhere: PathBuf) -> Fixture {
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
                Between::RunOutside => {
                    let arguments = ["-c".to_owned(), "printf x > outside".to_owned()];
                    assert_eq!(host.run(&shell(&arguments)).unwrap().code, Some(0));
                }
                Between::ReadOnlyCall | Between::UnmarkedCall => {
                    host.call(&target("reader.due"), &json!({"n": 1})).unwrap();
                }
                Between::LyingCall(_) => {
                    host.call(&target("liar.due"), &json!({"n": 1})).unwrap();
                }
                Between::Elsewhere => {
                    let other = host
                        .board(&Elsewhere::Directory(elsewhere.clone()))
                        .unwrap();
                    other.call(&target("reader.due"), &json!({"n": 1})).unwrap();
                }
                Between::SameBoard => {
                    let same = host.board(&Elsewhere::Directory(root.clone())).unwrap();
                    same.put(&[note_line("fixture")]).unwrap();
                }
                Between::RunOutsideCommitting => {
                    let arguments = ["-c".to_owned(), committing(&root)];
                    let ran = host.run(&shell(&arguments)).unwrap();
                    assert_eq!(
                        ran.code,
                        Some(0),
                        "{}",
                        String::from_utf8_lossy(&ran.stderr)
                    );
                }
                Between::ProcessLiar => {
                    host.call(&target("pliar.due"), &json!({"n": 1})).unwrap();
                }
                Between::ProcessReader => {
                    host.call(&target("preader.due"), &json!({"n": 1})).unwrap();
                }
                Between::SameBoardRead => {
                    let same = host.board(&Elsewhere::Directory(root.clone())).unwrap();
                    assert_eq!(notes(&*same), 0);
                }
                Between::CommitThenRunOutside => {
                    commit_elsewhere(&root);
                    let arguments = ["-c".to_owned(), "printf x > outside".to_owned()];
                    assert_eq!(host.run(&shell(&arguments)).unwrap().code, Some(0));
                }
                Between::Commit => commit_elsewhere(&root),
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

/// `reader`, answering with the notes it finds.
fn reader() -> Fixture {
    answering("reader", Box::new(|host| Ok(json!({"count": notes(host)}))))
}

/// `liar`, which says `due` is read-only and writes by `route`. Only the
/// put succeeds; each other route is refused in this board, after the host
/// dropped the snapshots it shares.
fn liar(route: Route) -> Fixture {
    answering(
        "liar",
        Box::new(move |host| {
            let mount: LogicalPath = "/repos/none".parse().unwrap();
            let deadline = Duration::from_secs(5);
            match route {
                Route::Put => {
                    host.put(&[note_line("liar")]).unwrap();
                }
                Route::Sync => assert!(host.sync(false, deadline).is_err()),
                Route::MountUpdate => assert!(host.mount_update(&mount, deadline).is_err()),
                Route::Decide => {
                    let _ = host.decide("liar.due", &mount, None);
                }
            }
            Ok(json!({"count": 1}))
        }),
    )
    .read_only()
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
    called_with(scenario, cache, fixtures, None)
}

/// [`called`] with the process plugins installed in `directory`.
fn called_with(
    scenario: &Scenario,
    cache: &Path,
    fixtures: Vec<Fixture>,
    directory: Option<&Path>,
) -> (Value, usize) {
    let mut board = open(scenario);
    board.build_in(built(fixtures));
    let plugins = Plugins {
        cache: Some(cache),
        directory,
        environment: directory.map_or_else(Environment::default, |_| environment("0")),
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
    around_in(between, true)
}

/// [`around`] in a board whose `commit_on_put` is `commits`.
fn around_in(between: Between, commits: bool) -> (Value, usize) {
    let (scenario, cache) = twin_board();
    if !commits {
        let config =
            json!({"$type": "board/config/v1", "plugins": ENABLED, "commit_on_put": false});
        scenario.write(".board/board.json", Item::File(&config.to_string()));
    }
    let (other, _) = twin_board();
    let root = scenario.workdir().to_path_buf();
    let data = Scenario::new();
    install_liar(&data, &root);
    let directory = data.workdir().join("plugins");
    let reading = match between {
        Between::UnmarkedCall => reader(),
        _ => reader().read_only(),
    };
    let fixtures = vec![
        querying(
            between,
            root,
            cache.path().to_path_buf(),
            other.workdir().to_path_buf(),
        ),
        writer(),
        reading,
        liar(match between {
            Between::LyingCall(route) => route,
            _ => Route::Put,
        }),
    ];
    called_with(&scenario, cache.path(), fixtures, Some(&directory))
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
fn a_put_a_process_a_nested_call_or_a_lying_callee_between_queries_shows_the_note() {
    for between in [
        Between::Put,
        Between::Run,
        Between::Call,
        Between::LyingCall(Route::Put),
        Between::SameBoard,
        Between::RunOutsideCommitting,
        Between::ProcessLiar,
    ] {
        let (output, taken) = around(between);
        assert_eq!(output["first"], 0, "{between:?}");
        assert_eq!(output["second"], 1, "{between:?}");
        assert_eq!(taken, 2, "{between:?}");
    }
}

#[test]
fn another_boards_commit_between_queries_leaves_one_snapshot_and_the_first_answer() {
    let (output, taken) = around(Between::Commit);
    assert_eq!(
        (output["first"].clone(), output["second"].clone()),
        (json!(0), json!(0))
    );
    assert_eq!(taken, 1);
}

#[test]
fn twenty_reads_after_twenty_foreign_commits_take_one_view_and_one_report_snapshot() {
    let (scenario, cache) = twin_board();
    let root = scenario.workdir().to_path_buf();
    let reading = answering(
        "fixture",
        Box::new(move |host| {
            let everything: LogicalPath = "/".parse().unwrap();
            let sql = "SELECT logical FROM entries WHERE logical LIKE '/notes/%'";
            let read = || {
                let answers = host
                    .refs(std::slice::from_ref(&everything), Absent::Refuse, &[])
                    .unwrap();
                assert!(answers.into_iter().all(|answer| answer.is_ok()));
                host.query(sql, Limit::ALL).unwrap().rows.len()
            };
            let mut seen = vec![read()];
            for number in 0..20 {
                let mut other = Board::open(&root, None).unwrap();
                other.ask_as(crate::Asker::default());
                let line = json!({
                    "path": format!("/notes/c{number}"),
                    "record": {"$type": item("fixture").as_str()},
                });
                let put = other.put_batch(&mut line.to_string().as_bytes()).unwrap();
                assert!(put.commit.is_some(), "the foreign put commits");
                seen.push(read());
            }
            Ok(json!({"count": seen.iter().sum::<usize>()}))
        }),
    );
    let (output, taken) = called(&scenario, cache.path(), vec![reading]);
    assert_eq!(output["count"], 0, "every read shows the board as it began");
    assert_eq!(taken, 2);
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

#[test]
fn a_read_only_nested_call_shares_the_callers_snapshot_and_its_unmarked_twin_takes_its_own() {
    let (output, taken) = around(Between::ReadOnlyCall);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 1);
    let (output, taken) = around(Between::UnmarkedCall);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 3);
}

#[test]
fn a_process_outside_the_board_keeps_the_snapshot_and_its_twin_in_a_board_that_does_not_commit_on_put_drops_it()
 {
    let (output, taken) = around(Between::RunOutside);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 1);
    let (output, taken) = around_in(Between::RunOutside, false);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 2);
}

#[test]
fn a_call_in_another_board_takes_its_own_snapshots_and_leaves_the_callers_count_alone() {
    let (output, taken) = around(Between::Elsewhere);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 1);
}

#[test]
fn a_read_only_callee_that_syncs_updates_a_mount_or_decides_drops_the_snapshots_it_shares() {
    for route in [Route::Sync, Route::MountUpdate, Route::Decide] {
        let (output, taken) = around(Between::LyingCall(route));
        assert_eq!(
            (&output["first"], &output["second"]),
            (&json!(0), &json!(0)),
            "{route:?}"
        );
        assert_eq!(taken, 2, "{route:?}");
    }
}

#[test]
fn a_read_only_process_plugin_or_a_same_board_host_that_only_reads_keeps_the_one_snapshot() {
    for between in [Between::ProcessReader, Between::SameBoardRead] {
        let (output, taken) = around(between);
        assert_eq!(
            (&output["first"], &output["second"]),
            (&json!(0), &json!(0)),
            "{between:?}"
        );
        assert_eq!(taken, 1, "{between:?}");
    }
}

#[test]
fn a_foreign_commit_before_a_process_outside_that_commits_nothing_keeps_the_snapshot() {
    let (output, taken) = around(Between::CommitThenRunOutside);
    assert_eq!(
        (&output["first"], &output["second"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(taken, 1);
}

#[test]
fn refs_after_a_foreign_commit_and_the_query_after_it_read_the_same_board() {
    let (scenario, cache) = twin_board();
    let root = scenario.workdir().to_path_buf();
    let reading = answering(
        "fixture",
        Box::new(move |host| {
            let first = notes(host);
            commit_elsewhere(&root);
            let note: LogicalPath = NOTE.parse().unwrap();
            let resolved = host
                .refs(std::slice::from_ref(&note), Absent::Refuse, &[])
                .unwrap()
                .into_iter()
                .all(|answer| answer.is_ok());
            Ok(json!({"count": 0, "first": first, "resolved": resolved, "second": notes(host)}))
        }),
    );
    let (output, taken) = called(&scenario, cache.path(), vec![reading]);
    assert_eq!(output["first"], 0);
    assert_eq!(output["resolved"], true, "refs reads the newer board");
    assert_eq!(
        output["second"], 1,
        "the Report's refresh rewrites the index slot the View reads, so the \
         View is retaken at the newer HEAD, with or without the drop"
    );
    assert_eq!(taken, 3, "a View taken before the commit is not used again");
}

/// A type whose `see` field is a typed reference.
const LINK_SCHEMA: &str =
    r#"{"type": "object", "properties": {"see": {"type": "string", "format": "board-ref"}}}"#;

#[test]
fn a_query_after_a_foreign_commit_and_the_refs_after_it_read_the_same_board() {
    let (scenario, cache) = twin_board();
    scenario
        .write(
            ".board/types/lr/link/v1.schema.json",
            Item::File(LINK_SCHEMA),
        )
        .write(
            "notes/target.json",
            Item::File(r#"{"$type": "fixture/item/v1"}"#),
        );
    let root = scenario.workdir().to_path_buf();
    let reading = answering(
        "fixture",
        Box::new(move |host| {
            let answer = |path: &str| {
                let path: LogicalPath = path.parse().unwrap();
                (host.refs(std::slice::from_ref(&path), Absent::Refuse, &[]))
                    .unwrap()
                    .into_iter()
                    .next()
                    .unwrap()
                    .unwrap()
            };
            // The root resolves, so this takes the Report snapshot.
            answer("/");
            let mut other = Board::open(&root, None).unwrap();
            other.ask_as(crate::Asker::default());
            let line =
                json!({"path": NOTE, "record": {"$type": "lr/link/v1", "see": "/notes/target"}});
            other.put_batch(&mut line.to_string().as_bytes()).unwrap();
            let queried = notes(host);
            let from: Vec<String> = (answer(NOTE).from.iter())
                .map(|outgoing| outgoing.target.to_string())
                .collect();
            let to: Vec<String> = (answer("/notes/target").to.iter())
                .filter_map(|incoming| incoming.logical.as_ref().map(ToString::to_string))
                .collect();
            Ok(json!({"count": 0, "queried": queried, "from": from, "to": to}))
        }),
    );
    let (output, taken) = called(&scenario, cache.path(), vec![reading]);
    assert_eq!(output["queried"], 1, "the query reads the newer board");
    assert_eq!(
        (&output["from"], &output["to"]),
        (&json!(["/notes/target"]), &json!([NOTE])),
        "the refs after the query read the board the query read"
    );
    assert_eq!(
        taken, 3,
        "the query drops the Report taken before the commit"
    );
}
