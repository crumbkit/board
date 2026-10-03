use super::*;

fn classified(path: &str, text: &str) -> (Kind, Option<Code>) {
    let classified = classify(path, Some(text.as_bytes()));
    (classified.kind, classified.code)
}

#[test]
fn a_record_type_is_namespace_kind_and_version() {
    for admitted in ["lr/task/v1", "a-1/b-2/v10", "board/config/v1"] {
        assert_eq!(admitted.parse::<RecordType>().unwrap().as_str(), admitted);
    }
    for refused in [
        "lr/task",
        "lr/task/1",
        "lr/task/v0",
        "lr/task/v01",
        "LR/task/v1",
        "lr/ta_sk/v1",
        "lr//v1",
        "/task/v1",
        "lr/task/v1/x",
        "lr/task/v",
        "lr/task/v1a",
    ] {
        let error = refused.parse::<RecordType>().unwrap_err();
        assert_eq!(error.code(), Code::RecordBadType, "{refused}");
    }
}

#[test]
fn a_record_type_names_its_schema_path() {
    let record_type: RecordType = "lr/task/v2".parse().unwrap();
    assert_eq!(
        record_type.schema_path(),
        ".board/types/lr/task/v2.schema.json"
    );
    assert!(!record_type.is_board());
    assert!("board/mount/v1".parse::<RecordType>().unwrap().is_board());
}

#[test]
fn a_record_type_deserializes_under_the_same_rules() {
    let admitted: RecordType = serde_json::from_str(r#""lr/task/v1""#).unwrap();
    assert_eq!(admitted.as_str(), "lr/task/v1");
    assert!(serde_json::from_str::<RecordType>(r#""lr/task""#).is_err());
}

#[test]
fn only_json_files_are_read() {
    assert!(needs_bytes("work/x.json"));
    assert!(!needs_bytes("notes.md"));
    assert!(!needs_bytes("x.JSON"));
    assert_eq!(classify("notes.md", None), Classified::of(Kind::File));
}

#[test]
fn a_json_object_with_a_type_string_is_a_record() {
    let record = classify("x.json", Some(br#"{"$type": "lr/task/v1", "title": "t"}"#));
    assert_eq!((record.kind, record.code), (Kind::Record, None));
    assert_eq!(record.record_type.unwrap().as_str(), "lr/task/v1");
    assert_eq!(
        classified("x.json", r#"{"title": "t"}"#),
        (Kind::Json, None)
    );
    assert_eq!(
        classified("x.json", r#"[{"$type": "lr/task/v1"}]"#),
        (Kind::Json, None)
    );
}

#[test]
fn a_type_that_is_not_a_well_formed_string_is_a_bad_type() {
    assert_eq!(
        classified("x.json", r#"{"$type": "lr/task/v1"}"#),
        (Kind::Record, None)
    );
    for bad in [
        r#"{"$type": 1}"#,
        r#"{"$type": "task"}"#,
        r#"{"$type": null}"#,
    ] {
        assert_eq!(
            classified("x.json", bad),
            (Kind::Record, Some(Code::RecordBadType)),
            "{bad}"
        );
    }
}

#[test]
fn a_board_type_board_does_not_define_is_unknown() {
    assert_eq!(
        classified("x.json", r#"{"$type": "board/config/v1"}"#),
        (Kind::Record, None)
    );
    assert_eq!(
        classified("x.json", r#"{"$type": "board/widget/v1"}"#),
        (Kind::Record, Some(Code::RecordUnknownType))
    );
}

#[test]
fn a_pointer_record_is_a_pointer_and_an_invalid_one_is_degraded() {
    let link = classify(
        "l.json",
        Some(br#"{"$type": "board/mount/v1", "path": "/x"}"#),
    );
    assert_eq!((link.kind, link.code), (Kind::Pointer, None));
    assert_eq!(link.pointer, Some(Pointer::Link("/x".parse().unwrap())));
    assert_eq!(
        classified(
            "l.json",
            r#"{"$type": "board/mount/v1", "path": "/x", "extra": 1}"#
        ),
        (Kind::Pointer, Some(Code::MountInvalid))
    );
}

#[test]
fn a_schema_is_a_json_file_under_the_types_directory() {
    let schema = r#"{"type": "object"}"#;
    assert_eq!(
        classified(".board/types/lr/task/v1.schema.json", schema),
        (Kind::Schema, None)
    );
    assert_eq!(
        classified("types/lr/task/v1.schema.json", schema),
        (Kind::Json, None)
    );
    assert_eq!(
        classified(".board/types/lr/task/v1.json", schema),
        (Kind::Json, None)
    );
}

#[test]
fn a_schema_for_the_reserved_board_namespace_is_invalid() {
    let schema = r#"{"type": "object"}"#;
    assert_eq!(
        classified(".board/types/lr/mount/v1.schema.json", schema),
        (Kind::Schema, None)
    );
    assert_eq!(
        classified(".board/types/board/mount/v1.schema.json", schema),
        (Kind::Schema, Some(Code::SchemaInvalid))
    );
}

#[test]
fn a_file_that_does_not_parse_keeps_its_kind_and_takes_the_parse_code() {
    assert_eq!(
        classified("x.json", "{"),
        (Kind::Json, Some(Code::JsonParse))
    );
    assert_eq!(
        classified(".board/types/lr/t/v1.schema.json", r#"{"a": 1, "a": 1}"#),
        (Kind::Schema, Some(Code::JsonDuplicateKey))
    );
    let oversized = classify("x.json", None);
    assert_eq!(
        (oversized.kind, oversized.code),
        (Kind::Json, Some(Code::JsonTooLarge))
    );
}

#[test]
fn the_board_configuration_is_a_record() {
    assert_eq!(
        classified(
            ".board/board.json",
            r#"{"$type": "board/config/v1", "plugins": []}"#
        ),
        (Kind::Record, None)
    );
}

#[test]
fn bolero_classifying_any_bytes_never_panics_and_a_record_has_a_type() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let classified = classify("x.json", Some(bytes));
        if classified.kind == Kind::Record && classified.code != Some(Code::RecordBadType) {
            assert!(classified.record_type.is_some());
        }
        if classified.code.is_none() {
            assert!(classified.value.is_some());
        }
    });
}
