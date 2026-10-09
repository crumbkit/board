//! Building the index from real repositories, counting the work done.

use board_index::{BlobFact, Index, Storage};
use board_model::{Code, Kind, Role, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;
use serde_json::{Value, json};

use crate::index::{Kept, Refreshed, facts, transact};
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, open, task};

/// Refreshes the index of `scenario` in `cache`.
fn build(scenario: &Scenario, cache: &std::path::Path) -> Refreshed {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    board.snapshot(index, None).unwrap().report.work
}

/// The board, plus two tasks with the same bytes and one invalid task.
fn tasks() -> Scenario {
    let scenario = board();
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write("tasks/b.json", Item::File(&task("")))
        .write("tasks/bad.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
        .write("notes.md", Item::File("not read"));
    scenario
}

#[test]
fn a_cold_refresh_hashes_every_file_and_parses_each_json_blob_and_compiles_each_schema_once() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    // Eight files are hashed; board.json, two schemas, the project, and two
    // distinct task blobs are parsed; notes.md is never read. Each schema
    // validates the records of its type with the compilation that judged it.
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 8,
            parsed: 6,
            compiled: 2,
            validated: 3,
            wrote: true,
            ..Refreshed::default()
        }
    );
}

#[test]
fn a_schema_that_does_not_compile_is_compiled_once_and_validates_nothing() {
    let scenario = tasks();
    scenario.write(
        ".board/types/lr/task/v1.schema.json",
        Item::File(r#"{"type": 5}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    let work = build(&scenario, cache.path());
    // Only the project is validated; no task asks for its schema again.
    assert_eq!((work.compiled, work.validated), (2, 1));
}

/// How many files a snapshot of `scenario` in `cache` opens.
fn opened(scenario: &Scenario, cache: &std::path::Path) -> usize {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let before = board.repository().files_opened();
    board.snapshot(index, None).unwrap();
    board.repository().files_opened() - before
}

#[test]
fn a_cold_refresh_opens_each_file_once_and_a_warm_one_opens_none() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    // The eight files hashed are each opened once: the JSON ones are parsed
    // from the bytes hashed.
    assert_eq!(opened(&scenario, cache.path()), 8, "cold");
    assert_eq!(opened(&scenario, cache.path()), 0, "warm");
}

#[test]
fn a_file_of_the_largest_size_kept_is_opened_once() {
    let scenario = tasks();
    let big = "a".repeat(board_model::MAX_RECORD_BYTES);
    scenario.write("big.json", Item::File(&big));
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(opened(&scenario, cache.path()), 9);
}

#[test]
fn a_file_too_large_to_keep_is_hashed_and_then_opened_again_to_be_classified() {
    let scenario = tasks();
    let big = "a".repeat(board_model::MAX_RECORD_BYTES + 1);
    scenario.write("big.json", Item::File(&big));
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(opened(&scenario, cache.path()), 10);
}

#[test]
fn a_second_refresh_does_no_work() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

#[test]
fn one_changed_record_hashes_one_file_parses_one_blob_and_compiles_and_validates_once() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("tasks/a.json", Item::File(&task(r#", "see": "/x""#)));
    // The index holds the schema's fact, so it is compiled to validate the record.
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            parsed: 1,
            compiled: 1,
            validated: 1,
            wrote: true,
            ..Refreshed::default()
        }
    );
}

#[test]
fn a_changed_schema_revalidates_exactly_the_records_of_its_type() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write(
        ".board/types/lr/task/v1.schema.json",
        Item::File(r#"{"type": "object", "required": []}"#),
    );
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            parsed: 1,
            compiled: 1,
            validated: 2,
            wrote: true,
            ..Refreshed::default()
        }
    );
}

/// The rows `sql` reads from the index of `scenario` in `cache`, once a
/// snapshot has refreshed it, each as a JSON array.
fn read(scenario: &Scenario, cache: &std::path::Path, sql: &str) -> Vec<Value> {
    let board = open(scenario);
    let index = Index::open(Some(cache), board.repository().path()).unwrap();
    let snapshot = board.snapshot(index, None).unwrap();
    let rows = snapshot.view.query(sql, 10_000).unwrap().rows;
    rows.into_iter().map(Value::from).collect()
}

#[test]
fn the_index_holds_each_blobs_kind_type_and_code() {
    let scenario = tasks();
    let broken = r#"{"type": 7}"#;
    scenario
        .write(".board/types/lr/broken/v1.schema.json", Item::File(broken))
        .write("bad.json", Item::File("{"));
    let cache = tempfile::tempdir().unwrap();
    let fact = |text: &str, role: &str| {
        let blob = scenario.blob(text);
        let sql = format!(
            "SELECT kind, type, code FROM main.blobs WHERE blob = '{blob}' AND role = '{role}'"
        );
        read(&scenario, cache.path(), &sql)
    };
    assert_eq!(
        fact(&task(""), "json"),
        [json!(["record", "lr/task/v1", null])]
    );
    assert_eq!(
        fact(&task(""), "schema"),
        [] as [Value; 0],
        "only roles seen"
    );
    assert_eq!(
        fact("{", "json"),
        [json!(["json", null, "board::json::parse"])]
    );
    assert_eq!(fact(TASK_SCHEMA, "schema"), [json!(["schema", null, null])]);
    assert_eq!(
        fact(broken, "schema"),
        [json!(["schema", null, "board::schema::invalid"])],
        "a schema's fact says whether it compiles"
    );
    assert_eq!(fact("not read", "json"), [] as [Value; 0]);
}

#[test]
fn the_index_holds_each_validation_with_its_references_in_the_order_found() {
    let scenario = board();
    let good = task(r#", "see": "/x", "project": "/projects/p""#);
    let bad = r#"{"$type": "lr/task/v1"}"#;
    scenario
        .write("tasks/good.json", Item::File(&good))
        .write("tasks/bad.json", Item::File(bad));
    let cache = tempfile::tempdir().unwrap();
    let validation = |record: &str, schema: &str| {
        let (record, schema) = (scenario.blob(record), scenario.blob(schema));
        let sql = format!(
            "SELECT code, pointer, reason, failures FROM main.validations \
             WHERE record = '{record}' AND schema = '{schema}'"
        );
        read(&scenario, cache.path(), &sql)
    };
    assert_eq!(
        validation(&good, TASK_SCHEMA),
        [json!([null, null, null, null])]
    );
    let references = format!(
        "SELECT ordinal, pointer, target, accepts FROM main.refs \
         WHERE record = '{}' AND schema = '{}' ORDER BY ordinal",
        scenario.blob(&good),
        scenario.blob(TASK_SCHEMA)
    );
    assert_eq!(
        read(&scenario, cache.path(), &references),
        [
            json!([0, "/project", "/projects/p", r#"["lr/project/v1"]"#]),
            json!([1, "/see", "/x", null]),
        ]
    );
    assert_eq!(
        validation(bad, TASK_SCHEMA),
        [json!([
            "board::record::invalid",
            "",
            "fails its schema at its root: \"title\" is a required property",
            1
        ])]
    );
    assert_eq!(validation(&good, PROJECT_SCHEMA), [] as [Value; 0]);
}

#[test]
fn a_file_modified_after_the_last_refresh_began_is_hashed_again() {
    let scenario = tasks();
    scenario.write("fresh.md", Item::Ahead("just saved"));
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            wrote: true,
            ..Refreshed::default()
        },
        "its metadata matches, but it could have changed unseen"
    );
}

#[test]
fn a_file_modified_before_the_last_refresh_began_is_trusted() {
    let scenario = tasks();
    scenario.write("fresh.md", Item::File("saved an hour ago"));
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

#[test]
fn a_deleted_file_leaves_the_index_and_a_new_one_is_hashed() {
    let scenario = tasks();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("notes.md", Item::Deleted);
    scenario.write("more.md", Item::File("new"));
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            hashed: 1,
            wrote: true,
            ..Refreshed::default()
        }
    );
    let paths = read(&scenario, cache.path(), "SELECT path FROM main.worktree");
    assert!(paths.contains(&json!(["more.md"])));
    assert!(!paths.contains(&json!(["notes.md"])));
}

#[test]
fn the_index_keeps_each_entrys_address_code_blob_and_metadata() {
    let scenario = tasks();
    scenario.write("bad\tname.md", Item::File(""));
    let cache = tempfile::tempdir().unwrap();
    let sql = "SELECT path, logical, code, blob, size FROM main.worktree \
               WHERE path IN ('tasks/a.json', 'bad\tname.md') ORDER BY path";
    assert_eq!(
        read(&scenario, cache.path(), sql),
        [
            json!([
                "bad\tname.md",
                null,
                "board::path::malformed",
                scenario.blob("").to_string(),
                0
            ]),
            json!([
                "tasks/a.json",
                "/tasks/a",
                null,
                scenario.blob(&task("")).to_string(),
                task("").len()
            ]),
        ]
    );
}

#[test]
fn head_s_trees_are_listed_once_and_after_a_commit_only_the_changed_ones() {
    let scenario = Scenario::new();
    scenario.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "1"),
        ("b/y.md", "1"),
    ]);
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        build(&scenario, cache.path()).trees,
        4,
        "root, .board, a, b"
    );
    assert_eq!(build(&scenario, cache.path()).trees, 0);
    scenario.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "2"),
        ("b/y.md", "1"),
    ]);
    assert_eq!(
        build(&scenario, cache.path()).trees,
        2,
        "the new root and a"
    );
}

/// A board with three committed files, all in the working tree as
/// committed.
fn committed() -> Scenario {
    let scenario = Scenario::new();
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "1"),
        ("b/y.md", "1"),
    ];
    scenario.commit(&files);
    for (path, text) in files {
        scenario.write(path, Item::File(text));
    }
    scenario
}

#[test]
fn a_refresh_with_nothing_changed_on_a_committed_board_writes_nothing() {
    let scenario = committed();
    let cache = tempfile::tempdir().unwrap();
    assert!(build(&scenario, cache.path()).wrote);
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

#[test]
fn a_changed_file_or_a_new_commit_writes_and_the_refresh_after_does_not() {
    let scenario = committed();
    let cache = tempfile::tempdir().unwrap();
    build(&scenario, cache.path());
    scenario.write("a/x.md", Item::File("2"));
    let edited = build(&scenario, cache.path());
    assert_eq!((edited.hashed, edited.wrote), (1, true));
    scenario.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        ("a/x.md", "2"),
        ("b/y.md", "1"),
    ]);
    assert_eq!(
        build(&scenario, cache.path()),
        Refreshed {
            trees: 2,
            wrote: true,
            ..Refreshed::default()
        },
        "the working tree is as it was, but HEAD is not"
    );
    assert_eq!(build(&scenario, cache.path()), Refreshed::default());
}

/// Runs a transaction on a filled index whose closure reads, lets another
/// connection commit a write when `interfere` and this is its first run,
/// then writes. Returns how many times the closure ran, whether the
/// transaction that committed wrote, and whether it wrote to the file.
fn retried(interfere: bool) -> (usize, bool, bool) {
    let cache = tempfile::tempdir().unwrap();
    let root = std::path::Path::new("/boards/retry");
    let blob = |byte| board_model::ObjectId::Sha1([byte; 20]);
    let fact = BlobFact {
        kind: Kind::Json,
        record_type: None,
        code: None,
    };
    let index = Index::open(Some(cache.path()), root).unwrap();
    let mut runs = 0;
    let ((), wrote, view) = transact(index, |writer| {
        runs += 1;
        writer.blob(blob(1), Role::Json)?;
        if interfere && runs == 1 {
            let other = Index::open(Some(cache.path()), root).unwrap();
            let other = other.write().unwrap();
            other.put_blob(blob(2), Role::Json, &fact).unwrap();
            other.decide(&[]).unwrap();
        }
        writer.put_blob(blob(3), Role::Json, &fact)?;
        Ok(((), Vec::new()))
    })
    .unwrap();
    (runs, wrote, matches!(view.storage(), Storage::File(_)))
}

#[test]
fn a_transaction_another_process_wrote_under_runs_again_and_writes_to_the_file() {
    assert_eq!(retried(true), (2, true, true));
}

#[test]
fn a_transaction_no_other_process_wrote_under_runs_once_and_writes_to_the_file() {
    assert_eq!(retried(false), (1, true, true));
}

/// The entries of `board`'s working tree, walked without keeping or
/// indexing any file, as for a file `facts` must open itself.
fn unkept(board: &crate::Board) -> Result<Vec<board_tree::Entry>, board_tree::GitError> {
    board
        .repository()
        .walk_working_tree(&mut |_, _| None, &mut |_, _| {})
}

#[test]
fn a_file_changed_after_the_walk_is_stored_under_its_new_blob_and_left_out_while_its_twin_is_kept()
{
    let scenario = board();
    let (old, new, twin) = (task(""), task(r#", "see": "/y""#), task(r#", "see": "/z""#));
    scenario
        .write("tasks/a.json", Item::File(&old))
        .write("tasks/b.json", Item::File(&twin));
    let board = open(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let ((facts, under_new, under_old), _, _) = transact(index, |writer| {
        let entries = unkept(&board)?;
        scenario.write("tasks/a.json", Item::File(&new));
        let mut work = Refreshed::default();
        let facts = facts(
            writer,
            board.repository(),
            &Source::WorkingTree,
            &entries,
            &mut work,
            Kept::default(),
        )?;
        let under_new = writer.blob(scenario.blob(&new), Role::Json)?;
        let under_old = writer.blob(scenario.blob(&old), Role::Json)?;
        Ok(((facts, under_new, under_old), Vec::new()))
    })
    .unwrap();
    assert!(!facts.contains_key("tasks/a.json"), "left out of this run");
    assert_eq!(facts["tasks/b.json"].blob, scenario.blob(&twin));
    assert_eq!(under_new.map(|fact| fact.kind), Some(Kind::Record));
    assert_eq!(
        under_old, None,
        "the new bytes' fact is not stored under the old blob"
    );
}

/// The fact the index gives `big.json`, written with `walked` bytes for the
/// walk and then rewritten with `read` bytes before it is read, or `None`
/// when the run leaves it out.
fn too_large_after(walked: usize, read: usize) -> Option<BlobFact> {
    let scenario = board();
    scenario.write("big.json", Item::File(&"a".repeat(walked)));
    let board = open(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), board.repository().path()).unwrap();
    let (facts, _, _) = transact(index, |writer| {
        let entries = unkept(&board)?;
        if read != walked {
            scenario.write("big.json", Item::File(&"b".repeat(read)));
        }
        facts(
            writer,
            board.repository(),
            &Source::WorkingTree,
            &entries,
            &mut Refreshed::default(),
            Kept::default(),
        )
        .map(|facts| (facts, Vec::new()))
    })
    .unwrap();
    facts.get("big.json").map(|indexed| indexed.fact.clone())
}

#[test]
fn a_file_the_walk_saw_over_16_mib_is_too_large() {
    let over = board_model::MAX_RECORD_BYTES + 1;
    let fact = too_large_after(over, over).unwrap();
    assert_eq!(fact.code, Some(Code::JsonTooLarge));
}

#[test]
fn a_file_the_walk_saw_at_16_mib_that_grew_before_the_read_is_left_out() {
    let limit = board_model::MAX_RECORD_BYTES;
    assert_eq!(too_large_after(limit, limit + 1), None);
}

#[test]
fn a_schema_copied_into_the_reserved_namespace_still_validates_the_records_of_its_own_type() {
    for namespace in ["aa", "zz"] {
        let scenario = board();
        let record = format!(r#"{{"$type": "{namespace}/task/v1", "title": "t"}}"#);
        scenario
            .write(
                &format!(".board/types/{namespace}/task/v1.schema.json"),
                Item::File(TASK_SCHEMA),
            )
            .write(
                ".board/types/board/task/v1.schema.json",
                Item::File(TASK_SCHEMA),
            )
            .write("tasks/a.json", Item::File(&record));
        let report = open(&scenario).check(None, None).unwrap();
        let status = |path: &str| {
            let entry = report.entries.iter().find(|entry| entry.path == path);
            entry.unwrap().status
        };
        assert_eq!(status("tasks/a.json"), Status::Ok, "{namespace}");
        assert_eq!(
            status(".board/types/board/task/v1.schema.json"),
            Status::Degraded(Code::SchemaInvalid),
            "{namespace}"
        );
    }
}
