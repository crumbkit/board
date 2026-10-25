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
            Malformation::HiddenCharacter { char: control },
        );
    }
}

#[test]
fn a_segment_may_not_hold_a_bidi_control_a_zero_width_character_or_a_separator() {
    let hidden = [
        '\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}', '\u{200b}', '\u{200f}', '\u{feff}',
        '\u{2028}', '\u{2029}', '\u{115f}', '\u{1160}', '\u{3164}', '\u{ffa0}',
    ];
    for char in hidden {
        pair(
            "/notes/café",
            &format!("/notes/caf{char}é"),
            Malformation::HiddenCharacter { char },
        );
    }
}

#[test]
fn a_segment_may_hold_a_zero_width_joiner_or_non_joiner_and_not_a_zero_width_space() {
    for name in ["/👨\u{200d}👩\u{200d}👧.md", "/می\u{200c}خواهم.md"] {
        assert_eq!(name.parse::<LogicalPath>().unwrap().as_str(), name);
    }
    pair(
        "/می\u{200c}خواهم.md",
        "/می\u{200b}خواهم.md",
        Malformation::HiddenCharacter { char: '\u{200b}' },
    );
}

#[test]
fn escaping_shows_each_hidden_character_and_keeps_a_joiner() {
    assert_eq!(escaped("a\u{202e}b\u{3164}c"), "a\\u{202e}b\\u{3164}c");
    assert_eq!(escaped("👨\u{200d}👩 café"), "👨\u{200d}👩 café");
}

#[test]
fn a_hidden_character_is_named_by_its_code_point() {
    let error = "/notes/a\u{202e}b".parse::<LogicalPath>().unwrap_err();
    assert!(error.to_string().contains("U+202E"), "{error}");
    let error = "/notes/a\tb".parse::<LogicalPath>().unwrap_err();
    assert!(error.to_string().contains("U+0009"), "{error}");
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

/// `text` read from `from`, and the refusal of its otherwise identical twin
/// as malformed for `reason`.
fn joined(from: &str, text: &str, expected: &str, twin: &str, reason: Malformation) {
    let from: LogicalPath = from.parse().unwrap();
    assert_eq!(from.join(text).unwrap().as_str(), expected);
    let error = from.join(twin).unwrap_err();
    assert_eq!(error.code(), Code::PathMalformed);
    assert!(
        matches!(error, PathError::Malformed { reason: got, .. } if got == reason),
        "{error}"
    );
}

#[test]
fn a_dot_is_the_path_joined_to_and_a_dot_dot_is_refused_as_a_dot_segment() {
    joined(
        "/repos/lib",
        ".",
        "/repos/lib",
        "..",
        Malformation::DotSegment,
    );
    joined("/", ".", "/", "..", Malformation::DotSegment);
    joined(
        "/repos/lib",
        "./x",
        "/repos/lib/x",
        "../x",
        Malformation::DotSegment,
    );
    joined("/", "./x", "/x", "./../x", Malformation::DotSegment);
    joined("/repos", "./.", "/repos", "./..", Malformation::DotSegment);
}

#[test]
fn a_relative_path_continues_the_path_joined_to_and_an_absolute_one_replaces_it() {
    joined(
        "/repos/lib",
        "docs/a.md",
        "/repos/lib/docs/a.md",
        "docs//a.md",
        Malformation::EmptySegment,
    );
    joined(
        "/",
        "docs/a.md",
        "/docs/a.md",
        "docs/./a.md",
        Malformation::DotSegment,
    );
    joined(
        "/repos/lib",
        "/docs",
        "/docs",
        "/docs/",
        Malformation::EmptySegment,
    );
    joined(
        "/repos/lib",
        "./",
        "/repos/lib",
        "",
        Malformation::NotAbsolute,
    );
}

#[test]
fn one_trailing_slash_on_a_relative_path_is_dropped_and_two_are_an_empty_segment() {
    joined(
        "/repos",
        "lib/",
        "/repos/lib",
        "lib//",
        Malformation::EmptySegment,
    );
    joined("/", "./lib/", "/lib", "/lib/", Malformation::EmptySegment);
}

/// A segment that keeps every rule: no `/`, no hidden character, not `.` or
/// `..`, and short enough that sixteen of them fit in one path.
fn segment() -> impl Strategy<Value = String> {
    "[^/\\p{Cc}\\p{Cf}\\p{Zl}\\p{Zp}]{1,64}".prop_filter("dot or hidden", |segment| {
        segment != "." && segment != ".." && !segment.chars().any(is_hidden)
    })
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
    fn a_relative_path_joins_as_the_absolute_path_beneath_it(
        from in proptest::collection::vec(segment(), 0..4),
        rest in proptest::collection::vec(segment(), 1..4),
    ) {
        let from: LogicalPath = format!("/{}", from.join("/")).parse().unwrap();
        let rest = rest.join("/");
        let absolute = format!("{}/{rest}", from.as_str().trim_end_matches('/'));
        for text in [rest.clone(), format!("./{rest}"), absolute.clone()] {
            prop_assert_eq!(String::from(from.join(&text).unwrap()), absolute.clone());
        }
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
