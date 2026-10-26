use super::*;

const GIT: &str = "\
commit 0123abc
Author: A <a@example.com>

    Raise the retry limit

diff --git a/src/retry.rs b/src/retry.rs
index 1111111..2222222 100644
--- a/src/retry.rs
+++ b/src/retry.rs
@@ -1,4 +1,5 @@ fn limit
 fn limit() -> u32 {
-    3
+    // Measured on a flaky remote.
+    5
 }
 
diff --git a/docs/new.md b/docs/new.md
new file mode 100644
--- /dev/null
+++ b/docs/new.md
@@ -0,0 +1 @@
+New.
\\ No newline at end of file
diff --git a/old.txt b/renamed.txt
similarity index 100%
rename from old.txt
rename to renamed.txt
diff --git a/logo.png b/logo.png
Binary files a/logo.png and b/logo.png differ
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-Gone.
";

type Summary<'a> = (Option<&'a str>, Option<&'a str>, usize, usize, bool);

fn summary(patch: &Patch) -> Vec<Summary<'_>> {
    patch
        .files
        .iter()
        .map(|file| {
            (
                file.old.as_deref(),
                file.new.as_deref(),
                file.added,
                file.removed,
                file.binary,
            )
        })
        .collect()
}

#[test]
fn a_git_diff_reads_as_its_files_with_their_names_and_change_counts() {
    let patch = Patch::parse(GIT).unwrap();
    assert_eq!(
        summary(&patch),
        [
            (Some("src/retry.rs"), Some("src/retry.rs"), 2, 1, false),
            (None, Some("docs/new.md"), 1, 0, false),
            (Some("old.txt"), Some("renamed.txt"), 0, 0, false),
            (Some("logo.png"), Some("logo.png"), 0, 0, true),
            (Some("gone.txt"), None, 0, 1, false),
        ]
    );
}

#[test]
fn hunk_lines_carry_their_numbers_in_the_old_and_new_file() {
    let patch = Patch::parse(GIT).unwrap();
    let hunk = &patch.files[0].hunks[0];
    assert_eq!(hunk.header, "@@ -1,4 +1,5 @@ fn limit");
    let numbered: Vec<_> = hunk
        .lines
        .iter()
        .map(|line| (line.change, line.old, line.new, line.text.as_str()))
        .collect();
    assert_eq!(
        numbered,
        [
            (Change::Context, Some(1), Some(1), "fn limit() -> u32 {"),
            (Change::Removed, Some(2), None, "    3"),
            (
                Change::Added,
                None,
                Some(2),
                "    // Measured on a flaky remote."
            ),
            (Change::Added, None, Some(3), "    5"),
            (Change::Context, Some(3), Some(4), "}"),
            (Change::Context, Some(4), Some(5), ""),
        ]
    );
}

#[test]
fn a_plain_unified_diff_keeps_its_paths_and_drops_their_timestamps() {
    let text = "--- retry.rs\t2026-10-25 10:00:00\n+++ retry.rs\t2026-10-25 10:05:00\n@@ -1 +1 @@\n-3\n+5\n";
    let patch = Patch::parse(text).unwrap();
    assert_eq!(
        summary(&patch),
        [(Some("retry.rs"), Some("retry.rs"), 1, 1, false)]
    );
}

#[test]
fn a_text_with_no_file_diff_is_refused_and_the_same_text_with_one_is_admitted() {
    let prose = "Raise the retry limit.\n";
    assert_eq!(Patch::parse(prose), Err(PatchError::Empty));
    assert_eq!(Patch::parse(""), Err(PatchError::Empty));
    assert!(Patch::parse(&format!("{prose}--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n")).is_ok());
}

#[test]
fn a_hunk_before_any_file_header_is_refused_and_one_after_a_header_is_admitted() {
    let hunk = "@@ -1 +1 @@\n-a\n+b\n";
    assert_eq!(Patch::parse(hunk), Err(PatchError::Fragment { line: 1 }));
    assert!(Patch::parse(&format!("--- a/x\n+++ b/x\n{hunk}")).is_ok());
}

#[test]
fn a_malformed_hunk_header_is_refused_and_a_well_formed_one_is_admitted() {
    let file = |header: &str| format!("--- a/x\n+++ b/x\n{header}\n-a\n+b\n");
    assert_eq!(
        Patch::parse(&file("@@ -1 1 @@")),
        Err(PatchError::Header { line: 3 })
    );
    assert_eq!(
        Patch::parse(&file(&format!("@@ -{} +1 @@", usize::MAX))),
        Err(PatchError::Header { line: 3 })
    );
    assert!(Patch::parse(&file("@@ -1 +1 @@")).is_ok());
}

#[test]
fn a_hunk_shorter_than_its_header_counts_is_refused_and_a_complete_one_is_admitted() {
    let complete = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n c\n";
    assert!(Patch::parse(complete).is_ok());
    assert_eq!(
        Patch::parse(complete.trim_end_matches(" c\n")),
        Err(PatchError::Short { line: 3 })
    );
}

#[test]
fn a_line_past_its_hunk_s_counts_is_refused_and_the_hunk_without_it_is_admitted() {
    let within = "--- a/x\n+++ b/x\n@@ -1,2 +1,1 @@\n-a\n-b\n+c\n";
    assert!(Patch::parse(within).is_ok());
    let past = within.replace("+c", "-c");
    assert_eq!(
        Patch::parse(&past),
        Err(PatchError::Over { line: 6, hunk: 3 })
    );
}

#[test]
fn a_hunk_line_with_an_unknown_prefix_is_refused_and_one_with_a_space_is_admitted() {
    let file = |line: &str| format!("--- a/x\n+++ b/x\n@@ -1 +1 @@\n{line}\n");
    assert_eq!(
        Patch::parse(&file("*a")),
        Err(PatchError::Stray { line: 4, hunk: 3 })
    );
    assert!(Patch::parse(&file(" a")).is_ok());
}

#[test]
fn a_patch_over_the_line_limit_is_refused_and_one_at_the_limit_is_admitted() {
    let text = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    assert!(Patch::parse_within(text, 5).is_ok());
    assert_eq!(
        Patch::parse_within(text, 4),
        Err(PatchError::TooLong { max: 4 })
    );
}

#[test]
fn a_change_to_a_line_ending_alone_shows_the_carriage_return_escaped() {
    let patch = Patch::parse("--- a/x\r\n+++ b/x\r\n@@ -1 +1 @@\r\n-a\r\n+a\n").unwrap();
    let file = &patch.files[0];
    assert_eq!(file.new.as_deref(), Some("x"));
    let shown: Vec<_> = file.hunks[0]
        .lines
        .iter()
        .map(|line| (line.hidden, line.text.as_str()))
        .collect();
    assert_eq!(shown, [(true, "a\\r"), (false, "a")]);
}

#[test]
fn quoted_git_paths_are_unquoted_and_lose_their_prefixes() {
    let text = "diff --git \"a/caf\\303\\251\\tx\" \"b/caf\\303\\251\\tx\"\n--- \"a/caf\\303\\251\\tx\"\n+++ \"b/caf\\303\\251\\tx\"\n@@ -1 +1 @@\n-a\n+b\n";
    let patch = Patch::parse(text).unwrap();
    assert_eq!(patch.files[0].new.as_deref(), Some("café\\tx"));
    assert_eq!(patch.files[0].old.as_deref(), Some("café\\tx"));
}

#[test]
fn a_quoted_path_keeps_an_escaped_quote_and_a_short_octal_escape() {
    let text = "--- \"a/x\\\"y\\1z\"\n+++ \"b/x\\\"y\\1z\"\n@@ -1 +1 @@\n-a\n+b\n";
    let patch = Patch::parse(text).unwrap();
    assert_eq!(patch.files[0].new.as_deref(), Some("x\"y\\u{1}z"));
}

#[test]
fn a_quoted_binary_file_is_named_from_its_diff_git_header() {
    let quoted =
        "diff --git \"a/caf\\303\\251.png\" \"b/caf\\303\\251.png\"\nBinary files differ\n";
    let plain = "diff --git a/cafe.png b/cafe.png\nBinary files differ\n";
    let quoted = Patch::parse(quoted).unwrap();
    let plain = Patch::parse(plain).unwrap();
    assert_eq!(
        (
            quoted.files[0].old.as_deref(),
            quoted.files[0].new.as_deref()
        ),
        (Some("café.png"), Some("café.png"))
    );
    assert_eq!(plain.files[0].new.as_deref(), Some("cafe.png"));
}

#[test]
fn a_blank_context_line_of_a_crlf_patch_is_context_and_a_stray_cr_line_is_refused() {
    let blank = "--- a/x\r\n+++ b/x\r\n@@ -1,2 +1,2 @@\r\n\r\n-a\r\n+b\r\n";
    let stray = "--- a/x\r\n+++ b/x\r\n@@ -1,2 +1,2 @@\r\n\rq\r\n-a\r\n+b\r\n";
    let blank = Patch::parse(blank).unwrap();
    assert_eq!(blank.files[0].hunks[0].lines[0].change, Change::Context);
    assert_eq!(
        Patch::parse(stray),
        Err(PatchError::Stray { line: 4, hunk: 3 })
    );
}

#[test]
fn a_copy_names_both_files_and_a_mode_change_names_both_modes() {
    let text = "\
diff --git a/a.sh b/b.sh
similarity index 100%
copy from a.sh
copy to b.sh
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
";
    let patch = Patch::parse(text).unwrap();
    let [copy, mode] = &patch.files[..] else {
        panic!("{patch:?}")
    };
    assert_eq!(
        (copy.old.as_deref(), copy.new.as_deref(), copy.copied),
        (Some("a.sh"), Some("b.sh"), true)
    );
    assert_eq!(
        (
            mode.old_mode.as_deref(),
            mode.new_mode.as_deref(),
            mode.copied
        ),
        (Some("100644"), Some("100755"), false)
    );
}

#[test]
fn parsing_any_bytes_never_panics_and_counts_each_added_and_removed_line() {
    bolero::check!().for_each(|bytes: &[u8]| {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if let Ok(patch) = Patch::parse(text) {
            for file in &patch.files {
                let lines = file.hunks.iter().flat_map(|hunk| &hunk.lines);
                let added = lines.clone().filter(|line| line.change == Change::Added);
                let removed = lines.filter(|line| line.change == Change::Removed);
                assert_eq!((added.count(), removed.count()), (file.added, file.removed));
            }
        }
    });
}

#[test]
fn a_line_holding_a_bidi_control_is_escaped_and_marked_and_a_tab_indented_line_is_kept() {
    let text =
        "--- a/x\u{202e}y\n+++ b/x\u{202e}y\n@@ -1 +1,2 @@\n-\tok\n+\tif admin \u{202e}{\n+\tok\n";
    let patch = Patch::parse(text).unwrap();
    let file = &patch.files[0];
    assert_eq!(file.new.as_deref(), Some("x\\u{202e}y"));
    let shown: Vec<_> = file.hunks[0]
        .lines
        .iter()
        .map(|line| (line.hidden, line.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (false, "\tok"),
            (true, "\tif admin \\u{202e}{"),
            (false, "\tok"),
        ]
    );
}
