use super::*;

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

#[test]
fn inline_reference_and_image_links_are_found_in_document_order() {
    let text = "\
See [b](b.md), then [the plan][plan] and ![a capture](shots/1.png).

[plan]: /plans/p.md \"The plan\"
";
    assert_eq!(
        links(text.as_bytes()),
        ["b.md", "/plans/p.md", "shots/1.png"]
    );
}

#[test]
fn a_link_keeps_its_fragment_and_query_as_written() {
    let text = "[x](b.md#intro) [y](/c.md?plain=1)";
    assert_eq!(links(text.as_bytes()), ["b.md#intro", "/c.md?plain=1"]);
}

#[test]
fn links_in_code_spans_and_fenced_code_are_not_references() {
    let text = "\
Write `[x](a.md)` to link.

```markdown
[y](b.md)
```

    [z](c.md)

[kept](d.md)
";
    assert_eq!(links(text.as_bytes()), ["d.md"]);
}

#[test]
fn external_urls_autolinks_and_fragments_alone_are_not_references() {
    let text = "\
[web](https://example.com/a.md) [mail](mailto:a@example.com)
[host](//example.com/a.md) [here](#section) <https://example.com/b.md>
<a@example.com> [local](a.md)
";
    assert_eq!(links(text.as_bytes()), ["a.md"]);
}

#[test]
fn a_colon_after_something_that_is_no_scheme_leaves_a_path() {
    let text = "[a](2026:plan.md) [b](my_plan:v2.md) [c](v2:plan.md)";
    assert_eq!(links(text.as_bytes()), ["2026:plan.md", "my_plan:v2.md"]);
}

#[test]
fn a_caret_link_with_no_footnote_definition_is_a_link() {
    assert_eq!(links(b"See [^x](plan.md)."), ["plan.md"]);
}

#[test]
fn a_footnote_definition_is_not_a_link_and_a_link_inside_one_is() {
    let text = "\
A claim.[^1] Another.[^note]

[^1]: Ibid.
[^note]: See [the plan](/plans/p.md).
";
    assert_eq!(links(text.as_bytes()), ["/plans/p.md"]);
}

#[test]
fn bytes_that_are_not_utf8_have_no_links() {
    assert_eq!(links(b"[a](a.md) \xff"), Vec::<String>::new());
    assert_eq!(links(b"[a](a.md) x"), ["a.md"]);
}

#[test]
fn a_relative_link_is_read_from_the_files_directory() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "b.md"), Some(path("/notes/b.md")));
    assert_eq!(reached(&file, "./sub/c.md"), Some(path("/notes/sub/c.md")));
    assert_eq!(reached(&file, "../plans/p.md"), Some(path("/plans/p.md")));
    assert_eq!(reached(&path("/a.md"), "b.md"), Some(path("/b.md")));
}

#[test]
fn an_absolute_link_is_read_from_the_boards_root() {
    let file = path("/notes/deep/a.md");
    assert_eq!(reached(&file, "/plans/p.md"), Some(path("/plans/p.md")));
    assert_eq!(reached(&file, "/"), Some(path("/")));
}

#[test]
fn dot_dot_stops_at_the_root_as_in_a_url() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "../../../b.md"), Some(path("/b.md")));
}

#[test]
fn the_fragment_and_query_do_not_change_the_target() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "b.md#intro"), Some(path("/notes/b.md")));
    assert_eq!(reached(&file, "/c.md?plain=1#x"), Some(path("/c.md")));
}

#[test]
fn a_link_to_a_records_file_reaches_the_record() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "../tasks/t.json"), Some(path("/tasks/t")));
    assert_eq!(reached(&file, "../tasks/t"), Some(path("/tasks/t")));
}

#[test]
fn a_link_to_a_directory_reaches_the_directory() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "sub/"), Some(path("/notes/sub")));
    assert_eq!(reached(&file, "."), Some(path("/notes")));
}

#[test]
fn each_segment_is_percent_decoded_before_dot_segments_are_removed() {
    let file = path("/notes/a.md");
    assert_eq!(
        reached(&file, "my%20plan.md"),
        Some(path("/notes/my plan.md"))
    );
    assert_eq!(reached(&file, "%2e%2e/b.md"), Some(path("/b.md")));
    assert_eq!(reached(&file, "%2E/b.md"), Some(path("/notes/b.md")));
    assert_eq!(reached(&file, "caf%C3%A9.md"), Some(path("/notes/café.md")));
}

#[test]
fn a_segment_that_decodes_to_a_slash_or_to_bytes_that_are_not_utf8_reaches_nothing() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "a%2Fb.md"), None);
    assert_eq!(reached(&file, "a%FFb.md"), None);
    assert_eq!(reached(&file, "a%41b.md"), Some(path("/notes/aAb.md")));
}

#[test]
fn a_link_that_is_not_a_logical_path_reaches_nothing() {
    let file = path("/notes/a.md");
    assert_eq!(reached(&file, "a\u{1}b.md"), None);
    assert_eq!(reached(&file, "ab.md"), Some(path("/notes/ab.md")));
}

#[test]
fn bolero_any_bytes_give_links_that_name_paths() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let file = path("/notes/a.md");
        for link in links(bytes) {
            assert!(!link.starts_with('#'));
            if let Some(target) = reached(&file, &link) {
                assert_eq!(target.as_str().parse::<LogicalPath>().unwrap(), target);
            }
        }
    });
}

#[test]
fn a_file_s_text_is_its_first_heading_and_its_prose_after_the_frontmatter() {
    let file = "\
---
$type: tasks/task/v1
title: Ship it
---
Some intro.

## The *budget* plan

# Later heading
";
    let found = text(file.as_bytes()).unwrap();
    assert_eq!(found.heading.as_deref(), Some("The budget plan"));
    assert_eq!(
        found.body,
        "Some intro.\n\n## The *budget* plan\n\n# Later heading\n"
    );
}

#[test]
fn a_file_with_no_heading_has_none_and_its_whole_text_as_prose() {
    let found = text(b"Just a paragraph.\n").unwrap();
    assert_eq!(found.heading, None);
    assert_eq!(found.body, "Just a paragraph.\n");
    let setext = text(b"Title\n=====\n\nBody.\n").unwrap();
    assert_eq!(setext.heading.as_deref(), Some("Title"));
}

#[test]
fn a_notes_app_s_frontmatter_is_not_prose() {
    let found = text(b"---\ntags: [a]\n---\n# Note\n").unwrap();
    assert_eq!(found.heading.as_deref(), Some("Note"));
    assert_eq!(found.body, "# Note\n");
}

#[test]
fn prose_is_cut_at_the_text_limit_on_a_character_boundary() {
    let long = format!("{}é tail", "a".repeat(TEXT_BYTES - 1));
    let found = text(long.as_bytes()).unwrap();
    assert_eq!(found.body.len(), TEXT_BYTES - 1);
    let fits = "a".repeat(TEXT_BYTES);
    assert_eq!(text(fits.as_bytes()).unwrap().body.len(), TEXT_BYTES);
}

#[test]
fn bytes_that_are_not_utf8_have_no_text() {
    assert_eq!(text(b"# Title\n\xff\xfe"), None);
    assert!(text(b"# Title\n").is_some());
}
