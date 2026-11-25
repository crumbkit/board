//! The manifest parses as Board reads it and carries the skill's description,
//! the record comparison, the refusal of index codes and the attachment
//! scheme read as the Python plugin read them, a capability the plugin
//! does not offer is refused, and a reference's pointer is resolved through
//! a schema to tell a part-of field.

use serde_json::json;

use super::*;
use closure::part_of;
use to::{Line, capped, refused, same, scheme};

/// A host that does nothing; each call here is refused before the plugin
/// reaches the board.
struct Nothing;

impl Host for Nothing {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("a refused call runs no query")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("a refused call resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("a refused call reads no record")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("a refused call writes no record")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("share calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("share opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("share asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("this host syncs nothing")
    }
    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("this host updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("a refused call reads no settings")
    }
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/share/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Share.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("share", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    assert_eq!(manifest.types, Vec::new());
    assert!(Share.schemas().is_empty(), "the plugin ships no types");
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["to"]);
    assert_eq!(super::manifest()["description"], skill_description());
    let start = "Start with `board -C <board> share to --path /<record> --to /<mount>`, which \
                 lists";
    assert!(
        skill_description().contains(start),
        "{}",
        skill_description()
    );
}

#[test]
fn every_input_property_carries_a_sentence_that_says_what_it_means() {
    let manifest = super::manifest();
    let input = &manifest["capabilities"][0]["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["required"], json!(["path", "to"]));
    let mut described = Vec::new();
    for (name, property) in input["properties"].as_object().unwrap() {
        let sentence = property["description"].as_str().unwrap();
        assert!(sentence.ends_with('.'), "{name}: {sentence}");
        assert!(
            !sentence.contains('\n'),
            "{name} reads as one line, as before: {sentence}"
        );
        described.push(name.as_str());
    }
    assert_eq!(
        described,
        ["allow-uncopied", "local_attachments", "path", "to", "yes"]
    );
    for flag in ["allow-uncopied", "local_attachments", "yes"] {
        assert_eq!(input["properties"][flag]["type"], "boolean", "{flag}");
        assert_eq!(input["properties"][flag]["default"], false, "{flag}");
    }
    for path in ["path", "to"] {
        assert_eq!(input["properties"][path]["format"], "board-path", "{path}");
        assert_eq!(input["properties"][path]["minLength"], 1, "{path}");
    }
}

#[test]
fn the_to_flag_says_how_to_set_up_a_shared_board_and_its_mount() {
    let manifest = super::manifest();
    let to = manifest["capabilities"][0]["inputSchema"]["properties"]["to"]["description"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        to.contains(
            "To share into a team's board, clone its repository, run `board init` there if it \
             is empty, commit and push it, then run `board mount add /team <clone>` here, which \
             follows the branch checked out in the clone. plugins/share/README.md, Setting up a \
             shared board, gives the commands."
        ),
        "{to}"
    );
}

#[test]
fn a_record_held_with_true_where_the_source_has_1_is_different_and_one_in_another_key_order_is_the_same()
 {
    let line = Line::Record(json!({"a": 1, "b": "c"}));
    assert!(!same(&line, br#"{"a": true, "b": "c"}"#));
    assert!(same(&line, br#"{"b": "c", "a": 1}"#));
    assert!(!same(&line, b"not json"));
    let text = Line::Text("# A\n".to_owned());
    assert!(same(&text, b"# A\n"));
    assert!(!same(&text, b"# A"));
}

#[test]
fn a_record_held_with_minus_zero_where_the_source_has_zero_is_different_and_one_with_zero_is_the_same()
 {
    let line = Line::Record(json!({"x": 0.0}));
    assert!(!same(&line, br#"{"x": -0.0}"#));
    assert!(same(&line, br#"{"x": 0.0}"#));
}

#[test]
fn a_held_integer_board_cannot_hold_exactly_differs_from_the_float_it_reads_as_and_that_float_is_the_same()
 {
    let line = Line::Record(json!({"x": 1e20}));
    assert!(!same(&line, br#"{"x": 100000000000000000000}"#));
    assert!(same(&line, br#"{"x": 1e20}"#));
}

#[test]
fn an_index_code_other_than_a_broken_reference_is_refused_and_a_broken_reference_is_admitted() {
    assert!(refused("record", Some("board::record::invalid")));
    assert!(refused("file", Some("board::frontmatter::invalid")));
    assert!(!refused("record", Some("board::ref::dangling")));
    assert!(!refused("record", None));
    assert!(!refused("record", Some("")));
    assert!(!refused("file", Some("board::record::invalid")));
}

#[test]
fn an_attachment_location_has_a_scheme_only_when_letters_start_it_before_a_colon() {
    for (location, expected) in [
        ("s3://objects/9f86", "s3"),
        ("HTTPS://example.com/a", "https"),
        ("file:///Users/me/a", "file"),
        ("svn+ssh://host/a", "svn+ssh"),
        ("/Users/me/objects/9f86", ""),
        ("objects/9f86", ""),
        ("1s3://objects", ""),
        ("None", ""),
    ] {
        assert_eq!(scheme(location), expected, "{location}");
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_and_to_reaches_its_own_check() {
    let other: CapabilityName = "from".parse().unwrap();
    let refused = Share.call(&Nothing, &other, json!({})).unwrap_err();
    assert_eq!(refused.code, "share::call::unknown_capability");
    assert_eq!(
        refused.message,
        "share offers no capability from; it offers to"
    );
    let known: CapabilityName = "to".parse().unwrap();
    let refused = Share
        .call(&Nothing, &known, json!({"path": "/evals/x", "to": "team"}))
        .unwrap_err();
    assert_eq!(refused.code, "share::mount::not_found");
}

#[test]
fn a_capped_list_keeps_the_first_entries_and_counts_them_all() {
    assert_eq!(capped(vec!["/a", "/b", "/c"], 2), (vec!["/a", "/b"], 3));
    assert_eq!(capped(vec!["/a"], 2), (vec!["/a"], 1));
}

/// A schema that marks `parent`, the items of `parts` and, through a local
/// `$ref`, `whole`, as naming the record this one is part of, and leaves
/// `after` unmarked.
fn parts_schema() -> Value {
    let marked = json!({"type": "string", "format": "board-ref", "x-board-part-of": true});
    json!({
        "$defs": {"Whole": marked, "Holder": {"type": "object", "properties": {"whole": {"$ref": "#/$defs/Whole"}}}},
        "type": "object",
        "properties": {
            "parent": marked,
            "after": {"type": "array", "items": {"type": "string", "format": "board-ref"}},
            "parts": {"type": "array", "items": marked},
            "holder": {"$ref": "#/$defs/Holder"},
            "whole": {"$ref": "#/$defs/Whole", "description": "The whole."},
        },
    })
}

#[test]
fn a_pointer_reaches_a_part_of_field_through_properties_items_and_a_local_ref() {
    let schema = parts_schema();
    for pointer in [
        "/parent",
        "/parts/0",
        "/parts/12",
        "/whole",
        "/holder/whole",
    ] {
        assert!(part_of(&schema, pointer), "{pointer}");
    }
    for pointer in ["/after/0", "/parts", "/holder", ""] {
        assert!(!part_of(&schema, pointer), "{pointer}");
    }
}

#[test]
fn a_pointer_that_reaches_no_field_schema_is_not_part_of() {
    let schema = parts_schema();
    for pointer in ["/missing", "/parent/0", "/parts/x", "/holder/missing"] {
        assert!(!part_of(&schema, pointer), "{pointer}");
    }
    let dangling = json!({"properties": {"a": {"$ref": "#/$defs/None"}}});
    assert!(!part_of(&dangling, "/a"));
    let looping =
        json!({"$defs": {"A": {"$ref": "#/$defs/A"}}, "properties": {"a": {"$ref": "#/$defs/A"}}});
    assert!(!part_of(&looping, "/a"));
    let remote = json!({"properties": {"a": {"$ref": "https://example.com/a"}}});
    assert!(!part_of(&remote, "/a"));
    let escaped = json!({"properties": {"a/b": {"x-board-part-of": true}}});
    assert!(part_of(&escaped, "/a~1b"));
    let not_true = json!({"properties": {"a": {"x-board-part-of": "yes"}}});
    assert!(!part_of(&not_true, "/a"));
}

/// A host over a real Board in `board`'s working tree, which notes each
/// path a call asks `refs` about.
struct Counting {
    board: board_scenario::Scenario,
    asked: std::cell::RefCell<Vec<String>>,
}

impl Counting {
    fn open(&self) -> board_ops::Board {
        board_ops::Board::open(self.board.workdir(), None).unwrap()
    }
}

impl Host for Counting {
    fn query(
        &self,
        sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        self.open().query(sql, usize::MAX, None, None)
    }

    fn refs(
        &self,
        paths: &[board_model::LogicalPath],
        absent: board_ops::Absent,
        fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        (self.asked.borrow_mut()).extend(paths.iter().map(ToString::to_string));
        Ok(self
            .open()
            .refs(paths, absent, fields, None, None)?
            .collect())
    }

    fn get_each(
        &self,
        paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        Ok(self.open().get_each(paths)?.collect())
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("closure writes nothing")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("share calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("share opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("share asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("closure syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("closure updates no mount")
    }

    fn settings(&self) -> &board_ops::Settings {
        panic!("closure reads no settings")
    }
}

/// A board holding the project `/tasks/p.md`, its task `/tasks/c.md`, 300
/// change records whose `task` names the project, and two decisions whose
/// `refs` name it. Every change and decision also names `/notes/far.md`.
fn three_hundred_changes() -> Counting {
    use board_scenario::{Item, Scenario};
    let board = Scenario::board();
    let change_type = r#"{"type": "object", "properties": {"task": {"type": "string", "format": "board-ref"}, "paths": {"type": "array", "items": {"type": "string", "format": "board-ref"}}}}"#;
    let decision = |name: &str| {
        json!({
            "$type": "plans/decision/v1", "decision": name, "why": "It reads well.",
            "by": {"person": "ana"}, "decided": "2026-11-20T09:00:00Z",
            "refs": ["/tasks/p.md", "/notes/far.md"],
        })
        .to_string()
    };
    board
        .write(
            ".board/types/tasks/task/v2.schema.json",
            Item::File(include_str!(
                "../../board-plugin-tasks/types/tasks/task/v2.schema.json"
            )),
        )
        .write(
            ".board/types/plans/decision/v1.schema.json",
            Item::File(include_str!(
                "../../board-plugin-plans/types/plans/decision/v1.schema.json"
            )),
        )
        .write(
            ".board/types/lr/change/v1.schema.json",
            Item::File(change_type),
        )
        .write(
            "tasks/p.md",
            Item::File("---\n$type: tasks/task/v2\ntitle: P\nstatus: open\nkind: project\n---\n"),
        )
        .write(
            "tasks/c.md",
            Item::File(
                "---\n$type: tasks/task/v2\ntitle: C\nstatus: open\nparent: /tasks/p.md\n---\n",
            ),
        )
        .write("notes/far.md", Item::File("# Far\n"))
        .write("projects/p/decisions/a.json", Item::File(&decision("A")))
        .write("projects/p/decisions/b.json", Item::File(&decision("B")));
    let change =
        json!({"$type": "lr/change/v1", "task": "/tasks/p.md", "paths": ["/notes/far.md"]});
    for number in 0..300 {
        let at = format!("changes/c{number:03}.json");
        board.write(&at, Item::File(&change.to_string()));
    }
    Counting {
        board,
        asked: std::cell::RefCell::default(),
    }
}

/// STANDARDS.md 20: a whole's decisions cost one `refs` question each, and
/// the change records that name the whole and the paths a decision names
/// cost none.
#[test]
fn a_wholes_decisions_are_asked_about_once_each_and_its_change_records_and_their_targets_never() {
    let host = three_hundred_changes();
    let found = closure::closure(&host, "/tasks/c.md").unwrap();
    assert_eq!(
        found.found,
        [
            "/projects/p/decisions/a",
            "/projects/p/decisions/b",
            "/tasks/c.md",
            "/tasks/p.md"
        ]
    );
    let asked = host.asked.borrow();
    let count = |path: &str| asked.iter().filter(|asked| *asked == path).count();
    assert_eq!(
        (
            count("/projects/p/decisions/a"),
            count("/projects/p/decisions/b"),
            count("/notes/far.md")
        ),
        (1, 1, 0),
        "{asked:?}"
    );
    assert!(
        !asked.iter().any(|path| path.starts_with("/changes/")),
        "{asked:?}"
    );
}

/// A host over a real Board in `board`'s working tree that writes through
/// `board put`, where another writer may put `first` just before the next
/// put.
struct Racing {
    board: board_scenario::Scenario,
    first: std::cell::RefCell<Option<Value>>,
    settings: board_ops::Settings,
}

impl Racing {
    fn open(&self) -> board_ops::Board {
        board_ops::Board::open(self.board.workdir(), None).unwrap()
    }

    fn put_line(&self, line: &Value) -> Result<board_ops::PutReport, board_ops::PutError> {
        self.open().put_batch(&mut line.to_string().as_bytes())
    }
}

impl Host for Racing {
    fn query(
        &self,
        sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        self.open().query(sql, usize::MAX, None, None)
    }

    fn refs(
        &self,
        paths: &[board_model::LogicalPath],
        absent: board_ops::Absent,
        fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        Ok(self
            .open()
            .refs(paths, absent, fields, None, None)?
            .collect())
    }

    fn get_each(
        &self,
        paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        Ok(self.open().get_each(paths)?.collect())
    }

    fn put(&self, lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        if let Some(line) = self.first.borrow_mut().take() {
            self.put_line(&line).unwrap();
        }
        let batch: Vec<String> = lines.iter().map(Value::to_string).collect();
        self.open().put_batch(&mut batch.join("\n").as_bytes())
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("share calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("share opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("share asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("share syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("share updates no mount")
    }

    fn settings(&self) -> &board_ops::Settings {
        &self.settings
    }
}

/// The schema of the note `/notes/a` the source board holds.
const NOTE_SCHEMA: &str = r#"{"type": "object"}"#;

/// A board holding the note `/notes/a` of a type of its own, which mounts
/// at `/team` an empty team board that commits its puts, and the team
/// board.
fn note_and_team() -> (Racing, board_scenario::Scenario) {
    use board_scenario::{Item, Scenario};
    let team = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": true}"#;
    team.write(".board/board.json", Item::File(config));
    let first = team.commit(&[(".board/board.json", config)]);
    team.track(first).identified();
    let board = Scenario::board();
    // A put through the mount moves its pin, which commits in this board too.
    board.identified();
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{first}"}}"#,
        team.path()
    );
    board
        .write(
            ".board/types/lr/note/v1.schema.json",
            Item::File(NOTE_SCHEMA),
        )
        .write("notes/a.json", Item::File(r#"{"$type": "lr/note/v1"}"#))
        .write("team.json", Item::File(&mount));
    let host = Racing {
        board,
        first: std::cell::RefCell::default(),
        settings: board_ops::Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            utc_offset: 0,
            caller: None,
            environment: board_ops::Variables::default(),
            cache_home: None,
        },
    };
    (host, team)
}

/// Shares `/notes/a` into `/team`, writing.
fn shared(host: &Racing) -> Result<Value, PluginError> {
    to::share_to(
        host,
        &json!({"path": "/notes/a", "to": "/team", "yes": true}),
    )
}

#[test]
fn a_schema_the_destination_lacks_is_written_with_the_set() {
    let (host, team) = note_and_team();
    let output = shared(&host).unwrap();
    assert_eq!(output["types"], json!(["lr/note/v1"]));
    assert!(
        team.workdir()
            .join(".board/types/lr/note/v1.schema.json")
            .exists()
    );
    assert!(team.workdir().join("notes/a.json").exists());
}

/// The share after another writer put `schema` at the destination's
/// `lr/note/v1` schema between the share's read and its write.
fn shared_after(schema: &Value) -> (Result<Value, PluginError>, board_scenario::Scenario) {
    let (host, team) = note_and_team();
    *host.first.borrow_mut() = Some(json!({
        "path": "/team/.board/types/lr/note/v1.schema",
        "record": schema,
    }));
    (shared(&host), team)
}

/// A schema line takes no expect: Board refuses one on it, writes a schema
/// the board lacks and leaves an equal one it holds. So a different schema
/// another writer stores first refuses the whole batch, and an equal one
/// is left as theirs while the set is copied.
#[test]
fn a_different_schema_another_writer_stores_first_refuses_the_share_and_an_equal_one_is_kept() {
    let theirs = json!({"type": "object", "title": "theirs"});
    let (refused, team) = shared_after(&theirs);
    let refused = refused.unwrap_err();
    assert_eq!(
        refused.fields["code"], "board::put::invalid",
        "{}",
        refused.message
    );
    assert!(
        !team.workdir().join("notes/a.json").exists(),
        "no copy is written"
    );
    let held = std::fs::read(team.workdir().join(".board/types/lr/note/v1.schema.json")).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&held).unwrap(), theirs);
    let equal: Value = serde_json::from_str(NOTE_SCHEMA).unwrap();
    let (admitted, team) = shared_after(&equal);
    admitted.unwrap();
    assert!(
        team.workdir().join("notes/a.json").exists(),
        "the set is copied"
    );
}
