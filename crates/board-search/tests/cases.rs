//! Plan test: Board's parser passes every case of the shared case file,
//! with the same parts, canonical text, error codes and spans, and the
//! canonical text of each case parses back to the same parts.
#![cfg(test)]

use board_search::{Part, QueryError, Span, parse};
use serde::Deserialize;
use serde_json::Value;

/// Board's copy of Flare's case file, which tests/pin.rs pins.
const CASES: &str = include_str!("../cases/search-cases.json");

#[derive(Debug, Deserialize)]
struct File {
    version: u32,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    about: String,
    input: String,
    parts: Option<Vec<Part>>,
    canonical: Option<String>,
    errors: Option<Vec<Expected>>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Expected {
    code: String,
    span: Span,
}

fn file() -> File {
    serde_json::from_str(CASES).unwrap()
}

/// The parts without their spans, which a printed query cannot keep.
fn shapes(parts: &[Part]) -> Value {
    let mut value = serde_json::to_value(parts).unwrap();
    for part in value.as_array_mut().unwrap() {
        let part = part.as_object_mut().unwrap();
        part.remove("span");
        if let Some(values) = part.get_mut("values") {
            for value in values.as_array_mut().unwrap() {
                value.as_object_mut().unwrap().remove("span");
            }
        }
    }
    value
}

fn found(errors: &[QueryError]) -> Vec<Expected> {
    errors
        .iter()
        .map(|error| Expected {
            code: error.code.text().to_owned(),
            span: error.span,
        })
        .collect()
}

#[test]
fn the_case_file_is_version_one_and_each_case_has_either_parts_or_errors() {
    let file = file();
    assert_eq!(file.version, 1);
    assert!(file.cases.len() >= 40, "{}", file.cases.len());
    for case in &file.cases {
        let parsed = case.parts.is_some() && case.canonical.is_some();
        assert!(parsed != case.errors.is_some(), "{}", case.about);
    }
}

#[test]
fn every_valid_case_parses_to_its_parts_and_prints_its_canonical_text() {
    for case in file().cases {
        let (Some(parts), Some(canonical)) = (case.parts, case.canonical) else {
            continue;
        };
        let query =
            parse(&case.input).unwrap_or_else(|errors| panic!("{}: {:?}", case.about, errors));
        assert_eq!(query.parts(), parts.as_slice(), "{}", case.about);
        assert_eq!(query.to_string(), canonical, "{}", case.about);
    }
}

#[test]
fn every_invalid_case_gives_its_error_codes_and_spans_in_order() {
    for case in file().cases {
        let Some(expected) = case.errors else {
            continue;
        };
        let errors = parse(&case.input).expect_err(&case.about);
        assert_eq!(found(&errors), expected, "{}", case.about);
    }
}

#[test]
fn every_error_says_how_to_fix_it_in_one_sentence() {
    for case in file().cases {
        let Err(errors) = parse(&case.input) else {
            continue;
        };
        for error in errors {
            let message = &error.message;
            assert!(message.ends_with(['.', '?']), "{}: {message}", case.about);
            assert!(!message.contains('\n'), "{}: {message}", case.about);
            assert!(message.len() > 20, "{}: {message}", case.about);
        }
    }
}

#[test]
fn the_canonical_text_of_every_valid_case_parses_back_to_the_same_parts() {
    for case in file().cases {
        let Ok(query) = parse(&case.input) else {
            continue;
        };
        let again = parse(&query.to_string())
            .unwrap_or_else(|errors| panic!("{}: {:?}", case.about, errors));
        assert_eq!(
            shapes(again.parts()),
            shapes(query.parts()),
            "{}",
            case.about
        );
        assert_eq!(again.to_string(), query.to_string(), "{}", case.about);
    }
}

#[test]
fn the_case_file_s_codes_name_no_tool_so_board_reads_them_as_they_are() {
    for case in file().cases {
        for error in case.errors.unwrap_or_default() {
            assert!(!error.code.contains("::"), "{}: {}", case.about, error.code);
        }
    }
    assert!(CASES.contains("\"unknown_field\""));
}
