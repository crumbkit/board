use super::*;

#[test]
fn words_are_separated_by_any_whitespace() {
    assert_eq!(count("one  two\nthree\tfour\n"), 4);
    assert_eq!(count(""), 0);
}

#[test]
fn an_import_is_a_token_that_starts_with_an_at_sign() {
    assert_eq!(
        imports("see @AGENTS.md and @docs/a.md\n@b.md"),
        ["AGENTS.md", "docs/a.md", "b.md"]
    );
}

#[test]
fn an_at_sign_inside_a_word_is_not_an_import() {
    assert!(imports("mail owner@example.com, or write @ alone").is_empty());
}

#[test]
fn an_import_inside_inline_code_is_not_an_import() {
    assert_eq!(
        imports("write `@path` to import, like @real.md"),
        ["real.md"]
    );
}

#[test]
fn an_import_inside_a_fenced_block_is_not_an_import() {
    let text = "@before.md\n```\n@inside.md\n```\n~~~\n@tilde.md\n~~~\n@after.md\n";
    assert_eq!(imports(text), ["before.md", "after.md"]);
}

#[test]
fn a_link_target_drops_its_title_brackets_fragment_and_leading_slash() {
    assert_eq!(
        link_targets("[a](plan/a.md \"t\") [b](<plan/b.md>) [c](./plan/c.md#x) [d](/plan/d.md)"),
        ["plan/a.md", "plan/b.md", "plan/c.md", "plan/d.md"]
    );
}

#[test]
fn a_link_inside_code_is_not_a_link_and_its_twin_outside_code_is() {
    let text = "`[a](plan/a.md)`\n```\n[b](plan/b.md)\n```\n[c](plan/c.md)";
    assert_eq!(link_targets(text), ["plan/c.md"]);
}
