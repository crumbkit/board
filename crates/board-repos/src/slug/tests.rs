//! Reflect's slug for a title, against cases from Reflect's own tests.
//! These were the slug cases of `plugins/notes/tests/test_layout.py`.

use super::*;

#[test]
fn a_title_takes_the_slug_reflect_gives_it() {
    let x59 = "x".repeat(59);
    let script = "\u{1d49c}";
    let cases = [
        // slugForTitle's documented examples (packages/core/src/markdown/slug.ts).
        ("Meeting Notes".to_owned(), "meeting-notes".to_owned()),
        ("Don't Panic!".to_owned(), "dont-panic".to_owned()),
        ("日本語ノート".to_owned(), "日本語ノート".to_owned()),
        ("🎉🎉🎉".to_owned(), "untitled".to_owned()),
        ("CON".to_owned(), "con-note".to_owned()),
        ("  a_b -- c  ".to_owned(), "a-b-c".to_owned()),
        (format!("{x59} y"), x59.clone()),
        // Cases checked against slugForTitle itself, which drops combining
        // marks NFC cannot compose, counts characters outside the Basic
        // Multilingual Plane as one each, and uses JavaScript's `\s`.
        ("Cafe\u{301}".to_owned(), "café".to_owned()),
        ("x\u{20dd} y".to_owned(), "x-y".to_owned()),
        (
            format!("{script}\u{1d4b7} math"),
            format!("{script}\u{1d4b7}-math"),
        ),
        (script.repeat(100), script.repeat(60)),
        ("a\u{feff}b".to_owned(), "a-b".to_owned()),
        ("a\u{1c}b\u{85}c".to_owned(), "abc".to_owned()),
    ];
    for (title, expected) in cases {
        assert_eq!(slug(&title), expected, "{title:?}");
    }
}
