//! Opening boards and checking them, against real repositories.

use board_model::{Code, Fields, Kind, LogicalPath, Object, ObjectId, Refusal, Source};
pub(crate) use board_scenario::generated::{PROJECT_SCHEMA, TASK_SCHEMA, task};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::{Board, CheckReport};

pub(crate) mod verdicts;

const CONFIG: &str = r#"{"$type": "board/config/v1"}"#;

/// A board with the task and project types and one project.
pub(crate) fn board() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(".board/board.json", Item::File(CONFIG))
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        )
        .write(
            ".board/types/lr/project/v1.schema.json",
            Item::File(PROJECT_SCHEMA),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    scenario
}

pub(crate) fn open(scenario: &Scenario) -> Board {
    Board::open(scenario.workdir(), None).unwrap()
}

/// Makes the next walk of `board`, indexed in `cache`, see `blob` at `path`.
/// The walk trusts a stored blob whose stat still matches, so a stand-in
/// makes it see a blob the file does not hold, as if the file changed
/// after it was hashed.
pub(crate) fn stand_in(board: &Board, cache: &std::path::Path, path: &str, blob: ObjectId) {
    let index = board_index::Index::open(Some(cache), board.repository().path()).unwrap();
    crate::index::transact(index, |writer| {
        let mut worktree = writer.worktree()?;
        let known = &mut |path: &str, stat: &_| worktree.known(path, stat);
        let mut entries = board
            .repository()
            .walk_working_tree(known, &mut |_, _| {})?;
        let entry = entries.iter_mut().find(|entry| entry.path == path).unwrap();
        entry.blob = Some(blob);
        assert_eq!(writer.sync_worktree(worktree, &entries)?, 0);
        Ok(((), Vec::new()))
    })
    .unwrap();
}

fn check(scenario: &Scenario) -> CheckReport {
    open(scenario).check(None, None).unwrap()
}

/// The status `check` gives the entry at `path`.
fn status(scenario: &Scenario, path: &str) -> Status {
    check(scenario)
        .entries
        .into_iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("{path} is not an entry"))
        .status
}

/// Where the note type's schema lives in a board.
pub(crate) const NOTE_SCHEMA_PATH: &str = ".board/types/lr/note/v1.schema.json";

pub(crate) const NOTE: &str = r#"{"$type": "lr/note/v1"}"#;

/// Writes `text` at `path` as a file no one may read, and says whether
/// Board cannot read it. A superuser reads a mode-000 file anyway, so there
/// the fixture cannot stage the failure; CI runs as an ordinary user.
pub(crate) fn unreadable(scenario: &Scenario, path: &str, text: &str) -> bool {
    scenario.write(path, Item::Unreadable(text));
    std::fs::File::open(scenario.workdir().join(path)).is_err()
}

/// Writes `item`, a directory the walk's user cannot list, at `path`, and
/// says whether Board cannot list it. A superuser lists any directory, so
/// there the fixture cannot stage the failure.
pub(crate) fn locked(scenario: &Scenario, path: &str, item: Item<'_>) -> bool {
    scenario.write(path, item);
    std::fs::read_dir(scenario.workdir().join(path)).is_err()
}

#[test]
fn a_board_is_found_from_any_directory_inside_it() {
    let scenario = board();
    scenario.write("work/deep/x.md", Item::File(""));
    let from_inside = Board::open(&scenario.workdir().join("work/deep"), None).unwrap();
    assert_eq!(
        from_inside.repository().path(),
        open(&scenario).repository().path()
    );
    assert!(from_inside.config().commit_on_put);
    assert!(from_inside.config().plugins.is_empty());
}

#[test]
fn without_board_json_at_the_repository_root_there_is_no_board() {
    let plain = Scenario::new();
    plain.write("x.json", Item::File("{}"));
    assert_eq!(
        Board::open(plain.workdir(), None).unwrap_err().code(),
        Code::ConfigNotFound
    );
    let nested = Scenario::new();
    nested.write("sub/.board/board.json", Item::File(CONFIG));
    let error = Board::open(&nested.workdir().join("sub"), None).unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound);
    assert!(Board::open(board().workdir(), None).is_ok());
}

/// A board whose root holds several files beside `.board`, one of them
/// large.
fn board_beside_large_files() -> Scenario {
    let scenario = Scenario::new();
    let large = "a".repeat(1 << 20);
    scenario
        .write("large.bin", Item::File(&large))
        .write("a.md", Item::File("a"))
        .write("b.json", Item::File("{}"))
        .write(".board/types/x.schema.json", Item::File("{}"));
    scenario
}

#[test]
fn opening_a_board_opens_only_its_configuration_and_an_unreadable_one_is_invalid() {
    let readable = board_beside_large_files();
    readable.write(".board/board.json", Item::File(CONFIG));
    let board = open(&readable);
    // board.json is hashed when resolution picks it and read to tell
    // whether it is a pointer. The read for its configuration goes through
    // a lent repository, which counts its own files.
    assert_eq!(board.repository().files_opened(), 2);
    let locked = board_beside_large_files();
    if unreadable(&locked, ".board/board.json", CONFIG) {
        let error = Board::open(locked.workdir(), None).unwrap_err();
        assert_eq!(error.code(), Code::ConfigInvalid);
    }
}

#[test]
fn a_board_directory_spelled_in_another_case_is_no_board() {
    let scenario = Scenario::new();
    scenario.write(".board/board.json", Item::File(CONFIG));
    assert!(Board::open(scenario.workdir(), None).is_ok());
    scenario
        .write(".board", Item::Deleted)
        .write(".Board/board.json", Item::File(CONFIG));
    let error = Board::open(scenario.workdir(), None).unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound);
}

#[test]
fn a_board_json_board_cannot_read_is_invalid() {
    for bad in [
        r#"{"$type": "board/config/v1", "colour": "red"}"#,
        r#"{"$type": "board/config/v2"}"#,
        r#"{"$type": "lr/task/v1"}"#,
        r#"{"$type": "board/config/v1", "plugins": [1]}"#,
        r#"{"$type": "board/config/v1", "plugins": ["Tasks"]}"#,
        r#"{"$type": "board/config/v1", "plugins": ["tasks", "notes", "tasks"]}"#,
        "{",
    ] {
        let scenario = Scenario::new();
        scenario.write(".board/board.json", Item::File(bad));
        let error = Board::open(scenario.workdir(), None).unwrap_err();
        assert_eq!(error.code(), Code::ConfigInvalid, "{bad}");
    }
    let scenario = Scenario::new();
    scenario.write(
        ".board/board.json",
        Item::File(
            r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["tasks", "notes"]}"#,
        ),
    );
    let board = open(&scenario);
    assert!(!board.config().commit_on_put);
    let plugins: Vec<&str> = board
        .config()
        .plugins
        .iter()
        .map(board_plugin::PluginName::as_str)
        .collect();
    assert_eq!(plugins, ["tasks", "notes"]);
}

#[test]
fn a_clean_board_checks_ok_with_every_kind_reported() {
    let scenario = board();
    let to_p = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    scenario
        .write(
            "work/t.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write("work/l.json", Item::File(to_p))
        .write("work/data.json", Item::File("[1, 2]"))
        .write("work/notes.md", Item::File("# notes"));
    let report = check(&scenario);
    assert!(!report.degraded(), "{report:#?}");
    let kinds: Vec<(&str, Kind)> = report
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (".board/board.json", Kind::Record),
            (".board/types/lr/project/v1.schema.json", Kind::Schema),
            (".board/types/lr/task/v1.schema.json", Kind::Schema),
            ("projects/p.json", Kind::Record),
            ("work/data.json", Kind::Json),
            ("work/l.json", Kind::Pointer),
            ("work/notes.md", Kind::File),
            ("work/t.json", Kind::Record),
        ]
    );
}

#[test]
fn a_record_whose_type_has_no_schema_is_unknown() {
    let scenario = board();
    scenario
        .write("a.json", Item::File(&task("")))
        .write("b.json", Item::File(r#"{"$type": "lr/note/v1"}"#));
    assert_eq!(status(&scenario, "a.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "b.json"),
        Status::Degraded(Code::RecordUnknownType)
    );
}

#[test]
fn a_record_whose_schema_cannot_be_read_is_unreadable_and_its_twin_under_a_readable_schema_is_ok() {
    let readable = board();
    readable
        .write(NOTE_SCHEMA_PATH, Item::File(PROJECT_SCHEMA))
        .write("n.json", Item::File(NOTE));
    assert_eq!(status(&readable, "n.json"), Status::Ok);

    let locked = board();
    locked.write("n.json", Item::File(NOTE));
    if !unreadable(&locked, NOTE_SCHEMA_PATH, PROJECT_SCHEMA) {
        return;
    }
    assert_eq!(
        status(&locked, "n.json"),
        Status::Degraded(Code::RepoUnreadable)
    );
}

#[test]
fn a_schema_made_unreadable_after_a_check_leaves_its_records_unreadable_warm_and_cold() {
    let scenario = board();
    scenario
        .write(NOTE_SCHEMA_PATH, Item::File(PROJECT_SCHEMA))
        .write("n.json", Item::File(NOTE));
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let status = |cache: &std::path::Path| {
        let report = board.check(None, Some(cache)).unwrap();
        let entry = report
            .entries
            .into_iter()
            .find(|entry| entry.path == "n.json");
        entry.unwrap().status
    };
    assert_eq!(status(cache.path()), Status::Ok);
    if !unreadable(&scenario, NOTE_SCHEMA_PATH, PROJECT_SCHEMA) {
        return;
    }
    assert_eq!(
        status(cache.path()),
        Status::Degraded(Code::RepoUnreadable),
        "warm"
    );
    let cold = tempfile::tempdir().unwrap();
    assert_eq!(
        status(cold.path()),
        Status::Degraded(Code::RepoUnreadable),
        "cold"
    );
}

/// A plugin manifest that Board admits, with one capability.
pub(crate) const MANIFEST: &str = r#"{"$type": "board/plugin-manifest/v1", "name": "p", "version": "0.1.0",
  "description": "d", "run": ["./run"], "capabilities": [{"name": "c", "description": "d",
  "inputSchema": {"type": "object"}, "outputSchema": {"type": "object"}}]}"#;

#[test]
fn a_plugin_manifest_is_checked_by_its_content_and_a_broken_one_is_a_bad_manifest() {
    let scenario = board();
    let broken = MANIFEST.replace(r#"["./run"]"#, "[]");
    // Neither directory is named p: an installed plugin's directory rule
    // does not apply to a manifest in a board.
    scenario
        .write("plugins/q/plugin.json", Item::File(MANIFEST))
        .write("plugins/r/plugin.json", Item::File(&broken));
    assert_eq!(status(&scenario, "plugins/q/plugin.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "plugins/r/plugin.json"),
        Status::Degraded(Code::PluginBadManifest)
    );
}

#[test]
fn a_schema_that_does_not_compile_degrades_itself_and_its_records() {
    let scenario = board();
    scenario
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write("n.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("t.json", Item::File(&task("")));
    let schema = ".board/types/lr/note/v1.schema.json";
    assert_eq!(
        status(&scenario, schema),
        Status::Degraded(Code::SchemaInvalid)
    );
    assert_eq!(
        status(&scenario, "n.json"),
        Status::Degraded(Code::SchemaInvalid)
    );
    assert_eq!(
        detail(&scenario, "n.json"),
        Some((
            Some("/$type".to_owned()),
            format!("its schema at {schema} does not compile"),
            1
        ))
    );
    assert_eq!(status(&scenario, "t.json"), Status::Ok);
}

#[test]
fn a_record_its_schema_refuses_is_invalid() {
    let scenario = board();
    scenario.write("ok.json", Item::File(&task(""))).write(
        "bad.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": 1}"#),
    );
    assert_eq!(status(&scenario, "ok.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "bad.json"),
        Status::Degraded(Code::RecordInvalid)
    );
}

#[test]
fn references_are_checked_for_shape_target_and_type() {
    let scenario = board();
    scenario
        .write(
            "ok.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write(
            "malformed.json",
            Item::File(&task(r#", "project": "projects/p""#)),
        )
        .write(
            "dangling.json",
            Item::File(&task(r#", "project": "/projects/q""#)),
        )
        .write("wrong.json", Item::File(&task(r#", "project": "/ok""#)))
        .write("untyped.json", Item::File(&task(r#", "see": "/ok""#)))
        .write(
            "untyped-dangling.json",
            Item::File(&task(r#", "see": "/nowhere""#)),
        );
    assert_eq!(status(&scenario, "ok.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "malformed.json"),
        Status::Degraded(Code::RefMalformed)
    );
    assert_eq!(
        status(&scenario, "dangling.json"),
        Status::Degraded(Code::RefDangling)
    );
    assert_eq!(
        status(&scenario, "wrong.json"),
        Status::Degraded(Code::RefWrongType)
    );
    assert_eq!(status(&scenario, "untyped.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "untyped-dangling.json"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn a_reference_to_something_other_than_a_record_is_the_wrong_type() {
    let scenario = board();
    scenario.write("projects/notes.md", Item::File("")).write(
        "t.json",
        Item::File(&task(r#", "project": "/projects/notes.md""#)),
    );
    assert_eq!(
        status(&scenario, "t.json"),
        Status::Degraded(Code::RefWrongType)
    );
}

/// What `check` says of the entry at `path`: its pointer, reason and
/// failure count.
fn detail(scenario: &Scenario, path: &str) -> Option<(Option<String>, String, u64)> {
    let entry = check(scenario)
        .entries
        .into_iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("{path} is not an entry"));
    entry
        .detail
        .map(|detail| (detail.pointer, detail.reason, detail.failures))
}

/// The schema file `check` names for the entry at `path`.
fn named(scenario: &Scenario, path: &str) -> Option<String> {
    check(scenario)
        .entries
        .into_iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("{path} is not an entry"))
        .schema
}

#[test]
fn check_names_the_schema_of_a_record_whose_schema_is_missing_unusable_or_unreadable_and_not_of_one_whose_schema_is_usable()
 {
    let usable = board();
    usable
        .write(NOTE_SCHEMA_PATH, Item::File(PROJECT_SCHEMA))
        .write("n.json", Item::File(NOTE));
    assert_eq!(status(&usable, "n.json"), Status::Ok);
    assert_eq!(named(&usable, "n.json"), None);

    let named_as = |scenario: &Scenario, code: Code| {
        assert_eq!(status(scenario, "n.json"), Status::Degraded(code));
        assert_eq!(named(scenario, "n.json").as_deref(), Some(NOTE_SCHEMA_PATH));
    };
    let missing = board();
    missing.write("n.json", Item::File(NOTE));
    named_as(&missing, Code::RecordUnknownType);

    let unusable = board();
    unusable
        .write(NOTE_SCHEMA_PATH, Item::File(r#"{"type": 5}"#))
        .write("n.json", Item::File(NOTE));
    named_as(&unusable, Code::SchemaInvalid);

    let unreadable_schema = board();
    unreadable_schema.write("n.json", Item::File(NOTE));
    if !unreadable(&unreadable_schema, NOTE_SCHEMA_PATH, PROJECT_SCHEMA) {
        return;
    }
    named_as(&unreadable_schema, Code::RepoUnreadable);
}

#[test]
fn a_degraded_entry_says_where_and_why_and_how_many_failures_it_has() {
    let scenario = board();
    scenario
        .write(
            "ok.json",
            Item::File(&task(r#", "project": "/projects/p""#)),
        )
        .write(
            "invalid.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": 5, "see": 1}"#),
        )
        .write("missing.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
        .write(
            "malformed.json",
            Item::File(&task(r#", "project": "projects/p""#)),
        )
        .write(
            "dangling.json",
            Item::File(&task(r#", "project": "/nowhere", "see": "/gone""#)),
        )
        .write("wrong.json", Item::File(&task(r#", "project": "/ok""#)))
        .write(
            "link.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/gone"}"#),
        )
        .write("unknown.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("broken.json", Item::File("{"));
    let at = |pointer: &str, reason: &str, failures| {
        Some((Some(pointer.to_owned()), reason.to_owned(), failures))
    };
    assert_eq!(detail(&scenario, "ok.json"), None);
    assert_eq!(
        detail(&scenario, "invalid.json"),
        at(
            "/see",
            r#"fails its schema at "/see": 1 is not of type "string""#,
            2
        )
    );
    assert_eq!(
        detail(&scenario, "missing.json"),
        at(
            "",
            r#"fails its schema at its root: "title" is a required property"#,
            1
        )
    );
    assert_eq!(
        detail(&scenario, "malformed.json"),
        at(
            "/project",
            r#"holds a reference at "/project": "projects/p" is not a logical path"#,
            1
        )
    );
    assert_eq!(
        detail(&scenario, "dangling.json"),
        at("/project", "nothing is at /nowhere", 2)
    );
    assert_eq!(
        detail(&scenario, "wrong.json"),
        at("/project", "/ok is not a record of type lr/project/v1", 1)
    );
    assert_eq!(
        detail(&scenario, "link.json"),
        Some((
            None,
            "the link at /link points to /gone, which is missing".to_owned(),
            1
        ))
    );
    assert_eq!(
        detail(&scenario, "unknown.json"),
        Some((
            None,
            "no schema for lr/note/v1 at .board/types/lr/note/v1.schema.json".to_owned(),
            1
        ))
    );
    assert_eq!(detail(&scenario, "broken.json"), None);
}

#[test]
fn a_pointer_whose_target_is_missing_is_dangling_in_the_walk() {
    let scenario = board();
    scenario
        .write(
            "good.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
        )
        .write(
            "bad.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#),
        );
    assert_eq!(status(&scenario, "good.json"), Status::Ok);
    assert_eq!(
        status(&scenario, "bad.json"),
        Status::Degraded(Code::RefDangling)
    );
}

#[test]
fn parse_problems_and_path_problems_come_first() {
    // Every name here is one a case-insensitive file system can hold; case
    // collisions are covered on commits in board-tree, on every platform.
    let scenario = board();
    scenario
        .write("dup.json", Item::File(r#"{"a": 1, "a": 2}"#))
        .write(".json", Item::File("{"))
        .write("t.json", Item::File(&task("")));
    assert_eq!(
        status(&scenario, "dup.json"),
        Status::Degraded(Code::JsonDuplicateKey)
    );
    assert_eq!(
        status(&scenario, ".json"),
        Status::Degraded(Code::PathMalformed)
    );
    assert_eq!(status(&scenario, "t.json"), Status::Ok);
}

#[test]
fn a_scope_checks_only_entries_at_or_beneath_it() {
    let scenario = board();
    scenario
        .write("work/t.json", Item::File(&task("")))
        .write("workshop/t.json", Item::File(&task("")));
    let scope: LogicalPath = "/work".parse().unwrap();
    let report = open(&scenario).check(Some(&scope), None).unwrap();
    let paths: Vec<&str> = report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, ["work/t.json"]);
    let missing: LogicalPath = "/nowhere".parse().unwrap();
    let error = open(&scenario).check(Some(&missing), None).unwrap_err();
    assert_eq!(error.code(), Code::PathNotFound);
    assert_eq!(error.to_string(), "nothing is at /nowhere");
    assert_eq!(error.refusal().fields, Fields::at(&missing).json());
}

#[test]
fn check_fails_when_any_entry_is_degraded() {
    let scenario = board();
    assert!(!check(&scenario).degraded());
    scenario.write("bad.json", Item::File("{"));
    assert!(check(&scenario).degraded());
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

#[test]
fn get_returns_a_records_stored_bytes_and_where_it_is() {
    let scenario = board();
    let stored = "{\n  \"title\": \"t\",   \"$type\": \"lr/task/v1\"\n}\n";
    scenario.write("work/t.json", Item::File(stored));
    let report = open(&scenario).get(&path("/work/t")).unwrap();
    assert_eq!(report.bytes, stored.as_bytes());
    assert_eq!(report.record.unwrap()["title"], "t");
    assert_eq!(report.location.path, "work/t.json");
    assert_eq!(report.display.as_str(), "/work/t");
}

#[test]
fn get_returns_a_record_whose_schema_cannot_be_read() {
    let scenario = board();
    scenario.write("notes/n.json", Item::File(NOTE));
    if !unreadable(&scenario, NOTE_SCHEMA_PATH, PROJECT_SCHEMA) {
        return;
    }
    let report = open(&scenario).get(&path("/notes/n")).unwrap();
    assert_eq!(report.bytes, NOTE.as_bytes());
    assert_eq!(report.record.unwrap()["$type"], "lr/note/v1");
}

#[test]
fn get_returns_the_blob_of_the_bytes_it_read_when_the_file_changed_after_resolution() {
    let scenario = board();
    let (old, new) = (task(""), task(r#", "see": "/y""#));
    scenario
        .write("work/a.json", Item::File(&old))
        .write("work/b.json", Item::File(&old));
    let board = open(&scenario);
    let locate = |text: &str| {
        let resolved = board
            .repositories()
            .resolve(&Source::WorkingTree, &path(text));
        resolved.unwrap()
    };
    let (changed, unchanged) = (locate("/work/a"), locate("/work/b"));
    scenario.write("work/a.json", Item::File(&new));
    let changed = crate::read::read_at(board.repositories(), changed, &path("/work/a")).unwrap();
    assert_eq!(changed.location.object, Object::Blob(scenario.blob(&new)));
    assert_eq!(changed.bytes, new.as_bytes());
    let unchanged =
        crate::read::read_at(board.repositories(), unchanged, &path("/work/b")).unwrap();
    assert_eq!(unchanged.location.object, Object::Blob(scenario.blob(&old)));
}

#[test]
fn get_follows_a_link_and_shows_where_it_landed() {
    let scenario = board();
    scenario.write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#),
    );
    let report = open(&scenario).get(&path("/l")).unwrap();
    assert_eq!(report.display.as_str(), "/projects/p");
    assert_eq!(report.location.pointers.len(), 1);
}

#[test]
fn get_returns_any_file_with_its_record_or_the_refusal_that_says_why_it_has_none() {
    let scenario = board();
    let broken = r#"{"$type": "task"}"#;
    scenario
        .write("data.json", Item::File("[1]"))
        .write("notes.md", Item::File("# n"))
        .write("bad.json", Item::File(broken))
        .write("conflict.json", Item::File("<<<<<<< ours\n{}\n"));
    let file = |text: &str| open(&scenario).get(&path(text)).unwrap();
    let refused = |text: &str| {
        let refusal = file(text).record.unwrap_err();
        (refusal.code.as_str().to_owned(), refusal.message)
    };
    assert!(file("/projects/p").record.is_ok());
    assert_eq!(file("/notes.md").bytes, b"# n");
    let not_a_record = |text: &str| {
        (
            Code::RecordNotARecord.to_string(),
            format!("{text} is not a record"),
        )
    };
    assert_eq!(refused("/notes.md"), not_a_record("/notes.md"));
    assert_eq!(file("/data").bytes, b"[1]");
    assert_eq!(refused("/data"), not_a_record("/data"));
    assert_eq!(file("/bad").bytes, broken.as_bytes());
    assert_eq!(
        refused("/bad"),
        (
            Code::RecordBadType.to_string(),
            "/bad is not a record".to_owned()
        )
    );
    assert_eq!(
        refused("/conflict"),
        (
            Code::JsonMergeConflict.to_string(),
            "/conflict is not a record: json holds a git conflict marker on line 1".to_owned()
        )
    );
}

#[test]
fn get_refuses_bytes_that_do_not_parse_with_the_parsers_message_and_names_the_blob() {
    let scenario = board();
    let (torn, whole) = (r#"{"$type": "lr/task/v1", "#, r#"{"$type": "lr/task/v1"}"#);
    scenario
        .write("work/torn.json", Item::File(torn))
        .write("work/whole.json", Item::File(whole));
    let get = |text: &str| open(&scenario).get(&path(text)).unwrap();
    assert_eq!(get("/work/whole").record.unwrap()["$type"], "lr/task/v1");
    let refusal = get("/work/torn").record.unwrap_err();
    assert_eq!(refusal.code.as_str(), Code::JsonParse.as_str());
    assert_eq!(
        refusal.message,
        "/work/torn is not a record: json does not parse: EOF while parsing an object at line 1 column 24"
    );
    let fields = Fields {
        blob: Some(scenario.blob(torn)),
        ..Fields::at(&path("/work/torn"))
    };
    assert_eq!(
        refusal,
        Refusal::new(Code::JsonParse, &refusal.message, &fields)
    );
}

#[test]
fn get_refuses_each_json_fault_with_its_code_and_the_path_before_the_parsers_message() {
    let scenario = board();
    let cases: [(&str, &[u8], Code, &str); 3] = [
        (
            "twice",
            br#"{"a": 1, "a": 2}"#,
            Code::JsonDuplicateKey,
            r#"json repeats the key "a" in one object"#,
        ),
        (
            "latin",
            b"{\"a\": \"\xff\"}",
            Code::JsonNotUtf8,
            "json is not utf-8: invalid utf-8 sequence of 1 bytes from index 7",
        ),
        (
            "huge",
            br#"{"$type": "lr/task/v1", "n": 18446744073709551616}"#,
            Code::JsonLossyNumber,
            "number 18446744073709551616 cannot be held exactly",
        ),
    ];
    scenario.write(
        "fine.json",
        Item::File(r#"{"$type": "lr/task/v1", "n": 18446744073709551615}"#),
    );
    for (name, bytes, ..) in &cases {
        scenario.write(&format!("{name}.json"), Item::Bytes(bytes));
    }
    let board = open(&scenario);
    assert!(board.get(&path("/fine")).unwrap().record.is_ok());
    for (name, _, code, parser) in cases {
        let refusal = board
            .get(&path(&format!("/{name}")))
            .unwrap()
            .record
            .unwrap_err();
        assert_eq!(refusal.code.as_str(), code.as_str(), "{name}");
        assert_eq!(
            refusal.message,
            format!("/{name} is not a record: {parser}")
        );
    }
}

#[test]
fn get_refuses_a_directory_and_a_missing_path() {
    let scenario = board();
    assert!(open(&scenario).get(&path("/projects/p")).is_ok());
    let code = |text: &str| open(&scenario).get(&path(text)).unwrap_err().code();
    assert_eq!(code("/projects"), Code::RecordNotARecord);
    assert_eq!(code("/missing"), Code::PathNotFound);
    let refusal = |text: &str| open(&scenario).get(&path(text)).unwrap_err().refusal("get");
    assert_eq!(
        refusal("/projects").fields,
        Fields::at(&path("/projects")).json()
    );
    assert_eq!(
        refusal("/missing").fields,
        Fields::at(&path("/missing")).json()
    );
}

#[test]
fn a_missing_path_that_ends_json_is_refused_with_a_hint_naming_it_without_json() {
    let scenario = board();
    scenario.write("d.json/n.md", Item::File("n"));
    let board = open(&scenario);
    let hint = |command: &'static str, text: &str| {
        let refused = match command {
            "ls" => board.ls(&path(text), None).unwrap_err(),
            _ => board.get(&path(text)).unwrap_err(),
        };
        let refusal = refused.refusal(command);
        assert_eq!(refusal.code.as_str(), refused.code().as_str());
        refusal.hint.unwrap()
    };
    assert_eq!(
        hint("get", "/tasks/t9.json"),
        "a logical path leaves out .json: try `board get /tasks/t9`"
    );
    assert_eq!(
        hint("ls", "/tasks/t9.json"),
        "a logical path leaves out .json: try `board ls /tasks/t9`"
    );
    let missing = Fields::at(&path("/tasks/t9"));
    assert_eq!(hint("get", "/tasks/t9"), Code::PathNotFound.hint(&missing));
    // A refusal other than a missing path keeps its own hint.
    let directory = Fields::at(&path("/d.json"));
    assert_eq!(
        hint("get", "/d.json"),
        Code::RecordNotARecord.hint(&directory)
    );
}

#[test]
fn get_reads_a_file_of_16_mib_and_refuses_a_larger_one() {
    let scenario = board();
    let limit = board_model::MAX_RECORD_BYTES;
    scenario
        .write("at.md", Item::File(&"a".repeat(limit)))
        .write("over.md", Item::File(&"a".repeat(limit + 1)));
    let board = open(&scenario);
    assert_eq!(board.get(&path("/at.md")).unwrap().bytes.len(), limit);
    let refused = board.get(&path("/over.md")).unwrap_err();
    assert_eq!(refused.code(), Code::JsonTooLarge);
}

#[test]
fn get_returns_a_file_reached_through_a_link() {
    let scenario = board();
    scenario.write("docs/a.md", Item::File("# a")).write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/docs/a.md"}"#),
    );
    let report = open(&scenario).get(&path("/l")).unwrap();
    assert_eq!(report.bytes, b"# a");
    assert_eq!(report.display.as_str(), "/docs/a.md");
}

#[test]
fn ls_lists_one_level_with_directories_kinds_and_statuses() {
    let scenario = board();
    scenario
        .write("work/t.json", Item::File(&task("")))
        .write("work/sub/x.md", Item::File(""))
        .write("work/broken.json", Item::File("{"))
        .write(
            "work/l.json",
            Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
        );
    let report = open(&scenario).ls(&path("/work"), None).unwrap();
    assert_eq!(
        report.listed,
        [
            crate::Listed::Entry {
                name: "broken.json".to_owned(),
                logical: Some(path("/work/broken")),
                kind: Kind::Json,
                status: Status::Degraded(Code::JsonParse),
            },
            crate::Listed::Entry {
                name: "l.json".to_owned(),
                logical: Some(path("/work/l")),
                kind: Kind::Pointer,
                status: Status::Ok,
            },
            crate::Listed::Directory {
                name: "sub".to_owned(),
                logical: path("/work/sub"),
            },
            crate::Listed::Entry {
                name: "t.json".to_owned(),
                logical: Some(path("/work/t")),
                kind: Kind::Record,
                status: Status::Ok,
            },
        ]
    );
}

#[test]
fn ls_gives_each_entry_the_status_check_gives_it_and_their_valid_twins_are_ok() {
    let scenario = board();
    let link = |target: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#);
    scenario
        .write(
            "work/invalid.json",
            Item::File(r#"{"$type": "lr/task/v1"}"#),
        )
        .write("work/valid.json", Item::File(&task("")))
        .write("work/dangling.json", Item::File(&link("/projects/q")))
        .write("work/resolving.json", Item::File(&link("/projects/p")))
        .write("work/.json", Item::File("{}"));
    let board = open(&scenario);
    let listed: Vec<(String, Kind, Status)> = board
        .ls(&path("/work"), None)
        .unwrap()
        .listed
        .into_iter()
        .filter_map(|listed| match listed {
            crate::Listed::Entry {
                name, kind, status, ..
            } => Some((name, kind, status)),
            crate::Listed::Directory { .. } => None,
        })
        .collect();
    let degraded = |name: &str, kind, code| (name.to_owned(), kind, Status::Degraded(code));
    let ok = |name: &str, kind| (name.to_owned(), kind, Status::Ok);
    assert_eq!(
        listed,
        [
            degraded(".json", Kind::File, Code::PathMalformed),
            degraded("dangling.json", Kind::Pointer, Code::RefDangling),
            degraded("invalid.json", Kind::Record, Code::RecordInvalid),
            ok("resolving.json", Kind::Pointer),
            ok("valid.json", Kind::Record),
        ]
    );
}

#[test]
fn ls_reads_the_tree_once_and_hashes_each_file_once_cold_none_warm_and_only_a_changed_one_after() {
    let scenario = board();
    for at in 0..100 {
        scenario.write(&format!("tasks/a/t{at}.json"), Item::File(&task("")));
        scenario.write(&format!("tasks/b/t{at}.json"), Item::File(&task("")));
    }
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    // Files hashed, directories listed and files opened.
    let work = |asked: &str| {
        let repository = board.repository();
        let (listed, opened) = (repository.directories_listed(), repository.files_opened());
        let report = board.ls(&path(asked), Some(cache.path())).unwrap();
        (
            report.work.hashed,
            repository.directories_listed() - listed,
            repository.files_opened() - opened,
        )
    };
    // The whole tree has 10 directories. Listing /tasks/a also lists the
    // root and tasks/ twice more, to resolve the path and the check's scope.
    assert_eq!(work("/tasks/a"), (204, 14, 204), "every file, once");
    assert_eq!(work("/tasks/a"), (0, 14, 0));
    assert_eq!(work("/"), (0, 10, 0));
    // A new blob is validated, so the task schema is read to compile it.
    scenario.write("tasks/b/t7.json", Item::File(&task(r#", "x": 1"#)));
    assert_eq!(work("/tasks/a"), (1, 14, 2));
    scenario.write("tasks/a/t7.json", Item::File(&task(r#", "x": 1"#)));
    assert_eq!(work("/tasks/a"), (1, 14, 1));
}

#[test]
fn ls_follows_a_link_to_the_directory_it_lists_and_names_it_by_display_path() {
    let scenario = board();
    scenario.write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    let report = open(&scenario).ls(&path("/l"), None).unwrap();
    assert_eq!(report.display.as_str(), "/projects");
    assert_eq!(
        report.listed,
        [crate::Listed::Entry {
            name: "p.json".to_owned(),
            logical: Some(path("/projects/p")),
            kind: Kind::Record,
            status: Status::Ok,
        }]
    );
}

#[test]
fn ls_of_a_file_lists_that_file() {
    let scenario = board();
    scenario.write("notes/readme.md", Item::File("# r")).write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    for (asked, name, shown) in [
        ("/projects/p", "p.json", "/projects/p"),
        ("/l/p", "p.json", "/projects/p"),
        ("/notes/readme.md", "readme.md", "/notes/readme.md"),
    ] {
        let report = open(&scenario).ls(&path(asked), None).unwrap();
        assert_eq!(report.listed.len(), 1, "{asked}");
        assert!(
            matches!(&report.listed[0], crate::Listed::Entry { name: listed, logical: Some(logical), .. }
                if listed == name && logical.as_str() == shown),
            "{asked}: {:?}",
            report.listed
        );
    }
}

#[test]
fn ls_through_a_mount_lists_what_the_same_directory_lists_in_a_plain_board() {
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("work/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("work/broken.json", "{"),
        ("work/sub/x.md", ""),
        ("elsewhere/q.json", r#"{"$type": "lr/project/v1"}"#),
    ];
    let plain = Scenario::new();
    for (path, text) in files {
        plain.write(path, Item::File(text));
    }
    let mounted = Scenario::new();
    let commit = mounted.commit(&files);
    let scenario = board();
    scenario.write(
        "shared.json",
        Item::File(&format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            mounted.path()
        )),
    );
    let through = open(&scenario).ls(&path("/shared/work"), None).unwrap();
    let direct = open(&plain).ls(&path("/work"), None).unwrap();
    let shape = |report: &crate::LsReport| -> Vec<(String, Option<Kind>, Option<Status>)> {
        report
            .listed
            .iter()
            .map(|listed| match listed {
                crate::Listed::Entry {
                    name, kind, status, ..
                } => (name.clone(), Some(*kind), Some(*status)),
                crate::Listed::Directory { name, .. } => (name.clone(), None, None),
            })
            .collect()
    };
    assert_eq!(shape(&through), shape(&direct));
    assert_eq!(shape(&direct).len(), 3);
    assert_eq!(through.display.as_str(), "/shared/work");
}
