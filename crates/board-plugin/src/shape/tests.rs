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
fn a_tag_writes_its_record_type_and_reads_only_that_type() {
    assert_eq!(
        serde_json::to_value(Tag::<Task>::default()).unwrap(),
        json!("tasks/task/v2")
    );
    assert!(serde_json::from_value::<Tag<Task>>(json!("tasks/task/v2")).is_ok());
    let refused = serde_json::from_value::<Tag<Task>>(json!("tasks/task/v1")).unwrap_err();
    assert_eq!(
        refused.to_string(),
        r#"$type is "tasks/task/v1", not "tasks/task/v2""#
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
        [
            "Actor",
            "Date",
            "Digest",
            "Glob",
            "LogicalGlob",
            "ObjectId",
            "PathTemplate",
            "Pattern",
            "Person",
            "Pinned",
            "Prose",
            "Repository",
            "SessionId",
            "Slug",
            "Subject",
            "Summary",
            "TypeName",
            "Url"
        ]
    );
    let properties = &schema_of::<Noted>(SchemaSettings::draft2020_12())["properties"];
    let mut why = properties["why"].clone();
    why.as_object_mut().unwrap().remove("description");
    assert_eq!(registry["Prose"], why);
}

#[test]
fn a_summary_of_25_words_on_one_line_is_admitted_and_one_of_26_words_or_two_lines_is_refused() {
    let schema = json!({
        "type": "object",
        "properties": {"summary": registry()["Summary"].clone()},
    });
    let schema = board_validate::Schema::compile(schema).unwrap();
    let summary = |text: &str| json!({ "summary": text });
    let words = |count: usize| vec!["word"; count].join(" ");
    assert_eq!(SUMMARY_WORDS, 25);
    for (refused, admitted) in [
        (words(SUMMARY_WORDS + 1), words(SUMMARY_WORDS)),
        ("Two\nlines.".to_owned(), "One line.".to_owned()),
        (" Leading space.".to_owned(), "No leading space.".to_owned()),
        (String::new(), "One.".to_owned()),
    ] {
        assert_eq!(
            schema.validate(&summary(&refused)).unwrap_err().code(),
            board_model::Code::RecordInvalid,
            "{refused:?}"
        );
        schema.validate(&summary(&admitted)).unwrap();
    }
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

/// Board's validator refuses a digest that is not sha256: and 64 lowercase hex digits
/// and an empty glob, and admits each one's twin.
#[test]
fn a_digest_without_its_prefix_or_lowercase_hex_and_an_empty_glob_are_refused_and_their_twins_are_admitted()
 {
    let compiled = |schema| board_validate::Schema::compile(schema).unwrap();
    let digest = compiled(schema_of::<Digest>(SchemaSettings::draft2020_12()));
    let glob = compiled(schema_of::<Glob>(SchemaSettings::draft2020_12()));
    let code = |schema: &board_validate::Schema, text: &str| {
        schema.validate(&json!(text)).err().map(|e| e.code())
    };
    let invalid = Some(board_model::Code::RecordInvalid);
    let lower = "0123456789abcdef".repeat(4);
    for refused in [
        lower.clone(),
        format!("sha256:{}", lower.to_uppercase()),
        format!("sha256:{}", &lower[1..]),
        format!("sha256:{lower}0"),
        format!("SHA256:{lower}"),
    ] {
        assert_eq!(code(&digest, &refused), invalid, "{refused}");
    }
    assert_eq!(code(&digest, &format!("sha256:{lower}")), None);
    assert_eq!(code(&glob, ""), invalid);
    assert_eq!(code(&glob, "*"), None);
}

/// A logical glob starts at the board's root, as a logical path does.
#[test]
fn a_logical_glob_without_a_leading_slash_is_refused_and_one_with_it_is_admitted() {
    let schema = schema_of::<LogicalGlob>(SchemaSettings::draft2020_12());
    let glob = board_validate::Schema::compile(schema).unwrap();
    let code = |text: &str| glob.validate(&json!(text)).err().map(|e| e.code());
    assert_eq!(code("repos/x/**"), Some(board_model::Code::RecordInvalid));
    assert_eq!(code("/repos/x/**"), None);
}

/// Compiles the schema of `T` and gives the code each value is refused
/// with, or none when it is admitted.
fn refusal_of<T: JsonSchema>() -> impl Fn(Value) -> Option<board_model::Code> {
    let schema =
        board_validate::Schema::compile(schema_of::<T>(SchemaSettings::draft2020_12())).unwrap();
    move |value| schema.validate(&value).err().map(|error| error.code())
}

/// An actor names at least one of its session, program, person and agent,
/// each in its own grammar, and nothing else.
#[test]
fn an_actor_with_no_part_a_bad_part_or_an_unknown_key_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<Actor>();
    let invalid = Some(board_model::Code::RecordInvalid);
    let session = "local_84ccdcef-4c52-487a-9ac6-ebf2f157e440";
    for (refused, admitted) in [
        (json!({}), json!({"program": "codex"})),
        (json!({"session": "84ccdcef"}), json!({"session": session})),
        (
            json!({"program": "Codex nightly scheduler"}),
            json!({"program": "claude-code"}),
        ),
        (json!({"person": "Luke"}), json!({"person": "luke"})),
        (
            json!({"person": "a b@c"}),
            json!({"person": "owner@example.com"}),
        ),
        (
            json!({"agent": 7}),
            json!({"agent": "Codex nightly scheduler"}),
        ),
        (
            json!({"person": "luke", "model": "opus"}),
            json!({"session": session, "program": "claude-code", "person": "luke"}),
        ),
    ] {
        assert_eq!((code(refused.clone()), &refused), (invalid, &refused));
        assert_eq!((code(admitted.clone()), &admitted), (None, &admitted));
    }
}

/// An actor reads and writes only the parts it holds.
#[test]
fn an_actor_writes_only_the_parts_it_holds() {
    let actor = Actor {
        program: Some("codex".into()),
        ..Actor::default()
    };
    assert_eq!(
        serde_json::to_value(&actor).unwrap(),
        json!({"program": "codex"})
    );
    assert_eq!(
        serde_json::from_value::<Actor>(json!({"program": "codex"})).unwrap(),
        actor
    );
    let all = Actor {
        session: Some("local_1".into()),
        person: Some("luke".into()),
        agent: Some("Codex builder".into()),
        ..actor.clone()
    };
    assert_eq!(
        (actor.to_string(), all.to_string()),
        ("codex".into(), "codex, Codex builder, luke, local_1".into())
    );
}

/// A subject is an `owner/name` repository or a board path.
#[test]
fn a_subject_that_is_neither_a_repository_nor_a_path_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<Subject>();
    let invalid = Some(board_model::Code::RecordInvalid);
    for (refused, admitted) in [
        ("board", "crumbkit/board"),
        ("Raycast Workspaces", "raycast/extensions"),
        ("crumbkit/board/crates", "/repos/board/crates"),
        ("/", "/repos/board"),
        ("/repos/a b", "nextest-rs/nextest"),
    ] {
        assert_eq!((refused, code(json!(refused))), (refused, invalid));
        assert_eq!((admitted, code(json!(admitted))), (admitted, None));
    }
}

/// A repository is `owner/name`, as GitHub names it.
#[test]
fn a_repository_that_is_not_owner_and_name_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<Repository>();
    let invalid = Some(board_model::Code::RecordInvalid);
    for (refused, admitted) in [
        ("typescript-standards", "crumbkit/typescript-standards"),
        ("/repos/board", "crumbkit/board"),
        ("crumbkit/board/crates", "TanStack/query"),
        ("a b/c", "llvm/llvm-project"),
    ] {
        assert_eq!((refused, code(json!(refused))), (refused, invalid));
        assert_eq!((admitted, code(json!(admitted))), (admitted, None));
    }
}

/// A pinned reference names a record and the 40- or 64-digit blob it was
/// read at.
#[test]
fn a_pinned_reference_with_a_short_blob_or_no_record_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<Pinned>();
    let invalid = Some(board_model::Code::RecordInvalid);
    let sha1 = "a".repeat(40);
    let sha256 = "b".repeat(64);
    for (refused, admitted) in [
        (
            json!({"record": "/r", "blob": "a".repeat(39)}),
            json!({"record": "/r", "blob": sha1}),
        ),
        (
            json!({"record": "/r", "blob": "b".repeat(50)}),
            json!({"record": "/r", "blob": sha256}),
        ),
        (
            json!({"blob": "a".repeat(40)}),
            json!({"record": "/r", "blob": "a".repeat(40)}),
        ),
    ] {
        assert_eq!((code(refused.clone()), &refused), (invalid, &refused));
        assert_eq!((code(admitted.clone()), &admitted), (None, &admitted));
    }
    let schema = schema_of::<Pinned>(SchemaSettings::draft2020_12());
    assert_eq!(
        schema["properties"]["record"],
        json!({"description": "The record.", "type": "string", "format": "board-ref"})
    );
}

/// The registry holds each object shape as a record schema's `$defs` holds
/// it, so the lint can compare a type's copy with it.
#[test]
fn the_registry_holds_each_object_shape_as_a_record_schemas_defs_holds_it() {
    #[derive(JsonSchema)]
    struct Uses {
        /// Who.
        by: Actor,
        /// What.
        subject: Pinned,
    }
    let schema = schema_of::<Uses>(SchemaSettings::draft2020_12());
    let registry = registry();
    assert_eq!(schema["$defs"]["Actor"], registry["Actor"]);
    assert_eq!(schema["$defs"]["Pinned"], registry["Pinned"]);
    assert!(!registry["Actor"].to_string().contains("null"));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
enum Weather {
    Sunny,
    LightRain,
}

/// An open value reads a value its enum knows as `Known`, keeps one it does
/// not know as `Other`, and writes each back as it was read.
#[test]
fn an_open_value_reads_a_known_value_keeps_an_unknown_one_and_writes_both_back() {
    for (text, read) in [
        ("light-rain", Open::Known(Weather::LightRain)),
        ("hail", Open::Other("hail".to_owned())),
        ("LightRain", Open::Other("LightRain".to_owned())),
    ] {
        let value: Open<Weather> = serde_json::from_value(json!(text)).unwrap();
        assert_eq!(value, read, "{text}");
        assert_eq!(serde_json::to_value(&value).unwrap(), json!(text));
    }
    assert_eq!(Open::Known(Weather::Sunny).known(), Some(&Weather::Sunny));
    assert_eq!(Open::<Weather>::Other("hail".to_owned()).known(), None);
}

/// An open value refuses what is not a string, as its enum does, and
/// reads the string twin.
#[test]
fn an_open_value_refuses_a_non_string_and_reads_its_string_twin() {
    for (refused, admitted) in [
        (json!(1), json!("1")),
        (json!(null), json!("null")),
        (json!(["sunny"]), json!("sunny")),
        (json!({"sunny": null}), json!("sunny")),
    ] {
        assert!(
            serde_json::from_value::<Open<Weather>>(refused.clone()).is_err(),
            "{refused}"
        );
        assert!(serde_json::from_value::<Open<Weather>>(admitted).is_ok());
    }
}

/// An open field's schema is its enum's, so wrapping a stored field
/// changes no committed schema.
#[test]
fn an_open_fields_schema_is_its_enums() {
    #[derive(JsonSchema)]
    struct Closed {
        /// The weather.
        weather: Weather,
        /// Yesterday's.
        before: Option<Weather>,
        /// A forecast.
        ahead: Vec<Weather>,
    }
    #[derive(JsonSchema)]
    #[schemars(rename = "Closed")]
    struct Opened {
        /// The weather.
        weather: Open<Weather>,
        /// Yesterday's.
        before: Option<Open<Weather>>,
        /// A forecast.
        ahead: Vec<Open<Weather>>,
    }
    assert_eq!(
        schema_of::<Opened>(SchemaSettings::draft2020_12()),
        schema_of::<Closed>(SchemaSettings::draft2020_12())
    );
    assert_eq!(
        schema_of::<Opened>(SchemaSettings::draft2020_12())["properties"]["weather"],
        json!({"description": "The weather.", "type": "string", "enum": ["sunny", "light-rain"]})
    );
}

/// A type name is a namespace, a kind and a version from 1, as a record's
/// `$type` names it.
#[test]
fn a_type_name_without_three_parts_or_a_version_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<TypeName>();
    let invalid = Some(board_model::Code::RecordInvalid);
    for (refused, admitted) in [
        ("changes/flow", "changes/flow/v1"),
        ("changes/flow/v0", "changes/flow/v10"),
        ("Changes/flow/v1", "review/report/v1"),
        ("/changes/flow/v1", "jira/issue/v1"),
    ] {
        assert_eq!((refused, code(json!(refused))), (refused, invalid));
        assert_eq!((admitted, code(json!(admitted))), (admitted, None));
    }
}

/// A path template is a logical path, placeholders and all, on one line.
#[test]
fn a_path_template_without_a_leading_slash_or_with_a_space_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<PathTemplate>();
    let invalid = Some(board_model::Code::RecordInvalid);
    for (refused, admitted) in [
        ("controls/{repo}/ui-paths", "/controls/{repo}/ui-paths"),
        ("/flows/{draft} .json", "/flows/{draft}.json"),
    ] {
        assert_eq!((refused, code(json!(refused))), (refused, invalid));
        assert_eq!((admitted, code(json!(admitted))), (admitted, None));
    }
}

/// A pattern is marked as a regular expression and is never empty.
#[test]
fn an_empty_pattern_is_refused_and_its_twin_is_admitted() {
    let code = refusal_of::<Pattern>();
    assert_eq!(code(json!("")), Some(board_model::Code::RecordInvalid));
    assert_eq!(code(json!("^none\\b")), None);
    let schema = schema_of::<Pattern>(SchemaSettings::draft2020_12());
    assert_eq!(schema["x-board-regex"], true);
}
