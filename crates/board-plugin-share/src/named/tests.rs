//! `named` finds the absolute board paths a Markdown body names in prose,
//! in a code span and as a link, and ignores the frontmatter, URLs,
//! relative paths and text that is not a logical path.

use super::named;

fn paths(text: &str) -> Vec<String> {
    named(text).iter().map(ToString::to_string).collect()
}

#[test]
fn a_path_in_prose_in_a_code_span_and_before_a_full_stop_is_named() {
    let text = "2. Keep one file. Decision: /projects/x/decisions/a\n\
                See `/projects/x/questions/b` and /projects/x/notes/c.\n";
    assert_eq!(
        paths(text),
        [
            "/projects/x/decisions/a",
            "/projects/x/notes/c",
            "/projects/x/questions/b"
        ]
    );
}

#[test]
fn a_link_to_an_absolute_path_is_named_and_a_relative_link_is_not() {
    let text = "Asked in [the question](/projects/x/questions/b), see [other](other.md).\n";
    assert_eq!(paths(text), ["/projects/x/questions/b"]);
}

#[test]
fn a_path_after_a_parenthesis_or_with_a_json_extension_is_named_as_its_record() {
    let text = "(see /projects/x/decisions/a.json; /projects/x/notes/c.md)\n";
    assert_eq!(
        paths(text),
        ["/projects/x/decisions/a", "/projects/x/notes/c.md"]
    );
}

#[test]
fn a_path_in_the_frontmatter_a_url_a_relative_path_and_the_root_are_not_named() {
    let text = "---\n$type: lr/task/v1\nsee: /tasks\n---\n\
                Read https://x/y, tasks/a.md, ./b, //host/c, km/h and a / b.\n";
    assert_eq!(paths(text), Vec::<String>::new());
}

#[test]
fn a_path_in_frontmatter_that_claims_no_type_is_named_as_body_text() {
    let text = "---\ntitle: x\n---\nSee /tasks/a.md\n";
    let named: Vec<String> = paths(text);
    assert_eq!(named, ["/tasks/a.md"]);
    assert_eq!(paths("---\ntitle: /notes/n\n---\n"), ["/notes/n"]);
}

#[test]
fn text_that_is_not_a_logical_path_is_not_named() {
    assert_eq!(paths("/a/../b /c/./d /e//f\n"), Vec::<String>::new());
}

#[test]
fn a_path_named_twice_is_named_once() {
    assert_eq!(paths("/a/b and /a/b.\n"), ["/a/b"]);
}
