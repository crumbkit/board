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
