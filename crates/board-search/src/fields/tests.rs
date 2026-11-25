use super::{FIELDS, TYPES, check, short_type, unknown_person};
use crate::error::ErrorCode;
use crate::parse::parse;
use crate::query::{Field, Span, Value};

#[test]
fn each_short_type_but_doc_names_a_prefix_that_ends_at_a_version_segment() {
    for short in TYPES {
        assert_eq!(short_type(short.name), Some(*short));
        let Some(prefix) = short.prefix else {
            assert_eq!(short.name, "doc");
            continue;
        };
        let segments: Vec<&str> = prefix.split('/').collect();
        assert_eq!(segments.len(), 3, "{prefix}");
        assert_eq!(segments[2], "", "{prefix}");
    }
    assert_eq!(TYPES.len(), 11);
}

#[test]
fn a_short_type_name_is_read_in_any_case() {
    assert_eq!(
        short_type("Question").map(|short| short.prefix),
        Some(Some("plans/question/"))
    );
    assert_eq!(short_type("DOC").map(|short| short.prefix), Some(None));
    assert_eq!(short_type("questions"), None);
}

#[test]
fn the_field_list_offers_every_short_type_and_me() {
    assert_eq!(FIELDS[0].field, Field::Type);
    let names: Vec<&str> = TYPES.iter().map(|short| short.name).collect();
    assert_eq!(FIELDS[0].values, names.as_slice());
    assert_eq!(FIELDS[1].field, Field::From);
    assert_eq!(FIELDS[1].values, &["me"]);
    assert_eq!(FIELDS[2].field, Field::Path);
    assert_eq!(FIELDS[2].values.len(), 0);
}

#[test]
fn a_path_that_does_not_start_with_a_slash_is_an_error_at_its_value() {
    let query = parse("budget path:tasks,/plans").unwrap();
    let errors = check(&query);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, ErrorCode::RelativePath);
    assert_eq!(errors[0].span, Span::new(12, 17));
    assert_eq!(
        errors[0].message,
        "path:tasks needs a board path that starts with /, such as path:/tasks."
    );
    assert_eq!(check(&parse("budget path:/tasks,/plans").unwrap()), []);
}

#[test]
fn an_unknown_type_is_an_error_at_its_value_listing_the_short_names() {
    let query = parse("budget type:task,questoin -type:run").unwrap();
    let errors = check(&query);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, ErrorCode::UnknownType);
    assert_eq!(errors[0].span, Span::new(17, 25));
    assert_eq!(
        errors[0].message,
        "There is no type called questoin; use one of task, question, answer, decision, change, plan, eval, run, artifact, rule, doc."
    );
}

#[test]
fn from_me_without_an_owner_says_where_to_name_one() {
    let value = Value {
        text: "me".to_owned(),
        span: Span::new(5, 7),
    };
    let error = unknown_person(&value);
    assert_eq!(error.code, ErrorCode::UnknownPerson);
    assert_eq!(error.span, Span::new(5, 7));
    assert!(error.message.contains("\"owner\""), "{}", error.message);
}
