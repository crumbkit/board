use proptest::prelude::*;

use super::*;

/// Asserts that `admitted` parses and that `refused`, its otherwise identical
/// twin, is refused as malformed for `reason`.
fn pair(admitted: &str, refused: &str, reason: Malformation) {
    let path: LogicalPath = admitted.parse().unwrap();
    assert_eq!(path.as_str(), admitted);
    let error = refused.parse::<LogicalPath>().unwrap_err();
    assert_eq!(error.code(), Code::PathMalformed);
    assert_eq!(
        error,
        PathError::Malformed {
            path: refused.to_owned(),
            reason,
        }
    );
}

fn segment_of(bytes: usize) -> String {
    "s".repeat(bytes)
}

#[test]
fn a_path_must_start_with_a_slash() {
    pair("/work/x", "work/x", Malformation::NotAbsolute);
    pair("/", "", Malformation::NotAbsolute);
}

#[test]
fn the_root_is_a_path_with_no_segments() {
    let root: LogicalPath = "/".parse().unwrap();
    assert_eq!(root.segments().count(), 0);
}

#[test]
fn segments_come_back_in_order() {
    let path: LogicalPath = "/work/board/x".parse().unwrap();
    assert_eq!(path.segments().collect::<Vec<_>>(), ["work", "board", "x"]);
}

#[test]
fn a_segment_may_not_be_empty() {
    pair("/a/b", "/a//b", Malformation::EmptySegment);
    pair("/a", "/a/", Malformation::EmptySegment);
    pair("/a", "//a", Malformation::EmptySegment);
}

#[test]
fn a_segment_may_not_be_a_dot_or_two() {
    pair("/a/.b", "/a/.", Malformation::DotSegment);
    pair("/a/..b", "/a/..", Malformation::DotSegment);
    pair("/.../a", "/../a", Malformation::DotSegment);
}

#[test]
fn a_segment_holds_at_most_255_bytes() {
    let fits = format!("/{}", segment_of(255));
    let over = format!("/{}", segment_of(256));
    pair(&fits, &over, Malformation::SegmentTooLong { bytes: 256 });
}

#[test]
fn the_segment_limit_counts_bytes_not_characters() {
    // "€" is three bytes, so 85 of them fill a segment exactly.
    let fits = format!("/{}", "€".repeat(85));
    let over = format!("/{}", "€".repeat(86));
    pair(&fits, &over, Malformation::SegmentTooLong { bytes: 258 });
}

#[test]
fn a_path_holds_at_most_4096_bytes() {
    // Sixteen segments of 255 bytes, each with its slash, are 4,096 bytes.
    let fits = format!("/{}", segment_of(255)).repeat(16);
    assert_eq!(fits.len(), MAX_PATH_BYTES);
    let over = format!("{fits}x");
    pair(&fits, &over, Malformation::TooLong { bytes: 4097 });
}

#[test]
fn a_segment_may_not_hold_a_control_character() {
    for control in ['\0', '\t', '\n', '\u{7f}', '\u{85}'] {
        pair(
            "/a b",
            &format!("/a{control}b"),
            Malformation::ControlCharacter,
        );
    }
}

#[test]
fn a_segment_may_hold_any_other_unicode() {
    for name in ["/naïve", "/日本語", "/emoji-🙂", "/a:b", "/A B", "/a\\b"] {
        assert_eq!(name.parse::<LogicalPath>().unwrap().as_str(), name);
    }
}

#[test]
fn the_message_names_the_path_and_the_rule() {
    let error = "/a//b".parse::<LogicalPath>().unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"logical path "/a//b" is malformed: it has an empty segment"#
    );
}

#[test]
fn deserializing_enforces_the_same_rules_as_parsing() {
    let path: LogicalPath = serde_json::from_str(r#""/work/x""#).unwrap();
    assert_eq!(path.as_str(), "/work/x");
    let error = serde_json::from_str::<LogicalPath>(r#""work/x""#).unwrap_err();
    assert!(
        error.to_string().contains("does not start with /"),
        "{error}"
    );
}

#[test]
fn a_path_serializes_as_its_string() {
    let path: LogicalPath = "/work/x".parse().unwrap();
    assert_eq!(serde_json::to_string(&path).unwrap(), r#""/work/x""#);
}

/// A segment that keeps every rule: no `/`, no control character, not `.` or
/// `..`, and short enough that sixteen of them fit in one path.
fn segment() -> impl Strategy<Value = String> {
    "[^/\\p{Cc}]{1,64}".prop_filter("dot segment", |segment| segment != "." && segment != "..")
}

proptest! {
    #[test]
    fn a_valid_path_round_trips_through_its_display(
        segments in proptest::collection::vec(segment(), 0..16),
    ) {
        let text = format!("/{}", segments.join("/"));
        prop_assume!(text.len() <= MAX_PATH_BYTES);
        let path: LogicalPath = text.parse().unwrap();
        prop_assert_eq!(path.segments().collect::<Vec<_>>(), segments.iter().map(String::as_str).collect::<Vec<_>>());
        let again: LogicalPath = path.to_string().parse().unwrap();
        prop_assert_eq!(again, path);
    }

    #[test]
    fn a_prefix_is_the_path_of_its_first_segments(
        segments in proptest::collection::vec(segment(), 0..16),
        depth in 0usize..20,
    ) {
        let text = format!("/{}", segments.join("/"));
        prop_assume!(text.len() <= MAX_PATH_BYTES);
        let path: LogicalPath = text.parse().unwrap();
        let kept = &segments[..depth.min(segments.len())];
        let expected: LogicalPath = format!("/{}", kept.join("/")).parse().unwrap();
        prop_assert_eq!(path.prefix(depth), expected);
    }

    #[test]
    fn an_admitted_path_is_its_segments_and_a_refusal_names_the_input(
        text in "(/|[^/]{0,4}|\\PC{0,4}|\\p{Cc}|\\.){0,12}",
    ) {
        match text.parse::<LogicalPath>() {
            Ok(path) => {
                let rebuilt = format!("/{}", path.segments().collect::<Vec<_>>().join("/"));
                prop_assert_eq!(rebuilt, text);
            }
            Err(PathError::Malformed { path, .. }) => prop_assert_eq!(path, text),
        }
    }
}

#[test]
fn bolero_any_text_is_admitted_exactly_or_refused_as_malformed() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        match text.parse::<LogicalPath>() {
            Ok(path) => {
                assert_eq!(path.as_str(), text);
                assert_eq!(path.to_string().parse::<LogicalPath>().unwrap(), path);
            }
            Err(error) => assert_eq!(error.code(), Code::PathMalformed),
        }
    });
}
