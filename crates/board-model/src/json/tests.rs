use serde_json::json;

use super::*;

fn code(bytes: &[u8]) -> Code {
    parse_json(bytes).unwrap_err().code()
}

#[test]
fn a_json_document_parses_to_its_value() {
    let value = parse_json(br#"{"b": [1, 2.5, "x", null, true], "a": {}}"#).unwrap();
    assert_eq!(value, json!({"a": {}, "b": [1, 2.5, "x", null, true]}));
}

#[test]
fn a_document_over_16_mib_is_too_large() {
    let fits = format!("\"{}\"", "a".repeat(MAX_RECORD_BYTES - 2));
    assert!(parse_json(fits.as_bytes()).is_ok());
    let over = format!("\"{}\"", "a".repeat(MAX_RECORD_BYTES - 1));
    assert_eq!(code(over.as_bytes()), Code::JsonTooLarge);
}

#[test]
fn a_document_must_be_utf8() {
    assert!(parse_json("\"é\"".as_bytes()).is_ok());
    assert_eq!(code(b"\"\xe9\""), Code::JsonNotUtf8);
}

#[test]
fn git_conflict_markers_are_named_as_a_bad_merge() {
    let clean = "{\n  \"a\": 1\n}\n";
    assert!(parse_json(clean.as_bytes()).is_ok());
    for conflicted in [
        "{\n<<<<<<< HEAD\n  \"a\": 1\n=======\n  \"a\": 2\n>>>>>>> other\n}\n",
        "{\n=======\n}\n",
        "{\n||||||| base\n}\n",
    ] {
        assert_eq!(
            code(conflicted.as_bytes()),
            Code::JsonMergeConflict,
            "{conflicted}"
        );
    }
    let error = parse_json(b"{\n<<<<<<< HEAD\n}").unwrap_err();
    assert_eq!(
        error.to_string(),
        "json holds a git conflict marker on line 2"
    );
}

#[test]
fn a_marker_inside_a_string_is_not_a_conflict() {
    assert!(parse_json(br#"{"a": "<<<<<<< not a marker"}"#).is_ok());
}

#[test]
fn text_that_is_not_one_json_document_does_not_parse() {
    assert!(parse_json(b"[1]").is_ok());
    for bad in [&b"[1"[..], b"[1] [2]", b"", b"{'a': 1}", b"[1,]"] {
        assert_eq!(
            code(bad),
            Code::JsonParse,
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn a_byte_order_mark_is_refused() {
    assert!(parse_json(b"{}").is_ok());
    assert_eq!(code(b"\xef\xbb\xbf{}"), Code::JsonParse);
}

#[test]
fn nesting_past_serde_jsons_limit_does_not_parse() {
    let fits = format!("{}{}", "[".repeat(100), "]".repeat(100));
    assert!(parse_json(fits.as_bytes()).is_ok());
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    assert_eq!(code(deep.as_bytes()), Code::JsonParse);
}

#[test]
fn a_duplicate_key_at_any_depth_is_refused() {
    assert!(parse_json(br#"{"a": 1, "b": {"c": 1, "d": [{"e": 1, "f": 2}]}}"#).is_ok());
    for duplicated in [
        &br#"{"a": 1, "a": 2}"#[..],
        br#"{"b": {"c": 1, "c": 1}}"#,
        br#"{"d": [{"e": 1, "e": 2}]}"#,
    ] {
        assert_eq!(
            code(duplicated),
            Code::JsonDuplicateKey,
            "{}",
            String::from_utf8_lossy(duplicated)
        );
    }
    let error = parse_json(br#"{"a": 1, "a": 2}"#).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"json repeats the key "a" in one object"#
    );
}

#[test]
fn a_syntax_error_wins_over_a_duplicate_key() {
    assert_eq!(code(br#"{"a": 1, "a": 2"#), Code::JsonParse);
}

#[test]
fn bolero_any_bytes_parse_to_a_value_that_parses_back_to_itself() {
    bolero::check!().for_each(|bytes: &[u8]| {
        if let Ok(value) = parse_json(bytes) {
            let again = serde_json::to_vec(&value).unwrap();
            assert_eq!(parse_json(&again).unwrap(), value);
        }
    });
}

#[test]
fn a_number_parses_back_to_the_same_value_after_printing() {
    // Found by fuzzing: an integer too large for u64 parses as a float, and
    // serde_json's default float parse is not exactly rounded, so this read
    // back as 3.3032288278647182e22 instead of 3.3032288278647186e22.
    for number in [
        "33032288278647188139631",
        "0.1",
        "1e308",
        "5e-324",
        "123456789012345678",
    ] {
        let value = parse_json(number.as_bytes()).unwrap();
        let printed = serde_json::to_vec(&value).unwrap();
        assert_eq!(parse_json(&printed).unwrap(), value, "{number}");
    }
}
