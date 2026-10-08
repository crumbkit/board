//! Opening boards and checking them, against real repositories.

use board_model::{Code, Fields, Kind, LogicalPath, Object, ObjectId, Source};
use board_scenario::{Item, Scenario};
use board_tree::Status;

use crate::{Board, CheckReport};

pub(crate) mod generated;
pub(crate) mod verdicts;

const CONFIG: &str = r#"{"$type": "board/config/v1"}"#;

pub(crate) const TASK_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["title"],
  "properties": {
    "title": {"type": "string"},
    "project": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"},
    "see": {"type": "string", "format": "board-ref"}
  }
}"#;

pub(crate) const PROJECT_SCHEMA: &str = r#"{"type": "object"}"#;

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
    let mut index = board_index::Index::open(Some(cache), board.repository().path()).unwrap();
    crate::index::transact(&mut index, |writer| {
        let mut row = writer.worktree()?.remove(path).unwrap();
        row.blob = Some(blob);
        Ok(writer.put_worktree(path, &row)?)
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

pub(crate) fn task(fields: &str) -> String {
    format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#)
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
        at("/see", r#"1 is not of type "string""#, 2)
    );
    assert_eq!(
        detail(&scenario, "missing.json"),
        at("", r#""title" is a required property"#, 1)
    );
    assert_eq!(
        detail(&scenario, "malformed.json"),
        at("/project", "it is not a logical path", 1)
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
            "add its schema at .board/types/lr/note/v1.schema.json".to_owned(),
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
    assert_eq!(error.fields(), Fields::at(&missing));
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
fn get_returns_the_blob_of_the_bytes_it_read_when_the_file_changed_after_resolution() {
    let scenario = board();
    let (old, new) = (task(""), task(r#", "see": "/y""#));
    scenario
        .write("work/a.json", Item::File(&old))
        .write("work/b.json", Item::File(&old));
    let board = open(&scenario);
    let locate = |text: &str| {
        let resolved = board
            .repository()
            .resolve(&Source::WorkingTree, &path(text));
        resolved.unwrap()
    };
    let (changed, unchanged) = (locate("/work/a"), locate("/work/b"));
    scenario.write("work/a.json", Item::File(&new));
    let changed = crate::read::read_at(changed, &path("/work/a")).unwrap();
    assert_eq!(changed.location.object, Object::Blob(scenario.blob(&new)));
    assert_eq!(changed.bytes, new.as_bytes());
    let unchanged = crate::read::read_at(unchanged, &path("/work/b")).unwrap();
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
fn get_returns_any_file_with_its_record_or_the_code_it_has_none() {
    let scenario = board();
    let broken = r#"{"$type": "task"}"#;
    scenario
        .write("data.json", Item::File("[1]"))
        .write("notes.md", Item::File("# n"))
        .write("bad.json", Item::File(broken))
        .write("conflict.json", Item::File("<<<<<<< ours\n{}\n"));
    let file = |text: &str| open(&scenario).get(&path(text)).unwrap();
    assert!(file("/projects/p").record.is_ok());
    assert_eq!(file("/notes.md").bytes, b"# n");
    assert_eq!(file("/notes.md").record, Err(Code::RecordNotARecord));
    assert_eq!(file("/data").bytes, b"[1]");
    assert_eq!(file("/data").record, Err(Code::RecordNotARecord));
    assert_eq!(file("/bad").bytes, broken.as_bytes());
    assert_eq!(file("/bad").record, Err(Code::RecordBadType));
    assert_eq!(file("/conflict").record, Err(Code::JsonMergeConflict));
}

#[test]
fn get_refuses_a_directory_and_a_missing_path() {
    let scenario = board();
    assert!(open(&scenario).get(&path("/projects/p")).is_ok());
    let code = |text: &str| open(&scenario).get(&path(text)).unwrap_err().code();
    assert_eq!(code("/projects"), Code::RecordNotARecord);
    assert_eq!(code("/missing"), Code::PathNotFound);
    let fields = |text: &str| open(&scenario).get(&path(text)).unwrap_err().fields();
    assert_eq!(fields("/projects"), Fields::at(&path("/projects")));
    assert_eq!(fields("/missing"), Fields::at(&path("/missing")));
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
    let report = open(&scenario).ls(&path("/work")).unwrap();
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
fn ls_follows_a_link_to_the_directory_it_lists_and_names_it_by_display_path() {
    let scenario = board();
    scenario.write(
        "l.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/projects"}"#),
    );
    let report = open(&scenario).ls(&path("/l")).unwrap();
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
        let report = open(&scenario).ls(&path(asked)).unwrap();
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
    let through = open(&scenario).ls(&path("/shared/work")).unwrap();
    let direct = open(&plain).ls(&path("/work")).unwrap();
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
