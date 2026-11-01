//! The index stores what `Role::classify` and `pointer()` answer, by blob
//! and by tree, so an index of an older format can hold answers they no
//! longer give.

use std::fmt::Write as _;

use board_model::{Role, pointer};

use super::*;

/// Each sample's name and the bytes of a JSON file.
const SAMPLES: [(&str, &str); 14] = [
    ("link", r#"{"$type": "board/mount/v1", "path": "/x"}"#),
    (
        "relative link",
        r#"{"$type": "board/mount/v1", "path": "x"}"#,
    ),
    ("bare pointer", r#"{"$type": "board/mount/v1"}"#),
    ("record", r#"{"$type": "lr/task/v1"}"#),
    ("bad type", r#"{"$type": "not a type"}"#),
    ("number type", r#"{"$type": 7}"#),
    ("untyped", r#"{"title": "t"}"#),
    (
        "duplicate",
        r#"{"$type": "lr/task/v1", "$type": "lr/task/v1"}"#,
    ),
    ("array", r#"["board/mount/v1"]"#),
    ("string", r#""board/mount/v1""#),
    ("not json", "not json"),
    ("bom", "\u{feff}{}"),
    ("commented", "{\"a\": 1, // a\n}"),
    ("commented record", "{\"$type\": \"lr/task/v1\", // a\n}"),
];

/// What the index stores for each sample: the kind and code
/// `Role::classify` gives a JSON file, and what `pointer()` answers.
const STORED: &str = "\
link: pointer - | pointer
relative link: pointer board::mount::invalid | board::mount::invalid
bare pointer: pointer board::mount::invalid | board::mount::invalid
record: record - | none
bad type: record board::record::bad_type | none
number type: record board::record::bad_type | none
untyped: json - | none
duplicate: json board::json::duplicate_key | none
array: json - | none
string: json - | none
not json: json board::json::parse | none
bom: json board::json::parse | none
commented: json - | none
commented record: json board::json::parse | none
";

#[test]
fn what_the_index_stores_for_a_json_file_changes_only_with_its_format() {
    let mut answers = String::new();
    for (name, sample) in SAMPLES {
        let classified = Role::Json.classify(Some(sample.as_bytes()));
        let code = classified
            .code
            .map_or("-".to_owned(), |code| code.to_string());
        let kind = format!("{:?}", classified.kind()).to_lowercase();
        let found = match pointer(sample.as_bytes()) {
            Ok(Some(_)) => "pointer".to_owned(),
            Ok(None) => "none".to_owned(),
            Err(error) => error.code().to_string(),
        };
        writeln!(answers, "{name}: {kind} {code} | {found}").unwrap();
    }
    assert_eq!(
        (answers.as_str(), INDEX_VERSION),
        (STORED, 22),
        "what classify or pointer() answers changed: an index of the current \
         format may hold the old answers, so bump INDEX_VERSION, then update \
         STORED and the version here"
    );
}
