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
        matches!(error, RecordError::MalformedReference { ref pointer } if pointer == "/parent")
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
