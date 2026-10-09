use serde_json::json;

use super::*;

fn compile_code(schema: Value) -> Code {
    Schema::compile(schema).unwrap_err().code()
}

fn task_schema() -> Schema {
    Schema::compile(json!({
        "type": "object",
        "required": ["title"],
        "properties": {
            "title": {"type": "string"},
            "parent": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"}
        }
    }))
    .unwrap()
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

#[test]
fn a_schema_must_be_json_schema_2020_12() {
    assert!(Schema::compile(json!({"$schema": DRAFT_2020_12, "type": "object"})).is_ok());
    assert!(Schema::compile(json!({"type": "object"})).is_ok());
    let older = json!({"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"});
    assert_eq!(compile_code(older), Code::SchemaInvalid);
}

#[test]
fn a_schema_that_does_not_compile_is_invalid() {
    assert!(Schema::compile(json!({"type": "string"})).is_ok());
    assert_eq!(compile_code(json!({"type": 5})), Code::SchemaInvalid);
}

#[test]
fn x_board_type_must_name_record_types() {
    assert!(Schema::compile(json!({"x-board-type": "lr/task/v1"})).is_ok());
    assert!(
        Schema::compile(json!({"properties": {"a": {"x-board-type": ["lr/a/v1", "lr/b/v2"]}}}))
            .is_ok()
    );
    for bad in [json!("task"), json!(["lr/a/v1", 1]), json!(7)] {
        let schema = json!({"properties": {"a": {"x-board-type": bad}}});
        assert_eq!(compile_code(schema), Code::SchemaInvalid, "{bad}");
    }
}

#[test]
fn a_schema_never_fetches_a_remote_ref() {
    let local = json!({"$defs": {"t": {"type": "string"}}, "$ref": "#/$defs/t"});
    assert!(Schema::compile(local).is_ok());
    for remote in [
        json!({"$ref": "https://example.com/schema.json"}),
        json!({"$ref": "file:///etc/schema.json"}),
    ] {
        assert_eq!(
            compile_code(remote.clone()),
            Code::SchemaInvalid,
            "{remote}"
        );
    }
}

#[test]
fn patterns_run_on_a_linear_time_engine() {
    let linear = json!({"type": "string", "pattern": "^(a+)+$"});
    let schema = Schema::compile(linear).unwrap();
    // A backtracking engine takes exponential time on this; a linear one
    // answers at once.
    let attack = format!("{}b", "a".repeat(10_000));
    assert!(schema.validate(&json!(attack)).is_err());
    // Look-around needs backtracking, which the linear engine refuses.
    assert_eq!(
        compile_code(json!({"pattern": "^(?=a)a$"})),
        Code::SchemaInvalid
    );
}

#[test]
fn a_record_that_fails_its_schema_names_where() {
    let schema = task_schema();
    assert!(schema.validate(&json!({"title": "t"})).is_ok());
    let error = schema.validate(&json!({"title": 1})).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
    assert!(matches!(error, RecordError::Invalid { ref pointer, .. } if pointer == "/title"));
    assert_eq!(
        error.fields(),
        Fields {
            pointer: Some("/title".to_owned()),
            ..Fields::default()
        }
    );
}

#[test]
fn a_failure_says_why_in_at_most_200_characters_and_counts_every_failure() {
    let schema = task_schema();
    let valid = json!({"title": "t"});
    assert!(schema.validate(&valid).is_ok());
    assert_eq!(schema.failures(&valid), 0);
    let wrong = json!({"title": 5});
    assert_eq!(
        schema.validate(&wrong).unwrap_err().reason(),
        r#"fails its schema at "/title": 5 is not of type "string""#
    );
    assert_eq!(schema.failures(&wrong), 1);
    let long = json!({"title": ["x".repeat(300)]});
    let reason = schema.validate(&long).unwrap_err().reason();
    assert_eq!(reason.chars().count(), 200);
    assert!(reason.ends_with('…'), "{reason}");
    assert_eq!(schema.failures(&json!({"parent": 1})), 2);
    let malformed = json!({"title": "t", "parent": "projects/p"});
    assert_eq!(
        schema.validate(&malformed).unwrap_err().reason(),
        r#"holds a reference at "/parent": "projects/p" is not a logical path"#
    );
}

#[test]
fn a_failure_reads_as_a_predicate_for_its_caller_to_give_a_subject() {
    let schema = task_schema();
    assert!(
        schema
            .validate(&json!({"title": "t", "parent": "/p"}))
            .is_ok()
    );
    for (record, message) in [
        (
            json!({"title": 5}),
            r#"fails its schema at "/title": 5 is not of type "string""#,
        ),
        (
            json!({}),
            r#"fails its schema at its root: "title" is a required property"#,
        ),
        (
            json!({"title": "t", "parent": "projects/p"}),
            r#"holds a reference at "/parent": "projects/p" is not a logical path"#,
        ),
    ] {
        assert_eq!(
            schema.validate(&record).unwrap_err().to_string(),
            message,
            "{record}"
        );
    }
}

#[test]
fn a_string_that_fails_another_format_is_invalid_and_not_a_malformed_reference() {
    let schema = Schema::compile(json!({
        "type": "object",
        "properties": {"due": {"type": "string", "format": "date"}},
    }))
    .unwrap();
    assert!(schema.validate(&json!({"due": "2026-01-01"})).is_ok());
    let error = schema.validate(&json!({"due": "2026-13-01"})).unwrap_err();
    assert_eq!(error.code(), Code::RecordInvalid);
}

#[test]
fn the_first_failure_in_sorted_order_is_reported() {
    let schema = Schema::compile(json!({
        "properties": {"a": {"type": "string"}, "b": {"type": "string"}, "c": {"type": "string"}}
    }))
    .unwrap();
    for _ in 0..3 {
        let error = schema
            .validate(&json!({"c": 1, "a": 1, "b": 1}))
            .unwrap_err();
        assert!(matches!(error, RecordError::Invalid { ref pointer, .. } if pointer == "/a"));
    }
}

#[test]
fn a_reference_that_is_not_a_logical_path_is_malformed() {
    let schema = task_schema();
    assert!(
        schema
            .validate(&json!({"title": "t", "parent": "/projects/p"}))
            .is_ok()
    );
    let error = schema
        .validate(&json!({"title": "t", "parent": "projects/p"}))
        .unwrap_err();
    assert_eq!(error.code(), Code::RefMalformed);
    assert!(
        matches!(error, RecordError::MalformedReference { ref pointer, .. } if pointer == "/parent")
    );
    assert_eq!(
        error.fields(),
        Fields {
            pointer: Some("/parent".to_owned()),
            ..Fields::default()
        }
    );
}

#[test]
fn references_carry_their_pointer_path_and_accepted_types() {
    let schema = task_schema();
    let references = schema.references(&json!({"title": "t", "parent": "/projects/p"}));
    assert_eq!(
        references,
        [Reference {
            pointer: "/parent".to_owned(),
            path: path("/projects/p"),
            accepts: Some(vec!["lr/project/v1".parse().unwrap()]),
        }]
    );
    assert!(schema.references(&json!({"title": "t"})).is_empty());
}

#[test]
fn references_are_found_through_the_searched_keywords_in_document_order() {
    let reference = json!({"type": "string", "format": "board-ref"});
    let schema = Schema::compile(json!({
        "$defs": {"ref": reference},
        "properties": {
            "direct": reference,
            "list": {"items": {"$ref": "#/$defs/ref"}},
            "pair": {"prefixItems": [reference, {"type": "string"}]},
            "map": {"patternProperties": {"^r-": reference}, "additionalProperties": {"$ref": "#/$defs/ref"}},
            "both": {"allOf": [reference]}
        }
    }))
    .unwrap();
    let record = json!({
        "direct": "/d",
        "list": ["/l0", "/l1", "/l2", "/l3", "/l4", "/l5", "/l6", "/l7", "/l8", "/l9", "/l10"],
        "pair": ["/p", "/not-a-reference"],
        "map": {"r-x": "/rx", "other": "/o"},
        "both": "/b"
    });
    assert!(schema.validate(&record).is_ok());
    let found: Vec<String> = schema
        .references(&record)
        .into_iter()
        .map(|reference| reference.pointer)
        .collect();
    let mut expected = vec!["/both".to_owned(), "/direct".to_owned()];
    expected.extend((0..=10).map(|index| format!("/list/{index}")));
    expected.extend(["/map/other", "/map/r-x", "/pair/0"].map(str::to_owned));
    assert_eq!(found, expected);
}

#[test]
fn branches_of_any_of_one_of_and_if_are_not_searched() {
    let reference = json!({"type": "string", "format": "board-ref"});
    let schema = Schema::compile(json!({
        "properties": {
            "any": {"anyOf": [reference]},
            "one": {"oneOf": [reference]},
            "cond": {"if": {"type": "string"}, "then": reference}
        }
    }))
    .unwrap();
    let record = json!({"any": "/a", "one": "/o", "cond": "/c"});
    assert!(schema.validate(&record).is_ok());
    assert!(schema.references(&record).is_empty());
}

#[test]
fn a_ref_cycle_in_a_schema_ends() {
    let schema = Schema::compile(json!({
        "$defs": {"loop": {"allOf": [{"$ref": "#/$defs/loop"}]}},
        "$ref": "#/$defs/loop"
    }))
    .unwrap();
    assert!(schema.references(&json!({"a": "/x"})).is_empty());
}

#[test]
fn a_schema_whose_branches_multiply_without_bound_is_refused_at_compile() {
    let exploding = json!({"allOf": [{"$ref": "#"}, {"$ref": "#"}]});
    assert_eq!(compile_code(exploding), Code::SchemaInvalid);
    let nested = json!({"properties": {"a": {"allOf": [{"$ref": "#/properties/a"}, {"$ref": "#/properties/a"}]}}});
    assert_eq!(compile_code(nested), Code::SchemaInvalid);
}

#[test]
fn a_deep_but_linear_schema_is_admitted() {
    // A chain of 30 `$defs`, each a `$ref` to the next, ending in a reference.
    let mut defs = serde_json::Map::new();
    for index in 0..30 {
        defs.insert(
            format!("d{index}"),
            json!({"$ref": format!("#/$defs/d{}", index + 1)}),
        );
    }
    defs.insert(
        "d30".to_owned(),
        json!({"type": "string", "format": "board-ref"}),
    );
    let chain =
        Schema::compile(json!({"$defs": defs, "properties": {"r": {"$ref": "#/$defs/d0"}}}))
            .unwrap();
    let record = json!({"r": "/x"});
    assert!(chain.validate(&record).is_ok());
    assert_eq!(chain.references(&record).len(), 1);
    // Wide but flat: 200 branches, each a leaf.
    let wide: Vec<Value> = (0..200).map(|_| json!({"type": "object"})).collect();
    assert!(Schema::compile(json!({"allOf": wide})).is_ok());
}

/// A schema whose `$defs` chain `depth` levels deep, each level applying the
/// next through `level`.
fn nested(depth: usize, level: Level) -> Value {
    let mut defs = serde_json::Map::new();
    for index in 0..depth {
        defs.insert(
            format!("d{index}"),
            level(json!({"$ref": format!("#/$defs/d{}", index + 1)})),
        );
    }
    defs.insert(format!("d{depth}"), json!({"type": "string"}));
    json!({"$defs": defs, "properties": {"r": {"$ref": "#/$defs/d0"}}})
}

/// Turns a `$ref` to the next level into one level's keywords.
type Level = fn(Value) -> Value;

const LEVELS: [(&str, Level); 6] = [
    ("oneOf", |next| json!({"oneOf": [next, next]})),
    ("anyOf", |next| json!({"anyOf": [next, next]})),
    (
        "if/then/else",
        |next| json!({"if": next, "then": next, "else": next}),
    ),
    (
        "not",
        |next| json!({"allOf": [{"not": next}, {"not": {"not": next}}]}),
    ),
    (
        "dependentSchemas",
        |next| json!({"dependentSchemas": {"a": next, "b": next}}),
    ),
    ("$dynamicRef", |next| {
        let target = next["$ref"].clone();
        json!({"anyOf": [{"$dynamicRef": target}, next]})
    }),
];

#[test]
fn branches_multiplying_through_any_in_place_keyword_are_refused_and_a_shallow_twin_admitted() {
    for (keyword, level) in LEVELS {
        assert_eq!(
            compile_code(nested(16, level)),
            Code::SchemaInvalid,
            "{keyword}"
        );
        assert!(Schema::compile(nested(3, level)).is_ok(), "{keyword}");
    }
}

#[test]
fn refusing_an_exploding_schema_stops_after_the_bound_at_each_start() {
    for (keyword, level) in LEVELS {
        let schema = nested(22, level);
        let mut count = 0;
        assert!(
            !references::within_expansion(&schema, &schema["$defs"]["d0"], 0, &mut count),
            "{keyword}"
        );
        assert_eq!(count, references::MAX_EXPANSION + 1, "{keyword}");
        let schema = nested(3, level);
        let mut count = 0;
        assert!(references::within_expansion(
            &schema,
            &schema["$defs"]["d0"],
            0,
            &mut count
        ));
        assert!(count <= references::MAX_EXPANSION, "{keyword}: {count}");
    }
}

#[test]
fn a_reference_that_is_not_a_json_pointer_is_refused_and_a_pointer_twin_admitted() {
    let anchored = json!({
        "$defs": {"leaf": {"$anchor": "leaf", "type": "string"}},
        "properties": {"r": {"$ref": "#leaf"}}
    });
    assert_eq!(compile_code(anchored), Code::SchemaInvalid);
    let pointed = json!({
        "$defs": {"leaf": {"$anchor": "leaf", "type": "string"}},
        "properties": {"r": {"$ref": "#/$defs/leaf"}}
    });
    assert!(Schema::compile(pointed).is_ok());
}

/// [`nested`], reached from `r` through `hops` single-`$ref` steps.
fn behind_hops(depth: usize, hops: usize) -> Value {
    let mut schema = nested(depth, LEVELS[0].1);
    for index in 0..hops {
        let next = if index + 1 == hops {
            "#/$defs/d0".to_owned()
        } else {
            format!("#/$defs/h{}", index + 1)
        };
        schema["$defs"][format!("h{index}")] = json!({"$ref": next});
    }
    schema["properties"]["r"] = json!({"$ref": "#/$defs/h0"});
    schema
}

#[test]
fn a_ladder_past_the_ref_limit_is_still_refused_from_its_own_start_and_a_shallow_twin_admitted() {
    let hops = 33;
    let deep = behind_hops(22, hops);
    // From `r`, the search gives up following `$ref`s before the ladder, so
    // only starting at every subschema finds it.
    let mut count = 0;
    assert!(references::within_expansion(
        &deep,
        &deep["properties"]["r"],
        0,
        &mut count
    ));
    assert_eq!(compile_code(deep), Code::SchemaInvalid);
    assert!(Schema::compile(behind_hops(3, hops)).is_ok());
}

#[test]
fn a_key_a_pattern_matches_only_under_rust_rules_is_no_reference_and_its_ascii_twin_is() {
    let schema = Schema::compile(json!({
        "patternProperties": {"^\\d+$": {"type": "string", "format": "board-ref"}}
    }))
    .unwrap();
    for (key, marked) in [("3", true), ("٣", false)] {
        let pointers: Vec<String> = schema
            .references(&json!({key: "/x"}))
            .into_iter()
            .map(|reference| reference.pointer)
            .collect();
        let expected: Vec<String> = if marked {
            vec![format!("/{key}")]
        } else {
            Vec::new()
        };
        assert_eq!(pointers, expected, "{key}");
        // The validator agrees: only a marked key's value must be a path.
        let checked = schema.validate(&json!({key: "not a path!"})).is_err();
        assert_eq!(checked, marked, "{key}");
    }
}
