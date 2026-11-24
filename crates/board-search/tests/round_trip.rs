//! Plan test: printing the parts of any query and parsing the text again
//! gives the same parts, over generated queries.
#![cfg(test)]

use board_search::{Field, Part, Query, Span, Term, Value, parse};
use proptest::prelude::*;

/// Any span: the printer ignores spans, and the comparison leaves them out.
const ANY: Span = Span::new(0, 0);

/// A word: letters and digits from three scripts, a dash or a colon after
/// the first character, and never a field name before a colon.
fn word() -> impl Strategy<Value = String> {
    "[a-zé日0-9][a-zé日0-9\\-]{0,8}(:[0-9]{1,2})?".prop_filter("not a filter", |word| {
        !word
            .split_once(':')
            .is_some_and(|(name, _)| name.starts_with(|c: char| c.is_ascii_alphabetic()))
    })
}

/// A phrase's text: words and spaces, with at least one word.
fn phrase() -> impl Strategy<Value = String> {
    "[a-zé ,:\\-]{0,6}[a-zé][a-zé ,:\\-]{0,6}"
}

/// A value: a plain one, or one with a space or a comma that prints quoted.
fn value() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z0-9@.\\-]{1,10}", "[a-z]{1,4}[ ,][a-z]{1,4}"]
}

fn term() -> impl Strategy<Value = Term> {
    prop_oneof![
        word().prop_map(|text| Term::Word { text }),
        phrase().prop_map(|text| Term::Phrase { text }),
        (
            prop_oneof![Just(Field::Type), Just(Field::From)],
            prop::collection::vec(value(), 1..4)
        )
            .prop_map(|(field, values)| Term::Filter {
                field,
                values: values
                    .into_iter()
                    .map(|text| Value { text, span: ANY })
                    .collect(),
            }),
    ]
}

fn part() -> impl Strategy<Value = Part> {
    (term(), any::<bool>()).prop_map(|(term, negated)| Part {
        term,
        negated,
        span: ANY,
    })
}

/// The part with every span set to [`ANY`].
fn without_spans(part: &Part) -> Part {
    let term = match &part.term {
        Term::Filter { field, values } => Term::Filter {
            field: *field,
            values: values
                .iter()
                .map(|value| Value {
                    text: value.text.clone(),
                    span: ANY,
                })
                .collect(),
        },
        other => other.clone(),
    };
    Part {
        term,
        negated: part.negated,
        span: ANY,
    }
}

proptest! {
    #[test]
    fn printing_any_query_and_parsing_it_again_gives_the_same_parts(
        parts in prop::collection::vec(part(), 0..6)
    ) {
        let query = Query::new(parts.clone());
        let text = query.to_string();
        let parsed = parse(&text).map_err(|errors| TestCaseError::fail(format!("{text}: {errors:?}")))?;
        let found: Vec<Part> = parsed.parts().iter().map(without_spans).collect();
        prop_assert_eq!(found, parts, "{}", text);
        prop_assert_eq!(parsed.to_string(), text);
    }

    #[test]
    fn any_text_parses_or_gives_errors_whose_spans_lie_inside_it(text in "\\PC{0,40}") {
        let length = text.chars().count();
        match parse(&text) {
            Ok(query) => {
                for part in query.parts() {
                    prop_assert!(part.span.start < part.span.end && part.span.end <= length);
                }
            }
            Err(errors) => {
                prop_assert!(!errors.is_empty());
                for error in errors {
                    prop_assert!(error.span.start < error.span.end && error.span.end <= length);
                }
            }
        }
    }
}
