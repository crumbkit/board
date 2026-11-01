//! The work `evaluate` asks of Board, and how it reads a pattern file.

use serde_json::json;

use super::*;
use crate::check::tests::{Call, recording};

fn control(rule: &Value) -> String {
    json!({"$type": "controls/control/v1", "requirement": "R.", "purpose": "P.", "rule": rule})
        .to_string()
}

#[test]
fn evaluate_reads_every_rule_file_once_in_one_get_each_call_and_none_without_one() {
    let names = control(&json!({"pattern": "/private/names.txt", "show": "match-only"}));
    let again = control(&json!({"pattern": "/private/names.txt", "show": "pattern"}));
    let short = control(&json!({"budget": "words", "max": 3}));
    let (_scenario, host) = recording(&[
        ("controls/names.json", &names),
        ("controls/again.json", &again),
        ("controls/short.json", &short),
        ("private/names.txt", "acme\n"),
    ]);
    let parts = json!({"a": "acme", "b": "fine"});
    let given = json!({"controls": ["/controls/short"], "parts": parts});
    evaluate(&host, &given).unwrap();
    assert_eq!(host.resolved(), []);
    let given = json!({"controls": ["/controls/names", "/controls/again"], "parts": parts});
    let output = evaluate(&host, &given).unwrap();
    assert_eq!(
        host.resolved(),
        [Call::GetEach(vec!["/private/names.txt".to_owned()])]
    );
    assert_eq!(output["passed"], false);
}

#[test]
fn a_parser_message_never_reaches_a_match_only_error_and_a_shown_one_quotes_the_line() {
    let rule = |show: Show| Pattern {
        pattern: "/private/names.txt".to_owned(),
        show,
    };
    for line in [
        "(?P=acmename)",
        "acme\\q",
        "(?P<1acme>x)",
        "a{99999999999}",
        "(?=acme)",
        "[acme",
    ] {
        let sources = BTreeMap::from([(
            "/private/names.txt".to_owned(),
            format!("ok\n{line}\n").into_bytes(),
        )]);
        let hidden = expressions("/controls/names", &rule(Show::MatchOnly), &sources).unwrap_err();
        assert_eq!(hidden.code, "controls::pattern::invalid", "{line}");
        assert!(!hidden.message.contains("acme"), "{}", hidden.message);
        assert!(!hidden.message.contains(line), "{}", hidden.message);
        assert!(
            hidden.message.starts_with("line 2 of /private/names.txt"),
            "{}",
            hidden.message
        );
        assert_eq!(hidden.fields["line"], json!(2));
        let shown = expressions("/controls/names", &rule(Show::Pattern), &sources).unwrap_err();
        assert!(
            shown.message.contains(&format!("{line:?}")),
            "{}",
            shown.message
        );
    }
    let sources = BTreeMap::from([(
        "/private/names.txt".to_owned(),
        b"(?P<acmename>acme)\n\n".to_vec(),
    )]);
    let admitted = expressions("/controls/names", &rule(Show::MatchOnly), &sources).unwrap();
    assert_eq!(admitted.len(), 1);
}

#[test]
fn a_match_names_the_line_it_starts_on_and_a_match_of_no_characters_is_no_finding() {
    let expressions = vec![compiled("b+"), compiled("^"), compiled("a$")];
    let (found, total) = matches(&expressions, false, "a\nbb\nxa");
    assert_eq!(total, 3);
    let quoted: Vec<(&str, Option<usize>)> = (found.iter())
        .map(|finding| (finding.text.as_str(), finding.line))
        .collect();
    assert_eq!(quoted, [("a", Some(1)), ("bb", Some(2)), ("a", Some(3))]);
}

fn compiled(line: &str) -> (String, regex::Regex) {
    (
        line.to_owned(),
        RegexBuilder::new(line).multi_line(true).build().unwrap(),
    )
}

#[test]
fn numbering_the_lines_of_many_matches_scans_each_byte_once_per_expression_and_keeps_only_the_shown()
 {
    // 4,000 lines of 50 `x` give 200,000 matches of `x`.
    let text = format!("{}\n", "x".repeat(50)).repeat(4_000);
    let expressions = vec![compiled("x"), compiled("^x")];
    let (hits, scanned) = hits(&expressions, &text);
    assert_eq!(hits.len(), 204_000);
    assert!(
        scanned <= text.len() * expressions.len(),
        "scanned {scanned} bytes"
    );
    let last = hits.iter().map(|hit| hit.line).max();
    assert_eq!(last, Some(4_000));
    let (found, total) = matches(&expressions, false, &text);
    assert_eq!(total, 204_000);
    assert_eq!(found.len(), SHOWN);
    assert!(found.iter().all(|finding| finding.line == Some(1)));
    // A part with fewer matches than are shown keeps them all.
    let (found, total) = matches(&expressions, false, "x\ny");
    assert_eq!((found.len(), total), (2, 2));
}
