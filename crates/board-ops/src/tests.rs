//! Opening boards and checking them, against real repositories.

use board_model::{Code, Fields, Kind, LogicalPath, Object, ObjectId, Refusal, Source};
pub(crate) use board_scenario::generated::{PROJECT_SCHEMA, TASK_SCHEMA, task};
use board_scenario::{Item, Scenario};
use board_tree::{Entry, Status};

use crate::{Board, CheckReport, OpenError};

mod complete;
mod frontmatter;
mod markdown;
mod symlinks;
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
    Board::open(scenario.workdir(), None, "").unwrap()
}

/// A library board that a board mounts at `/vendor`, with a task of the
/// board that refers into it. The library's second commit makes its project
/// a link to another, and changes its task and its note, so moving the
/// mount changes what each reference into it reaches.
pub(crate) struct Library {
    library: Scenario,
    second: ObjectId,
}

impl Library {
    /// Mounts a new library in `board` at its first commit.
    pub(crate) fn mounted(board: &Scenario) -> Self {
        let library = Scenario::new();
        let project = r#"{"$type": "lr/project/v1"}"#;
        let to_q = r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#;
        let (see_p, see_none) = (task(r#", "see": "/projects/p""#), task(r#", "see": "/x""#));
        let files = |p, t, note| {
            [
                (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
                (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
                ("projects/p.json", p),
                ("projects/q.json", project),
                ("tasks/t.json", t),
                ("notes.md", note),
            ]
        };
        let first = library.commit(&files(project, &see_p, "[p](/projects/p)"));
        let second = library.commit(&files(to_q, &see_none, "[t](tasks/t.json) [x](/x)"));
        let refers = task(r#", "project": "/vendor/projects/p", "see": "/vendor/tasks/t""#);
        board
            .write("vendor.json", Item::File(&Self::mount(&library, first)))
            .write("tasks/vendor.json", Item::File(&refers));
        Self { library, second }
    }

    /// Moves `board`'s mount to the library's second commit.
    pub(crate) fn moved(&self, board: &Scenario) {
        let mount = Self::mount(&self.library, self.second);
        board.write("vendor.json", Item::File(&mount));
    }

    fn mount(library: &Scenario, commit: ObjectId) -> String {
        format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
            library.path()
        )
    }
}

/// A check answers from a warm index as from a cold one after the mount
/// moves and a record changes.
#[test]
fn a_check_answers_from_a_warm_index_as_from_a_cold_one_after_the_mount_moves() {
    let scenario = board();
    let library = Library::mounted(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let checked = |cache: Option<&std::path::Path>| {
        let report = open(&scenario).check(None, cache).unwrap();
        report.entries
    };
    let before = checked(Some(cache.path()));
    library.moved(&scenario);
    scenario.write("projects/p.json", Item::File(r#"{"$type": "lr/task/v1"}"#));
    let warm = checked(Some(cache.path()));
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, checked(None));
}

/// Makes the next walk of `board`, indexed in `cache`, see `blob` at `path`.
/// The walk trusts a stored blob whose stat still matches, so a stand-in
/// makes it see a blob the file does not hold, as if the file changed
/// after it was hashed.
pub(crate) fn stand_in(board: &Board, cache: &std::path::Path, path: &str, blob: ObjectId) {
    restage(board, cache, path, |entry| entry.blob = Some(blob));
}

/// Stores in `cache` a walk of `board` in which `change` alters the entry
/// at `path`.
fn restage(board: &Board, cache: &std::path::Path, path: &str, change: impl Fn(&mut Entry)) {
    let index = board_index::Index::open(Some(cache), board.repository().path()).unwrap();
    crate::index::transact(index, |writer| {
        let mut worktree = writer.worktree()?;
        let known = &mut |path: &str, stat: &_| worktree.known(path, stat);
        let (mut entries, _) =
            board
                .repository()
                .walk_working_tree(known, &|_, _| {}, &mut |()| {
                    Ok::<(), board_tree::GitError>(())
                })?;
        change(entries.iter_mut().find(|entry| entry.path == path).unwrap());
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
    let from_inside = Board::open(&scenario.workdir().join("work/deep"), None, "").unwrap();
    assert_eq!(
        from_inside.repository().path(),
        open(&scenario).repository().path()
    );
    assert!(from_inside.config().commit_on_put);
    assert!(from_inside.config().plugins.is_empty());
}

#[test]
fn outside_a_repository_or_without_board_json_at_its_root_there_is_no_board() {
    let outside = tempfile::tempdir().unwrap();
    let error = Board::open(outside.path(), None, "").unwrap_err();
    assert!(matches!(error, OpenError::NoRepository { .. }), "{error}");
    assert_eq!(error.code(), Code::ConfigNotFound);
    let plain = Scenario::new();
    plain.write("x.json", Item::File("{}"));
    let error = Board::open(plain.workdir(), None, "").unwrap_err();
    assert!(matches!(error, OpenError::NotFound { .. }), "{error}");
    assert_eq!(error.code(), Code::ConfigNotFound);
    let nested = Scenario::new();
    nested.write("sub/.board/board.json", Item::File(CONFIG));
    let error = Board::open(&nested.workdir().join("sub"), None, "").unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound);
    assert!(Board::open(board().workdir(), None, "").is_ok());
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
    // board.json is hashed when resolution picks it, read to tell whether
    // it is a pointer, and read for its configuration.
    assert_eq!(board.repository().files_opened(), 3);
    let locked = board_beside_large_files();
    if unreadable(&locked, ".board/board.json", CONFIG) {
        let error = Board::open(locked.workdir(), None, "").unwrap_err();
        assert_eq!(error.code(), Code::ConfigInvalid);
    }
}

#[test]
fn a_board_directory_spelled_in_another_case_is_no_board() {
    let scenario = Scenario::new();
    scenario.write(".board/board.json", Item::File(CONFIG));
    assert!(Board::open(scenario.workdir(), None, "").is_ok());
    scenario
        .write(".board", Item::Deleted)
        .write(".Board/board.json", Item::File(CONFIG));
    let error = Board::open(scenario.workdir(), None, "").unwrap_err();
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
        let error = Board::open(scenario.workdir(), None, "").unwrap_err();
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
        .map(board_model::PluginName::as_str)
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

/// Checks `scenario` twice against one cache, and returns what the second
/// check hashed, whether it wrote and how many files it opened, with the
/// status of `n.json`.
fn checked_again(scenario: &Scenario) -> (usize, bool, usize, Status) {
    let cache = tempfile::tempdir().unwrap();
    let board = open(scenario);
    board.check(None, Some(cache.path())).unwrap();
    let opened = board.repository().files_opened();
    let report = board.check(None, Some(cache.path())).unwrap();
    let opened = board.repository().files_opened() - opened;
    let entry = report
        .entries
        .into_iter()
        .find(|entry| entry.path == "n.json");
    let status = entry.unwrap().status;
    (report.work.hashed, report.work.wrote, opened, status)
}

/// A warm check opens a file it could not read once, as a cold rebuild
/// would, in case it now can.
#[test]
fn a_second_check_hashes_and_writes_nothing_whether_or_not_a_schema_can_be_read() {
    let readable = board();
    readable
        .write(NOTE_SCHEMA_PATH, Item::File(PROJECT_SCHEMA))
        .write("n.json", Item::File(NOTE));
    assert_eq!(checked_again(&readable), (0, false, 0, Status::Ok));

    let locked = board();
    locked.write("n.json", Item::File(NOTE));
    if !unreadable(&locked, NOTE_SCHEMA_PATH, PROJECT_SCHEMA) {
        return;
    }
    assert_eq!(
        checked_again(&locked),
        (0, false, 1, Status::Degraded(Code::RepoUnreadable))
    );
}

/// The status of each entry a check of `board` gives with `cache`, and how
/// many files it hashed and whether it wrote.
fn statuses(board: &Board, cache: &std::path::Path) -> (Vec<(String, Status)>, usize, bool) {
    let report = board.check(None, Some(cache)).unwrap();
    let statuses = (report.entries.into_iter())
        .map(|entry| (entry.path, entry.status))
        .collect();
    (statuses, report.work.hashed, report.work.wrote)
}

/// Whether a file can be read can change while its stat does not: a group
/// is granted, a sandbox is lifted, a ticket is renewed. So a warm check
/// tries again a schema the index holds as unreadable, and agrees with a
/// cold one. Its twin, which still cannot read it, is in
/// `a_second_check_hashes_and_writes_nothing_whether_or_not_a_schema_can_be_read`.
#[test]
fn a_schema_the_index_holds_as_unreadable_is_read_by_a_warm_check_once_it_can_be() {
    let scenario = board();
    scenario
        .write(NOTE_SCHEMA_PATH, Item::File(PROJECT_SCHEMA))
        .write("n.json", Item::File(NOTE));
    let board = open(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let (cold, _, _) = statuses(&board, cache.path());
    assert!(cold.iter().all(|(_, status)| *status == Status::Ok));
    restage(&board, cache.path(), NOTE_SCHEMA_PATH, |entry| {
        entry.blob = None;
        entry.status = Status::Degraded(Code::RepoUnreadable);
    });
    // The schema is read but not counted, and its new row is written.
    assert_eq!(statuses(&board, cache.path()), (cold.clone(), 0, true));
    assert_eq!(statuses(&board, cache.path()), (cold, 0, false));
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
            format!(r#"names a type at "/$type": its schema at {schema} does not compile"#),
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
        at(
            "/project",
            r#"holds a reference at "/project": nothing is at /nowhere"#,
            2
        )
    );
    assert_eq!(
        detail(&scenario, "wrong.json"),
        at(
            "/project",
            r#"holds a reference at "/project": /ok is not a record of type lr/project/v1"#,
            1
        )
    );
    assert_eq!(
        detail(&scenario, "link.json"),
        Some((
            None,
            "does not resolve: the link at /link points to /gone, which is missing".to_owned(),
            1
        ))
    );
    assert_eq!(
        detail(&scenario, "unknown.json"),
        Some((
            None,
            "names an unknown type: no schema for lr/note/v1 at .board/types/lr/note/v1.schema.json"
                .to_owned(),
            1
        ))
    );
    assert_eq!(
        detail(&scenario, "broken.json"),
        Some((
            None,
            "is invalid: json does not parse: EOF while parsing an object at line 1 column 1"
                .to_owned(),
            1
        ))
    );
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
    assert_eq!(
        error.refusal().fields,
        Fields {
            ancestor: Some(LogicalPath::root()),
            ..Fields::at(&missing)
        }
        .json()
    );
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
    let mut view = board.repositories().snapshot().unwrap();
    let changed = crate::read::read_at(&mut view, changed, &path("/work/a"), true).unwrap();
    assert_eq!(changed.location.object, Object::Blob(scenario.blob(&new)));
    assert_eq!(changed.bytes, new.as_bytes());
    let unchanged = crate::read::read_at(&mut view, unchanged, &path("/work/b"), true).unwrap();
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
fn check_says_why_a_file_or_schema_is_degraded_and_a_healthy_one_has_no_reason() {
    let scenario = board();
    scenario
        .write(".board/types/board/x/v1.schema.json", Item::File("{}"))
        .write(".board/types/board/y/v1.schema.json", Item::File("{"))
        .write(
            ".board/types/lr/five/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write("twice.json", Item::File(r#"{"a": 1, "a": 2}"#))
        .write(
            "manifest.json",
            Item::File(r#"{"$type": "board/plugin-manifest/v1"}"#),
        );
    let reason = |path: &str| detail(&scenario, path).map(|(_, reason, _)| reason);
    assert_eq!(
        reason(".board/types/board/x/v1.schema.json").as_deref(),
        Some(
            "is in a namespace Board reserves, board or ui, so a board cannot define a schema in it"
        )
    );
    assert_eq!(
        reason(".board/types/board/y/v1.schema.json").as_deref(),
        Some("is invalid: json does not parse: EOF while parsing an object at line 1 column 1")
    );
    let five = reason(".board/types/lr/five/v1.schema.json").unwrap();
    assert!(
        five.starts_with("is invalid: schema does not compile: "),
        "{five}"
    );
    assert_eq!(
        reason("twice.json").as_deref(),
        Some(r#"is invalid: json repeats the key "a" in one object"#)
    );
    assert_eq!(
        reason("manifest.json").as_deref(),
        Some("is invalid: the manifest is not a plugin manifest: missing field `name`")
    );
    assert_eq!(reason(".board/types/lr/task/v1.schema.json"), None);
}

/// A mount of `repo` at `commit`.
fn mount_of(repo: &Scenario, commit: impl std::fmt::Display) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{commit}"}}"#,
        repo.path()
    )
}

/// The entry `check` reports at `logical`.
fn checked(scenario: &Scenario, logical: &str) -> crate::Checked {
    check(scenario)
        .entries
        .into_iter()
        .find(|entry| entry.logical.as_ref().map(LogicalPath::as_str) == Some(logical))
        .unwrap_or_else(|| panic!("{logical} is not an entry"))
}

#[test]
fn a_pointer_failing_inside_a_mount_names_its_fault_from_the_root() {
    let mounted = Scenario::new();
    let commit = mounted.commit(&[(
        "links/gone.json",
        r#"{"$type": "board/mount/v1", "path": "/nowhere"}"#,
    )]);
    let scenario = board();
    scenario.write("shared.json", Item::File(&mount_of(&mounted, commit)));
    let entry = checked(&scenario, "/shared/links/gone");
    assert_eq!(entry.status, Status::Degraded(Code::RefDangling));
    let detail = entry.detail.unwrap();
    assert_eq!(
        detail.at.map(String::from).as_deref(),
        Some("/shared/links/gone")
    );
    assert_eq!(detail.mount.map(String::from).as_deref(), Some("/shared"));
}

#[test]
fn a_pointer_fault_in_a_mounted_board_names_its_mount_point_and_one_in_the_root_names_none() {
    let inner = Scenario::new();
    inner.commit(&[("x.json", "{}")]);
    let missing = board_model::ObjectId::Sha1([9; 20]);
    let deep = Scenario::new();
    let deep_commit = deep.commit(&[("vendor/q.json", &mount_of(&inner, missing))]);
    let mounted = Scenario::new();
    let commit = mounted.commit(&[
        ("vendor/n.json", &mount_of(&inner, missing)),
        ("deep.json", &mount_of(&deep, deep_commit)),
    ]);
    let link = |path: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{path}"}}"#);
    let scenario = board();
    scenario
        .write("shared.json", Item::File(&mount_of(&mounted, commit)))
        .write("vendor/m.json", Item::File(&mount_of(&inner, missing)))
        .write("m.json", Item::File(&mount_of(&inner, missing)))
        .write("links/r.json", Item::File(&link("/shared/vendor/n/x")))
        .write("links/d.json", Item::File(&link("/shared/deep/vendor/q/x")))
        .write("links/s.json", Item::File(&link("/vendor/m/x")));
    let fault = |logical: &str| {
        let entry = checked(&scenario, logical);
        assert_eq!(entry.status, Status::Degraded(Code::MountCommitMissing));
        let detail = entry.detail.unwrap();
        (
            detail.at.unwrap().to_string(),
            detail.mount.map(String::from),
        )
    };
    let shared = Some("/shared".to_owned());
    let nested = ("/shared/vendor/n".to_owned(), shared);
    assert_eq!(fault("/shared/vendor/n"), nested);
    assert_eq!(fault("/links/r"), nested);
    let deeper = (
        "/shared/deep/vendor/q".to_owned(),
        Some("/shared/deep".to_owned()),
    );
    assert_eq!(fault("/links/d"), deeper);
    assert_eq!(fault("/shared/deep/vendor/q"), deeper);
    assert_eq!(fault("/links/s"), ("/vendor/m".to_owned(), None));
    assert_eq!(fault("/vendor/m"), ("/vendor/m".to_owned(), None));
    assert_eq!(fault("/m"), ("/m".to_owned(), None));
}

#[test]
fn a_record_whose_schema_is_at_fault_in_a_mounted_board_names_that_mount_and_one_in_the_root_none()
{
    let mounted = Scenario::new();
    let commit = mounted.commit(&[
        (".board/types/lr/five/v1.schema.json", r#"{"type": 5}"#),
        ("notes/r.json", r#"{"$type": "lr/five/v1"}"#),
        ("notes/u.json", r#"{"$type": "lr/none/v1"}"#),
    ]);
    let scenario = board();
    scenario
        .write("shared.json", Item::File(&mount_of(&mounted, commit)))
        .write(
            ".board/types/lr/five/v1.schema.json",
            Item::File(r#"{"type": 5}"#),
        )
        .write("notes/r.json", Item::File(r#"{"$type": "lr/five/v1"}"#))
        .write("notes/u.json", Item::File(r#"{"$type": "lr/none/v1"}"#));
    let said = |logical: &str| {
        let entry = checked(&scenario, logical);
        let Status::Degraded(code) = entry.status else {
            panic!("{logical} is not degraded");
        };
        (code, entry.detail.unwrap().mount.map(String::from))
    };
    let shared = Some("/shared".to_owned());
    assert_eq!(
        said("/shared/notes/r"),
        (Code::SchemaInvalid, shared.clone())
    );
    assert_eq!(said("/shared/notes/u"), (Code::RecordUnknownType, shared));
    assert_eq!(said("/notes/r"), (Code::SchemaInvalid, None));
    assert_eq!(said("/notes/u"), (Code::RecordUnknownType, None));
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
        refusal("/projects/x/y").fields,
        Fields {
            ancestor: Some(path("/projects")),
            ..Fields::at(&path("/projects/x/y"))
        }
        .json()
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
        hint("get", "/projects/p.json"),
        "a logical path leaves out .json: try `board get /projects/p`"
    );
    assert_eq!(
        hint("ls", "/projects/p.json"),
        "a logical path leaves out .json: try `board ls /projects/p`"
    );
    // Without `.json` the path names nothing either, so the hint lists
    // the deepest ancestor that exists.
    let missing = |text: &str| Fields {
        ancestor: Some(path("/projects")),
        ..Fields::at(&path(text))
    };
    assert_eq!(
        hint("get", "/projects/q.json"),
        Code::PathNotFound.hint(&missing("/projects/q.json"))
    );
    assert_eq!(
        hint("get", "/projects/q"),
        Code::PathNotFound.hint(&missing("/projects/q"))
    );
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
                logical: Some(path("/work/sub")),
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
fn get_each_lists_each_directory_once_and_gives_each_path_what_get_gives_it_alone() {
    let scenario = board();
    for at in 0..40 {
        scenario.write(&format!("tasks/t{at}.json"), Item::File(&task("")));
    }
    let board = open(&scenario);
    let mut paths: Vec<LogicalPath> = (0..40).map(|at| path(&format!("/tasks/t{at}"))).collect();
    paths.extend([path("/tasks"), path("/tasks/none")]);
    // Each answer's bytes or code, and the directories listed to answer.
    let answered = |get: &dyn Fn() -> Vec<Result<crate::GetReport, crate::ReadError>>| {
        let before = board.repository().directories_listed();
        let answers: Vec<_> = (get().into_iter())
            .map(|got| got.map(|report| report.bytes).map_err(|error| error.code()))
            .collect();
        (answers, board.repository().directories_listed() - before)
    };
    let (each, listed_once) = answered(&|| each(&board, &paths));
    let (alone, listed_per_path) = answered(&|| paths.iter().map(|at| board.get(at)).collect());
    assert_eq!(each, alone);
    assert_eq!(
        each[40..],
        [Err(Code::RecordNotARecord), Err(Code::PathNotFound)]
    );
    assert_eq!(listed_once, 2, "the root and tasks, once each");
    // Alone, every path lists the root, and each but /tasks lists tasks too.
    assert_eq!(listed_per_path, 2 * 42 - 1);
}

/// Every answer `get_each` gives `paths`.
fn each(
    board: &crate::Board,
    paths: &[LogicalPath],
) -> Vec<Result<crate::GetReport, crate::ReadError>> {
    board.get_each(paths).unwrap().collect()
}

/// Each answer's display path and bytes, or its code.
fn shapes(
    answers: Vec<Result<crate::GetReport, crate::ReadError>>,
) -> Vec<Result<(String, Vec<u8>), Code>> {
    (answers.into_iter())
        .map(|got| {
            got.map(|report| (report.display.to_string(), report.bytes))
                .map_err(|error| error.code())
        })
        .collect()
}

#[test]
fn get_each_answers_through_links_and_mounts_as_each_path_alone() {
    let mounted = Scenario::new();
    let commit = mounted.commit(&[("x.md", "mounted")]);
    let link = |path: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{path}"}}"#);
    let scenario = board();
    scenario
        .write("shared.json", Item::File(&mount_of(&mounted, commit)))
        .write("links/p.json", Item::File(&link("/projects/p")))
        .write("links/in.json", Item::File(&link("/shared/x.md")))
        .write("links/a.json", Item::File(&link("/links/b")))
        .write("links/b.json", Item::File(&link("/links/a")));
    let board = open(&scenario);
    let asked = [
        "/links/p",
        "/links/in",
        "/links/a",
        "/shared",
        "/shared/x.md",
        "/nothing",
        "/projects",
    ];
    let paths: Vec<LogicalPath> = asked.iter().map(|at| path(at)).collect();
    let alone = shapes(paths.iter().map(|at| board.get(at)).collect());
    assert_eq!(shapes(each(&board, &paths)), alone);
    let project = r#"{"$type": "lr/project/v1"}"#.as_bytes().to_vec();
    assert_eq!(
        alone,
        [
            Ok(("/projects/p".to_owned(), project)),
            Ok(("/shared/x.md".to_owned(), b"mounted".to_vec())),
            Err(Code::ResolveCycle),
            Err(Code::RecordNotARecord),
            Ok(("/shared/x.md".to_owned(), b"mounted".to_vec())),
            Err(Code::PathNotFound),
            Err(Code::RecordNotARecord),
        ]
    );
}

#[test]
fn ls_answers_from_a_warm_index_as_from_a_cold_one_after_the_mount_moves() {
    let scenario = board();
    let library = Library::mounted(&scenario);
    let cache = tempfile::tempdir().unwrap();
    let listed = |asked: &str, cache: Option<&std::path::Path>| {
        let report = open(&scenario).ls(&path(asked), cache).unwrap();
        (report.location, report.listed)
    };
    let before = listed("/vendor/projects", Some(cache.path()));
    library.moved(&scenario);
    scenario.write("tasks/new.json", Item::File(&task("")));
    for asked in ["/vendor/projects", "/tasks"] {
        assert_eq!(
            listed(asked, Some(cache.path())),
            listed(asked, None),
            "{asked}"
        );
    }
    assert_ne!(listed("/vendor/projects", Some(cache.path())), before);
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

#[test]
fn a_board_whose_configuration_is_reached_through_a_symlink_out_of_its_repository_names_the_symlink()
 {
    let outer = Scenario::new();
    outer.write("outside/board.json", Item::File(CONFIG));
    let directory = Scenario::within(&outer);
    directory.write(".board", Item::Symlink("../outside"));
    let file = Scenario::within(&outer);
    file.write(
        ".board/board.json",
        Item::Symlink("../../outside/board.json"),
    );
    for (linked, target) in [
        (directory, "../outside"),
        (file, "../../outside/board.json"),
    ] {
        let refused = Board::open(linked.workdir(), None, "").unwrap_err();
        assert_eq!(refused.code(), Code::SymlinkOutside);
        assert!(refused.to_string().contains("\"../"), "{refused}");
        let fields = refused.fields();
        assert_eq!(fields.path, Some("/.board/board".parse().unwrap()));
        assert_eq!(fields.target.as_deref(), Some(target));
    }
    let inside = Scenario::within(&outer);
    inside
        .write(".board/config.json", Item::File(CONFIG))
        .write(".board/board.json", Item::Symlink("config.json"));
    assert!(Board::open(inside.workdir(), None, "").is_ok());
    let invalid = Scenario::within(&outer);
    invalid.write(".board/board.json", Item::File("{}"));
    let refused = Board::open(invalid.workdir(), None, "").unwrap_err();
    assert_eq!(refused.code(), Code::ConfigInvalid);
    assert_eq!(refused.fields().path, None);
    let real = Scenario::within(&outer);
    real.write(".board/board.json", Item::File(CONFIG));
    assert!(Board::open(real.workdir(), None, "").is_ok());
}

#[test]
fn a_board_whose_configuration_is_a_dangling_symlink_names_the_symlink_and_one_to_a_file_opens() {
    let dangling = Scenario::new();
    dangling.write(".board/board.json", Item::Symlink("config.json"));
    let refused = Board::open(dangling.workdir(), None, "").unwrap_err();
    assert_eq!(refused.code(), Code::SymlinkDangling);
    let fields = refused.fields();
    assert_eq!(fields.path, Some("/.board/board".parse().unwrap()));
    assert_eq!(fields.target.as_deref(), Some("config.json"));
    dangling.write(".board/config.json", Item::File(CONFIG));
    assert!(Board::open(dangling.workdir(), None, "").is_ok());
}
