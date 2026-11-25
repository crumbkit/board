//! The Rust GLOB gives `SQLite`'s answer for each pattern and path, which
//! `SQLite` itself reports through a board's query.

use std::collections::BTreeSet;

use board_ops::{Host, Limit, literal};
use serde_json::Value;

use super::*;
use crate::check::tests::recording;

const PATTERNS: [&str; 34] = [
    "",
    "*",
    "**",
    "?",
    "a",
    "a*",
    "*a",
    "*a*",
    "a?c",
    "a*c",
    "a**c",
    "*?",
    "?*",
    "*?*c",
    "[ab]x",
    "[^ab]x",
    "[a-c]x",
    "[c-a]x",
    "[-a]x",
    "[a-]x",
    "[]]x",
    "[^]]x",
    "[]a]x",
    "[x",
    "*[x",
    "a*[bc]",
    "*[b]*",
    "a[*]c",
    "a[?]c",
    "src/*.rs",
    "src/*/f?.rs",
    "*/d0[0-1]/f09[^0-8].txt",
    "é*",
    "*ü?",
];

const TEXTS: [&str; 24] = [
    "",
    "a",
    "ab",
    "abc",
    "ac",
    "abbc",
    "ax",
    "bx",
    "cx",
    "dx",
    "-x",
    "]x",
    "[x",
    "a*c",
    "a?c",
    "src/a.rs",
    "src/b/f1.rs",
    "src/b/c/f1.rs",
    "x/d01/f099.txt",
    "x/d02/f099.txt",
    "x/d00/f098.txt",
    "éa",
    "aüb",
    "aü",
];

#[test]
fn the_rust_glob_answers_as_sqlite_does_for_every_pattern_and_path() {
    let (_scenario, host) = recording(&[]);
    let pairs: Vec<(&str, &str)> = (PATTERNS.iter())
        .flat_map(|pattern| TEXTS.iter().map(move |text| (*pattern, *text)))
        .collect();
    let values: Vec<String> = (pairs.iter())
        .map(|(pattern, text)| format!("({}, {})", literal(pattern), literal(text)))
        .collect();
    let sql = format!(
        "SELECT column2 GLOB column1 FROM (VALUES {})",
        values.join(", ")
    );
    let report = host.query(&sql, Limit::ALL).unwrap();
    assert_eq!(report.rows.len(), pairs.len());
    for ((pattern, text), row) in pairs.iter().zip(report.rows) {
        let sqlite = row[0] == 1;
        assert_eq!(glob(pattern, text), sqlite, "{text:?} GLOB {pattern:?}");
    }
}

#[test]
fn a_prefix_ends_before_the_first_wildcard() {
    assert_eq!(prefix("src/d00/f*.rs"), "src/d00/f");
    assert_eq!(prefix("src/a?"), "src/a");
    assert_eq!(prefix("src/[ab]"), "src/");
    assert_eq!(prefix("*/x"), "");
    assert_eq!(prefix("docs/a.md"), "docs/a.md");
}

/// Every string of at most `length` characters drawn from `alphabet`.
fn strings(alphabet: &[char], length: usize) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut last = vec![String::new()];
    for _ in 0..length {
        last = (last.iter())
            .flat_map(|stem| alphabet.iter().map(move |c| format!("{stem}{c}")))
            .collect();
        all.extend(last.iter().cloned());
    }
    all
}

#[test]
fn the_rust_glob_answers_as_sqlite_does_for_every_short_pattern_and_path() {
    let (_scenario, host) = recording(&[]);
    let patterns = strings(&['a', 'b', '*', '?', '[', ']', '^', '-'], 4);
    let texts = strings(&['a', 'b', ']', '-', '^'], 3);
    let table = |each: &[String]| {
        let rows: Vec<String> = each
            .iter()
            .map(|text| format!("({})", literal(text)))
            .collect();
        rows.join(", ")
    };
    // Only the matching pairs come back, so the query stays far inside the
    // query time limit.
    let sql = format!(
        "SELECT p.column1, t.column1 FROM (VALUES {}) p, (VALUES {}) t \
         WHERE t.column1 GLOB p.column1",
        table(&patterns),
        table(&texts)
    );
    let report = host.query(&sql, Limit::ALL).unwrap();
    let sqlite: BTreeSet<(String, String)> = (report.rows.into_iter())
        .map(|row| match <[Value; 2]>::try_from(row) {
            Ok([Value::String(pattern), Value::String(text)]) => (pattern, text),
            row => panic!("{row:?}"),
        })
        .collect();
    let rust: BTreeSet<(String, String)> = (patterns.iter())
        .flat_map(|pattern| texts.iter().map(move |text| (pattern, text)))
        .filter(|(pattern, text)| glob(pattern, text))
        .map(|(pattern, text)| (pattern.clone(), text.clone()))
        .collect();
    assert!(!sqlite.is_empty());
    let only_sqlite: Vec<_> = sqlite.difference(&rust).take(10).collect();
    let only_rust: Vec<_> = rust.difference(&sqlite).take(10).collect();
    assert!(
        only_sqlite.is_empty() && only_rust.is_empty(),
        "(pattern, path) pairs SQLite matches and Rust does not: {only_sqlite:?}; \
         that Rust matches and SQLite does not: {only_rust:?}"
    );
}
