//! The shapes generate the schemas the shape rules ask for.
#![expect(dead_code, reason = "these types exist only for their schemas")]

use schemars::JsonSchema;
use serde_json::json;

use super::*;

#[derive(JsonSchema)]
struct Task;

impl Record for Task {
    const TYPE: &'static str = "tasks/task/v2";
}

#[derive(JsonSchema)]
struct Note;

impl Record for Note {
    const TYPE: &'static str = "notes/note/v1";
}

/// Accepts either a task or a note.
enum Either {}

impl Accepts for Either {
    const TYPES: &'static [&'static str] = &[Task::TYPE, Note::TYPE];
}

#[derive(JsonSchema)]
struct Inner {
    /// Somewhere.
    place: Ref,
}

#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Mood {
    Calm,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
struct Sample {
    #[serde(rename = "$type")]
    tag: Tag<Task>,
    task: Ref<Task>,
    note: Ref<Note>,
    either: Ref<Either>,
    anything: Option<Ref>,
    inner: Option<Inner>,
    #[serde(default)]
    after: Vec<Ref<Task>>,
    mood: Option<Mood>,
    count: Option<u16>,
}

#[derive(JsonSchema)]
struct Output {
    next: Option<String>,
    inner: Option<Inner>,
}

fn sample() -> Value {
    schema_of::<Sample>(SchemaSettings::draft2020_12())
}

#[test]
fn a_tag_is_its_record_type_as_a_constant() {
    assert_eq!(
        sample()["properties"]["$type"],
        json!({"type": "string", "const": "tasks/task/v2"})
    );
}

#[test]
fn a_reference_names_one_type_several_or_none_and_two_types_share_no_definition() {
    let properties = &sample()["properties"];
    let reference = |types: Value| {
        let mut schema = json!({"type": "string", "format": "board-ref"});
        if !types.is_null() {
            schema["x-board-type"] = types;
        }
        schema
    };
    assert_eq!(properties["task"], reference(json!("tasks/task/v2")));
    assert_eq!(properties["note"], reference(json!("notes/note/v1")));
    assert_eq!(
        properties["either"],
        reference(json!(["tasks/task/v2", "notes/note/v1"]))
    );
    assert_eq!(properties["anything"], reference(Value::Null));
    let defs = sample().get("$defs").cloned().unwrap_or_default();
    let names: Vec<&str> = defs
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, ["Inner"]);
}

#[test]
fn a_tag_or_reference_at_a_schema_root_is_titled_by_its_shape() {
    let title = |schema: Value| schema["title"].clone();
    let settings = SchemaSettings::draft2020_12;
    assert_eq!(
        (
            title(schema_of::<Tag<Task>>(settings())),
            title(schema_of::<Ref>(settings()))
        ),
        (json!("Tag"), json!("Ref"))
    );
}

#[test]
fn a_list_of_references_puts_the_reference_in_items() {
    let after = &sample()["properties"]["after"];
    assert_eq!(after["type"], "array");
    assert_eq!(
        after["items"],
        json!({"type": "string", "format": "board-ref", "x-board-type": "tasks/task/v2"})
    );
}

#[test]
fn an_optional_field_is_left_out_of_required_and_admits_no_null() {
    let schema = sample();
    let required = &schema["required"];
    assert_eq!(required, &json!(["$type", "task", "note", "either"]));
    let properties = &schema["properties"];
    assert_eq!(properties["inner"], json!({"$ref": "#/$defs/Inner"}));
    assert_eq!(
        properties["mood"],
        json!({"type": "string", "enum": ["calm"]})
    );
    assert_eq!(
        properties["count"],
        json!({"type": "integer", "minimum": 0, "maximum": 65535})
    );
    assert!(!schema.to_string().contains("null"), "{schema}");
}

#[test]
fn a_field_an_output_always_holds_keeps_null_and_an_optional_one_loses_it() {
    let output = schema_of::<Output>(SchemaSettings::draft2020_12().for_serialize());
    assert_eq!(output["required"], json!(["next", "inner"]));
    assert_eq!(
        output["properties"]["next"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        output["properties"]["inner"]["anyOf"],
        json!([{"$ref": "#/$defs/Inner"}, {"type": "null"}])
    );
}

/// The generated schema compiles in Board, finds each reference with the
/// types it accepts, and refuses null where the twin without it is admitted.
#[test]
fn board_finds_each_reference_and_refuses_null_in_an_optional_field() {
    let schema = board_validate::Schema::compile(sample()).unwrap();
    let record = json!({
        "$type": "tasks/task/v2",
        "task": "/t",
        "note": "/n",
        "either": "/e",
        "inner": {"place": "/p"},
        "after": ["/a"],
    });
    schema.validate(&record).unwrap();
    let found: Vec<(String, usize)> = (schema.references(&record).into_iter())
        .map(|reference| {
            (
                reference.pointer,
                reference.accepts.map_or(0, |types| types.len()),
            )
        })
        .collect();
    let expected = [
        ("/after/0", 1),
        ("/either", 2),
        ("/inner/place", 0),
        ("/note", 1),
        ("/task", 1),
    ];
    assert_eq!(
        found,
        expected.map(|(pointer, types)| (pointer.to_owned(), types))
    );
    let mut nulled = record;
    nulled["inner"] = Value::Null;
    assert_eq!(
        schema.validate(&nulled).unwrap_err().code(),
        board_model::Code::RecordInvalid
    );
}

#[derive(JsonSchema)]
struct Noted {
    /// Why it matters.
    #[schemars(with = "Prose")]
    why: String,
    /// When it is due.
    #[schemars(with = "Date")]
    due: String,
}

#[test]
fn a_prose_field_is_marked_as_prose_and_a_date_field_is_a_date() {
    let properties = &schema_of::<Noted>(SchemaSettings::draft2020_12())["properties"];
    assert_eq!(
        properties["why"],
        json!({"description": "Why it matters.", "type": "string", "x-board-prose": true})
    );
    assert_eq!(
        properties["due"],
        json!({"description": "When it is due.", "type": "string", "format": "date"})
    );
}

#[test]
fn the_registry_holds_each_shared_shape_by_name_as_a_field_using_it_sees_it() {
    let registry = registry();
    assert_eq!(
        registry.keys().map(String::as_str).collect::<Vec<_>>(),
        ["Date", "Prose", "Url"]
    );
    let properties = &schema_of::<Noted>(SchemaSettings::draft2020_12())["properties"];
    let mut why = properties["why"].clone();
    why.as_object_mut().unwrap().remove("description");
    assert_eq!(registry["Prose"], why);
}

/// The committed `shapes.json` holds, byte for byte, the canonical JSON of
/// the registry. `SNAPSHOTS=overwrite` rewrites the file.
#[expect(
    clippy::disallowed_methods,
    reason = "shapes.json is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_shapes_file_is_the_registry() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shapes.json");
    let generated = board_model::canonical_json(&Value::Object(registry()));
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::write(&file, generated.as_bytes()).unwrap();
    }
    let committed = std::fs::read(&file).unwrap();
    assert!(
        committed == generated.as_bytes(),
        "{} differs from shape::registry(); run this test with SNAPSHOTS=overwrite:\n{}",
        file.display(),
        String::from_utf8_lossy(generated.as_bytes())
    );
}

/// Board's validator refuses each URL without a lower-case `http` or `https`
/// scheme or without a host, and admits its twin.
#[test]
fn a_url_without_a_host_or_a_web_scheme_is_refused_and_its_twin_is_admitted() {
    let schema =
        board_validate::Schema::compile(schema_of::<Url>(SchemaSettings::draft2020_12())).unwrap();
    let code = |text: &str| schema.validate(&json!(text)).err().map(|e| e.code());
    let invalid = Some(board_model::Code::RecordInvalid);
    for (refused, admitted) in [
        ("https://", "https://a"),
        ("http://u@", "http://u@h"),
        ("https://:80", "https://h:80"),
        ("https://u@:80/x", "https://u@[::1]:80/x"),
        ("HTTPS://example.com", "https://example.com"),
        ("ftp://example.com/x", "https://example.com/x"),
        ("/pull/1", "https://example.com/pull/1"),
        ("https://a b", "http://localhost:8080/x"),
    ] {
        assert_eq!((refused, code(refused)), (refused, invalid));
        assert_eq!((admitted, code(admitted)), (admitted, None));
    }
}
