//! A URL is an absolute `http` or `https` URI with a host, and its schema
//! admits exactly the strings its constructor admits.

use std::str::FromStr;

use proptest::prelude::*;
use schemars::generate::SchemaSettings;
use serde_json::{Value, json};

use super::*;
use crate::shape::schema_of;

fn code(text: &str) -> Option<&'static str> {
    Url::from_str(text).err().map(|error| error.code())
}

#[test]
fn a_scheme_other_than_lower_case_http_or_https_is_refused_and_its_twin_is_admitted() {
    assert_eq!(code("ftp://example.com/x"), Some("board::url::scheme"));
    assert_eq!(code("https://example.com/x"), None);
    assert_eq!(code("HTTPS://example.com"), Some("board::url::scheme"));
    assert_eq!(code("https://example.com"), None);
}

#[test]
fn text_that_is_not_an_absolute_uri_is_malformed_and_its_twin_is_admitted() {
    assert_eq!(code("/pull/1"), Some("board::url::malformed"));
    assert_eq!(code("https://example.com/pull/1"), None);
    assert_eq!(code("not a url"), Some("board::url::malformed"));
    assert_eq!(code("http://localhost:8080/x"), None);
}

#[test]
fn a_url_without_a_host_is_refused_and_its_twin_with_one_is_admitted() {
    assert_eq!(code("https://"), Some("board::url::no_host"));
    assert_eq!(code("https://a"), None);
    assert_eq!(code("https://user@:80/x"), Some("board::url::no_host"));
    assert_eq!(code("https://user@h:80/x"), None);
    assert_eq!(code("https:/x"), Some("board::url::no_host"));
    assert_eq!(code("https://x"), None);
}

#[test]
fn each_refusal_names_the_value() {
    for text in ["ftp://example.com/x", "not a url", "https://"] {
        let message = Url::from_str(text).unwrap_err().to_string();
        assert!(message.contains(&format!("{text:?}")), "{message}");
    }
}

#[test]
fn a_url_reads_from_json_through_the_same_check_and_writes_back_unchanged() {
    let text = "https://example.com/pull/1?tab=files#diff";
    let url: Url = serde_json::from_value(json!(text)).unwrap();
    assert_eq!(url.as_str(), text);
    assert_eq!(serde_json::to_value(&url).unwrap(), json!(text));
    let refused = serde_json::from_value::<Url>(json!("ftp://example.com/x")).unwrap_err();
    assert!(
        refused.to_string().contains("ftp://example.com/x"),
        "{refused}"
    );
    assert_eq!(Url::try_from(text.to_owned()).unwrap(), url);
}

#[test]
fn the_schema_is_an_inlined_uri_string_whose_pattern_needs_a_host() {
    assert_eq!(
        schema_of::<Url>(SchemaSettings::draft2020_12()),
        json!({
            "title": "Url",
            "type": "string",
            "format": "uri",
            "pattern": PATTERN,
        })
    );
    assert!(Url::inline_schema());
}

fn schema() -> board_validate::Schema {
    board_validate::Schema::compile(schema_of::<Url>(SchemaSettings::draft2020_12())).unwrap()
}

fn agree(schema: &board_validate::Schema, text: &str) {
    assert_eq!(
        Url::from_str(text).is_ok(),
        schema.validate(&Value::String(text.to_owned())).is_ok(),
        "{text:?}"
    );
}

#[test]
fn the_constructor_and_the_schema_agree_at_each_edge_of_the_check() {
    let schema = schema();
    for text in [
        "https://",
        "https://:80",
        "https://@",
        "https://u@",
        "https://u@:80/x",
        "https://@h",
        "https:/x",
        "https:x",
        "https://[::1]:8080/x",
        "https://a b",
        "https://ä",
        "Https://a",
        "http://a?#",
        "https://%41",
        "https://%4",
    ] {
        agree(&schema, text);
    }
}

/// Pieces that put the check's edges side by side: each scheme case, an
/// empty or missing authority, user information, ports, IP literals and
/// characters that RFC 3986 refuses. The common pieces repeat, so most
/// strings reach the host.
fn uri_shaped() -> impl Strategy<Value = String> {
    let scheme = prop::sample::select(vec!["http", "https", "https", "HTTPS", "ftp", ""]);
    let colon = prop::sample::select(vec![":", ":", ""]);
    let slashes = prop::sample::select(vec!["//", "//", "/", ""]);
    let user = prop::sample::select(vec!["", "", "@", "u@", "u:p@", "a@b@", "%41@"]);
    let host = prop::sample::select(vec![
        "",
        "",
        "a",
        "example.com",
        "[::1]",
        "[v1.x]",
        "[::1",
        "%41",
        "%4",
        "a b",
        "ä",
        ":",
    ]);
    let port = prop::sample::select(vec!["", "", ":", ":80", ":x"]);
    let rest = "[a/?#%: \\[\\]@é]{0,4}";
    (scheme, colon, slashes, user, host, port, rest).prop_map(
        |(scheme, colon, slashes, user, host, port, rest)| {
            [scheme, colon, slashes, user, host, port, &rest].concat()
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn the_constructor_and_the_schema_admit_the_same_uri_shaped_strings(text in uri_shaped()) {
        agree(&schema(), &text);
    }

    #[test]
    fn the_constructor_and_the_schema_admit_the_same_arbitrary_strings(text in ".{0,24}") {
        agree(&schema(), &text);
    }
}

#[test]
fn bolero_any_text_is_admitted_by_the_constructor_exactly_when_the_schema_admits_it() {
    let schema = std::panic::AssertUnwindSafe(schema());
    bolero::check!().for_each(|bytes: &[u8]| {
        if let Ok(text) = std::str::from_utf8(bytes) {
            agree(&schema, text);
        }
    });
}
