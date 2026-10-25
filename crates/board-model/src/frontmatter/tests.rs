use serde_json::json;

use super::*;

/// The frontmatter of a file whose block is `yaml` and a last line that
/// claims a type, without that type.
fn read(yaml: &str) -> Result<Option<Value>, FrontmatterError> {
    let mut value = frontmatter(format!("---\n{yaml}$type: lr/t/v1\n---\n# Body\n").as_bytes())?;
    if let Some(Value::Object(members)) = &mut value {
        members.remove("$type");
    }
    Ok(value)
}

fn refused(yaml: &str) -> FrontmatterError {
    read(yaml).unwrap_err()
}

#[test]
fn a_block_opens_on_the_first_line_and_closes_at_the_next_dashes_or_dots() {
    assert_eq!(split("---\na: 1\n---\nbody"), Some(("a: 1\n", "body")));
    assert_eq!(
        split("---\r\na: 1\r\n...\r\nbody"),
        Some(("a: 1\r\n", "body"))
    );
    assert_eq!(split("---\n---\n"), Some(("", "")));
    assert_eq!(split("\u{feff}---\na: 1\n---\n"), Some(("a: 1\n", "")));
    for none in [
        "a: 1\n---\n",
        " ---\na: 1\n---\n",
        "---\na: 1\n",
        "--- \na: 1\n---\n",
        "",
    ] {
        assert_eq!(split(none), None, "{none:?}");
    }
}

#[test]
fn a_file_without_a_block_or_not_utf8_has_no_frontmatter() {
    assert_eq!(frontmatter(b"# Title\n\n---\n"), Ok(None));
    assert_eq!(frontmatter(b"---\n\xff: 1\n---\n"), Ok(None));
    assert_eq!(frontmatter(b"---\n$type: a\n"), Ok(None));
}

#[test]
fn a_block_without_a_type_line_is_left_to_its_writer_and_its_typed_twin_is_read() {
    // A notes app's frontmatter, which need not be YAML Board reads.
    let notes = "---\ntitle: [draft\ntags: a: b\n---\n[c](c.md)\n";
    assert_eq!(frontmatter(notes.as_bytes()), Ok(None));
    assert_eq!(body(notes), notes);
    let typed = "---\n$type: lr/t/v1\ntitle: [draft\n---\n[c](c.md)\n";
    assert_eq!(
        frontmatter(typed.as_bytes()).unwrap_err().code(),
        Code::FrontmatterParse
    );
    assert_eq!(body(typed), "[c](c.md)\n");
    // A thematic break that opens the file, with prose after it.
    let prose = "---\nSome prose: with a colon.\n\n---\n\nMore prose.\n";
    assert_eq!(frontmatter(prose.as_bytes()), Ok(None));
    assert_eq!(body(prose), prose);
}

#[test]
fn a_leading_byte_order_mark_is_ignored_and_its_twin_without_one_reads_the_same() {
    let bare = "---\n$type: lr/t/v1\n---\n[c](c.md)";
    let marked = format!("\u{feff}{bare}");
    assert_eq!(
        frontmatter(marked.as_bytes()),
        Ok(Some(json!({"$type": "lr/t/v1"})))
    );
    assert_eq!(frontmatter(marked.as_bytes()), frontmatter(bare.as_bytes()));
    assert_eq!(body(&marked), "[c](c.md)");
}

#[test]
fn the_body_is_the_text_after_a_typed_block() {
    assert_eq!(
        body("---\n$type: a\nb: [b](b.md)\n---\n[c](c.md)"),
        "[c](c.md)"
    );
    assert_eq!(body("[c](c.md)"), "[c](c.md)");
}

#[test]
fn mappings_and_sequences_become_objects_and_arrays() {
    let file = "---\n$type: lr/task/v1\ntags: [a, b]\nnested:\n  - x: 1\n---\n";
    let value = frontmatter(file.as_bytes()).unwrap();
    assert_eq!(
        value,
        Some(json!({"$type": "lr/task/v1", "tags": ["a", "b"], "nested": [{"x": 1}]}))
    );
}

#[test]
fn plain_scalars_follow_the_yaml_1_2_core_schema() {
    let value = read(
        "n: ~\nempty:\nt: true\nf: FALSE\ni: -12\no: 0o17\nh: 0xff\nx: 1.5e3\n\
         big: 18446744073709551615\nword: yes\noff: off\nzero: 012\n",
    )
    .unwrap()
    .unwrap();
    assert_eq!(value["n"], Value::Null);
    assert_eq!(value["empty"], Value::Null);
    assert_eq!(value["t"], true);
    assert_eq!(value["f"], false);
    assert_eq!(value["i"], -12);
    assert_eq!(value["o"], 15);
    assert_eq!(value["h"], 255);
    assert_eq!(value["x"], 1500.0);
    assert_eq!(value["big"], u64::MAX);
    // YAML 1.1 booleans are strings under 1.2, and a leading zero is decimal.
    assert_eq!(value["word"], "yes");
    assert_eq!(value["off"], "off");
    assert_eq!(value["zero"], 12);
}

#[test]
fn quoted_and_block_scalars_are_strings() {
    let value = read("a: \"1\"\nb: 'true'\nc: |\n  two\n  lines\n")
        .unwrap()
        .unwrap();
    assert_eq!(value, json!({"a": "1", "b": "true", "c": "two\nlines\n"}));
}

#[test]
fn a_date_stays_the_string_it_was_written_as() {
    let value = read("due: 2026-10-26\nat: 2026-10-26T09:00:00Z\n")
        .unwrap()
        .unwrap();
    assert_eq!(
        value,
        json!({"due": "2026-10-26", "at": "2026-10-26T09:00:00Z"})
    );
}

#[test]
fn yaml_that_does_not_parse_names_its_file_line() {
    let FrontmatterError::Parse { line, .. } = refused("a: 1\nb: [\n") else {
        panic!("not a parse error");
    };
    assert_eq!(line, 4);
    assert_eq!(refused("a: 1\nb: [\n").code(), Code::FrontmatterParse);
}

#[test]
fn a_repeated_key_is_refused_and_the_same_key_in_another_mapping_is_admitted() {
    assert_eq!(
        refused("a: 1\nb: 2\na: 3\n"),
        FrontmatterError::DuplicateKey {
            line: 4,
            key: "a".to_owned()
        }
    );
    assert_eq!(refused("a: 1\na: 1\n").code(), Code::FrontmatterParse);
    assert_eq!(
        read("a: {x: 1}\nb: {x: 2}\n").unwrap(),
        Some(json!({"a": {"x": 1}, "b": {"x": 2}}))
    );
}

#[test]
fn yaml_that_json_cannot_hold_is_refused_and_its_json_form_is_admitted() {
    let cases = [
        ("a: &x 1\nb: *x\n", Feature::Anchor, "a: 1\nb: 1\n"),
        (
            "a: !!str 1\n",
            Feature::Tag("tag:yaml.org,2002:str".to_owned()),
            "a: '1'\n",
        ),
        ("1: a\n", Feature::Key, "'1': a\n"),
        ("? [a]\n: b\n", Feature::Key, "a: b\n"),
        (
            "a: .inf\n",
            Feature::Number(".inf".to_owned()),
            "a: '.inf'\n",
        ),
        (
            "a: -.inf\n",
            Feature::Number("-.inf".to_owned()),
            "a: '-.inf'\n",
        ),
        (
            "a: .nan\n",
            Feature::Number(".nan".to_owned()),
            "a: '.nan'\n",
        ),
        (
            "a: 18446744073709551616\n",
            Feature::Number("18446744073709551616".to_owned()),
            "a: '18446744073709551616'\n",
        ),
        (
            "a: 0.1000000000000000055511\n",
            Feature::Number("0.1000000000000000055511".to_owned()),
            "a: 0.1\n",
        ),
    ];
    for (yaml, feature, admitted) in cases {
        let error = refused(yaml);
        assert_eq!(error.code(), Code::FrontmatterUnsupported, "{yaml}");
        let FrontmatterError::Unsupported { feature: found, .. } = error else {
            panic!("{yaml} gave {error:?}");
        };
        assert_eq!(found, feature, "{yaml}");
        assert!(read(admitted).is_ok(), "{admitted}");
    }
}

#[test]
fn an_alias_is_refused_even_when_its_anchor_is_not_reached_first_and_its_plain_twin_is_admitted() {
    // The parser reports an undefined alias as an error of its own.
    assert_eq!(refused("a: *x\n").code(), Code::FrontmatterParse);
    assert_eq!(read("a: x\n").unwrap(), Some(json!({"a": "x"})));
}

#[test]
fn a_second_document_is_refused_and_the_same_text_without_the_marker_is_admitted() {
    // The block's own closing line is not a second document, but a marker
    // with a trailing space inside the block is one.
    assert_eq!(read("a: 1\nb: 2\n").unwrap(), Some(json!({"a": 1, "b": 2})));
    let yaml = "a: 1\n--- \nb: 2\n";
    assert_eq!(
        refused(yaml),
        FrontmatterError::Unsupported {
            line: 3,
            feature: Feature::Documents
        }
    );
}

#[test]
fn nesting_deeper_than_json_allows_is_refused_and_at_the_limit_is_admitted() {
    let deep = |depth: usize| format!("a: {}{}\n", "[".repeat(depth), "]".repeat(depth));
    // The block's mapping and its arrays together nest `MAX_DEPTH` deep.
    let admitted = read(&deep(MAX_DEPTH - 1)).unwrap().unwrap();
    let text = serde_json::to_string(&admitted).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), admitted);
    assert!(matches!(
        refused(&deep(MAX_DEPTH)),
        FrontmatterError::Unsupported {
            feature: Feature::Deep,
            ..
        }
    ));
}

#[test]
fn bolero_any_bytes_give_frontmatter_or_an_error_with_its_code() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let mut file = b"---\n$type: lr/t/v1\n".to_vec();
        file.extend_from_slice(bytes);
        file.extend_from_slice(b"\n---\n");
        match frontmatter(&file) {
            Ok(Some(value)) => {
                let text = serde_json::to_string(&value).unwrap();
                assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
            }
            Ok(None) => {}
            Err(error) => {
                assert!(matches!(
                    error.code(),
                    Code::FrontmatterParse | Code::FrontmatterUnsupported
                ));
            }
        }
    });
}
